//! 本地 OCR 引擎：PaddleOCR 的 PP-OCRv4 mobile 模型跑在 ONNX Runtime 上。
//!
//! 为什么不用系统自带的 Windows.Media.OCR：它认不了「紧框一行」的细长条图
//! （本机实测 700x26 直接返回 0 行），中文长句也常丢字。RapidOCR 的这套模型是
//! 中文场景里公认好用的那一档，模型随安装包分发，识别完全在本机完成、不需要联网。
//!
//! 模型放在安装目录的 ocr/ 下（见 tauri.conf.json 的 bundle.resources），
//! 开发时回落到源码树里的 src-tauri/ocr/。找不到模型就返回 Err，由调用方回落到
//! Windows OCR —— 缺模型不该让截图识别整个不可用。

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use image::RgbImage;
use paddle_ocr_rs::ocr_lite::OcrLite;

use crate::screenshot::OcrLine;

/// 检测前在图片外围补一圈边框，能给 DBNet 更多上下文（RapidOCR 的默认值）
const PADDING: u32 = 50;
/// 检测输入的最大边长。超过就等比缩小，坐标由库自己映射回原图
const MAX_SIDE_LEN: u32 = 1600;
/// 判定「这里像文字」的分值下限
const BOX_SCORE_THRESH: f32 = 0.5;
/// 文字框的置信度下限
const BOX_THRESH: f32 = 0.3;
/// 文本框外扩比例：DB 出来的框略小于真实文字，往外放一点再交给识别
const UNCLIP_RATIO: f32 = 2.0;

struct EngineState {
    /// 初始化成功后就常驻，别每次识别重建会话（重建要一秒左右）
    engine: Option<OcrLite>,
    /// 初始化失败的原因。记下来，免得每次识别都白等一次初始化
    failed: Option<String>,
}

fn state() -> &'static Mutex<EngineState> {
    static ENGINE: OnceLock<Mutex<EngineState>> = OnceLock::new();
    ENGINE.get_or_init(|| {
        Mutex::new(EngineState {
            engine: None,
            failed: None,
        })
    })
}

/// 模型目录。安装后是 <资源目录>/ocr，开发时是源码树里的 src-tauri/ocr。
fn model_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = app.path().resource_dir() {
        candidates.push(dir.join("ocr"));
        candidates.push(dir.join("resources").join("ocr"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("ocr"));
        }
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ocr"));
    candidates
        .into_iter()
        .find(|dir| dir.join("det.onnx").exists() && dir.join("rec.onnx").exists())
}

fn init_engine(dir: &std::path::Path) -> Result<OcrLite, String> {
    ensure_runtime(&dir)?;
    let path = |name: &str| dir.join(name).to_string_lossy().to_string();
    let threads = std::thread::available_parallelism()
        .map(|n| n.get().min(4))
        .unwrap_or(2);
    let mut engine = OcrLite::new();
    engine
        .init_models(&path("det.onnx"), &path("cls.onnx"), &path("rec.onnx"), threads)
        .map_err(|e| format!("加载 OCR 模型失败: {e}"))?;
    crate::selection::log_line(&format!(
        "ocr: PaddleOCR 就绪（{} 线程，模型目录 {}）",
        threads,
        dir.display()
    ));
    Ok(engine)
}

/// 显式加载随包的 ONNX Runtime。
///
/// ort 默认要么在构建期下载二进制、要么让它自己按系统路径找 DLL，两者在「装到别处」
/// 或「离线构建」时都容易出事。这里把 DLL 跟模型放在一起，运行时按自己的路径加载：
/// 结果用 OnceLock 记住，重复调用不会重复加载。
fn ensure_runtime(dir: &std::path::Path) -> Result<(), String> {
    static READY: OnceLock<Result<(), String>> = OnceLock::new();
    READY
        .get_or_init(|| {
            let dll = dir.join("onnxruntime.dll");
            if !dll.exists() {
                return Err(format!("缺少 ONNX Runtime：{}", dll.display()));
            }
            /*
             * 先自己探一次能不能加载。
             *
             * ort 在「DLL 加载失败」时是直接 panic，而 release 构建是 panic=abort：
             * 一个损坏或被换掉的 DLL 会把整个随译带走，连回落的机会都没有。
             * 这里先用 libloading 探一次，失败就干净地返回 Err，走系统 OCR 兜底。
             */
            {
                let probe = unsafe { libloading::Library::new(&dll) }
                    .map_err(|e| format!("ONNX Runtime 无法加载（{}）：{e}", dll.display()))?;
                drop(probe);
            }
            // rc.10 的 init_from 只登记路径，真正的加载发生在 commit 里；
            // 路径不存在或加载失败会在后面建会话时报错，这里先把路径喂进去。
            let _ = ort::init_from(dll.to_string_lossy().to_string()).commit();
            crate::selection::log_line(&format!("ocr: ONNX Runtime 已加载（{}）", dll.display()));
            Ok(())
        })
        .clone()
}

/// 后台预热：第一次截图不必等模型加载（约一秒）。失败只是记一笔，不影响功能。
pub fn warm_up(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut guard = match state().lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if guard.engine.is_some() {
            return;
        }
        let Some(dir) = model_dir(&app) else {
            crate::selection::log_line("ocr: 没有本地 OCR 模型，截图识别会用系统 OCR");
            return;
        };
        match init_engine(&dir) {
            Ok(e) => guard.engine = Some(e),
            Err(e) => {
                crate::selection::log_line(&format!("ocr: 预热失败（{e}），截图时再试一次"));
            }
        }
    });
}

/// 识别一张 RGB 图，返回行文本与它在图里的像素矩形（左上角为原点）。
///
/// 失败时返回 Err：调用方据此回落到 Windows OCR，而不是把「没认出来」当成空结果。
pub fn recognize(app: &tauri::AppHandle, img: &RgbImage) -> Result<Vec<OcrLine>, String> {
    let dir = model_dir(app).ok_or("找不到 OCR 模型目录（ocr/det.onnx 与 rec.onnx）")?;
    recognize_at(&dir, img)
}

/// 同 recognize，但直接给模型目录 —— 单测拿不到 AppHandle，用这个入口
pub fn recognize_at(dir: &std::path::Path, img: &RgbImage) -> Result<Vec<OcrLine>, String> {
    let mut guard = state().lock().map_err(|_| "OCR 引擎状态异常".to_string())?;
    if guard.engine.is_none() {
        if let Some(reason) = guard.failed.clone() {
            return Err(reason);
        }
        match init_engine(dir) {
            Ok(e) => guard.engine = Some(e),
            Err(e) => {
                crate::selection::log_line(&format!("ocr: PaddleOCR 初始化失败（{e}），回落系统 OCR"));
                guard.failed = Some(e.clone());
                return Err(e);
            }
        }
    }
    let engine = guard.engine.as_mut().ok_or("OCR 引擎未就绪")?;
    let result = engine
        .detect(
            img,
            PADDING,
            MAX_SIDE_LEN,
            BOX_SCORE_THRESH,
            BOX_THRESH,
            UNCLIP_RATIO,
            false, // 屏幕上的文字基本是正的，不做整图角度检测，省一半时间
            false,
        )
        .map_err(|e| format!("PaddleOCR 识别失败: {e}"))?;

    let mut lines: Vec<OcrLine> = result
        .text_blocks
        .into_iter()
        .filter(|b| !b.text.trim().is_empty())
        .map(|b| {
            // 四角坐标取轴对齐包围盒：结果面板按行贴译文只需要矩形
            let xs: Vec<u32> = b.box_points.iter().map(|p| p.x).collect();
            let ys: Vec<u32> = b.box_points.iter().map(|p| p.y).collect();
            let (min_x, max_x) = (
                xs.iter().copied().min().unwrap_or(0),
                xs.iter().copied().max().unwrap_or(0),
            );
            let (min_y, max_y) = (
                ys.iter().copied().min().unwrap_or(0),
                ys.iter().copied().max().unwrap_or(0),
            );
            OcrLine {
                text: b.text,
                x: min_x as f64,
                y: min_y as f64,
                w: (max_x.saturating_sub(min_x)) as f64,
                h: (max_y.saturating_sub(min_y)) as f64,
            }
        })
        .collect();
    // 按阅读顺序排：先按 y 分行（同一条带算一行），带内按 x 从左到右
    lines.sort_by(|a, b| {
        if (a.y - b.y).abs() < 14.0 {
            a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal)
        } else {
            a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal)
        }
    });
    Ok(lines)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// 端到端跑一遍固定样本：COM→ORT→检测→识别四段都在里面。
    /// 模型没拉（还没跑 `pnpm ocr:assets`）时跳过而不是失败，沿用系统 OCR 用例的约定。
    #[test]
    fn paddle_reads_the_fixture_sample() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ocr");
        if !dir.join("det.onnx").exists() || !dir.join("onnxruntime.dll").exists() {
            eprintln!("跳过：没有本地 OCR 模型（先跑 pnpm ocr:assets）");
            return;
        }
        let png = include_bytes!("../tests/fixtures/ocr-sample.png");
        let img = image::load_from_memory_with_format(png, image::ImageFormat::Png)
            .expect("样本图应能解码")
            .to_rgb8();
        let lines = match recognize_at(&dir, &img) {
            Ok(l) => l,
            Err(e) => panic!("PaddleOCR 调用失败: {e}"),
        };
        let text = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        // PP-OCRv4 中文模型同时认中英，样本里的两行都该出来
        let lower = text.to_lowercase();
        assert!(
            lower.contains("hello") || lower.contains("world"),
            "没认出样本里的英文，实际输出: {text:?}"
        );
        assert!(
            text.contains("随") || text.contains("译"),
            "没认出样本里的中文，实际输出: {text:?}"
        );
    }
}
