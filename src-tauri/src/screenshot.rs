//! M2 截图识别：Alt+S 冻结屏幕 → 覆盖层框选 → 裁剪 → Windows.Media.OCR → 弹窗翻译
//!
//! 流程：
//! 1. 热键触发：把所有显示器的并集矩形当一块画布，用 GDI 一次抓完（冻结帧），JPEG 编码存入会话；
//! 2. 在该并集矩形上铺一层无边框置顶覆盖层，前端加载冻结帧供框选；
//! 3. 松开鼠标：前端回传逻辑坐标 → Rust 换算物理坐标裁剪原图 → OCR；
//! 4. 隐藏覆盖层，OCR 文本投递给划词弹窗自动翻译。
//!
//! 多屏策略：整抓虚拟桌面（所有显示器的并集），覆盖层也铺满并集，因此可以在任意一块屏
//! 起框、跨屏拖选，和 PixPin 的手感一致。混合 DPI 下帧图仍按物理像素 1:1：
//! 覆盖层窗口只有一个小数缩放系数，帧图铺满窗口即可，裁剪时乘这个系数换算成物理像素。

use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

/// 进行中的截图会话（同一时刻最多一个）
pub struct ShotSession {
    /// 抓取区域（物理像素）：x, y, w, h，等于所有显示器的并集矩形
    pub vs: (i32, i32, i32, i32),
    /// 覆盖层窗口的缩放系数（show_overlay 时写入）。前端给的 clientX/Y 是 CSS 像素，
    /// 乘它就是相对于并集原点的物理像素。
    pub sf: f64,
    pub w: i32,
    pub h: i32,
    pub rgba: Vec<u8>,
    /// 冻结帧 JPEG 的 data URL（前端展示用）
    pub data_url: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenshotPayload {
    pub data_url: String,
    pub vs_x: i32,
    pub vs_y: i32,
    pub vs_w: i32,
    pub vs_h: i32,
    pub scale_hint: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrLine {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrPayload {
    pub text: String,
    pub lines: Vec<OcrLine>,
}

/// 一次截图识别的完整结果，供结果面板使用。
///
/// 有了裁出来的那块图与每行的矩形，「原图覆盖」才能把译文画回原来的位置：
/// 行坐标与 crop_url 是同一个像素空间，前端按百分比定位即可，不用碰 DPI 换算。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrResult {
    /// 裁剪后的选区图（PNG data URL）
    pub crop_url: String,
    pub crop_w: i32,
    pub crop_h: i32,
    /// 识别出的原文
    pub text: String,
    /// 行级矩形，坐标系与 crop_url 一致
    pub lines: Vec<OcrLine>,
    /// 识别引擎：windows 或 plugin:插件名
    pub engine: String,
    /// 识别用的语言标签（BCP-47）。系统 OCR 走 en-US，插件不报语言时为空
    pub lang: String,
    /// 识别失败的原因。为空表示识别流程本身跑完了（哪怕一个字都没认出来）
    pub error: Option<String>,
}

/// 最近一次识别结果。结果窗口挂载时直接取它，不依赖事件先到
#[derive(Default)]
pub struct OcrState(pub Mutex<Option<OcrResult>>);

/// OCR 输入区域相对选区的外扩量（图像像素）。
///
/// Windows OCR 认不了「紧框一行」的条状图：本机实测 700x26 与 420x26 都返回 0 行，
/// 同一张图上下各外扩 32px 后立刻能认出来。框一行文字恰恰是最常见的用法，
/// 所以先外扩再识别；外扩带进来的邻行随后按矩形过滤掉，不会混进译文。
const OCR_PAD: i32 = 32;

/// 外扩后的区域还这么小，就先放大再识别（小字对 Windows OCR 同样不友好）
const OCR_UPSCALE_IF_BELOW: i32 = 480;

/// 识别区域的外扩比。识别一次约 100~300ms，最多试三遍
const OCR_MAX_SCALE: u32 = 2;

/// 诊断落盘：识别为空时把裁剪图写到配置目录，方便事后查为什么没认出来
const EMPTY_CROP_FILE: &str = "last-crop.png";

/// 结果面板尺寸，与设计稿的第 3 节面板接近
const OCR_W: f64 = 460.0;
const OCR_H: f64 = 600.0;

/// 在光标附近弹出截图识别结果面板
fn show_ocr_panel(app: &AppHandle) -> Result<(), String> {
    let cursor = app.cursor_position().map_err(|e| e.to_string())?;
    // 面板跟着鼠标所在的显示器走。多屏下如果按主屏边界去 clamp，
    // 在副屏框选的用户会看到面板跳到另一块屏上。
    let target = app
        .monitor_from_point(cursor.x, cursor.y)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());

    // 光标右下方一点，不压住刚框出来的那块
    let mut lx = cursor.x + 16.0;
    let mut ly = cursor.y + 16.0;
    if let Some(m) = &target {
        let sf = m.scale_factor();
        let pos = m.position();
        let size = m.size();
        // 该显示器矩形在逻辑坐标下的范围
        let mx = pos.x as f64 / sf;
        let my = pos.y as f64 / sf;
        let mw = size.width as f64 / sf;
        let mh = size.height as f64 / sf;
        lx = cursor.x / sf + 16.0;
        ly = cursor.y / sf + 16.0;
        if lx + OCR_W > mx + mw {
            lx = mx + mw - OCR_W - 16.0;
        }
        if ly + OCR_H > my + mh {
            ly = my + mh - OCR_H - 16.0;
        }
        lx = lx.max(mx + 8.0);
        ly = ly.max(my + 8.0);
    }
    let (lx, ly) = (lx.max(8.0), ly.max(8.0));

    match app.get_webview_window("ocr") {
        Some(win) => {
            let _ = win.set_position(tauri::LogicalPosition::new(lx, ly));
            let _ = win.show();
            let _ = win.set_focus();
            Ok(())
        }
        None => WebviewWindowBuilder::new(app, "ocr", WebviewUrl::App("ocr.html".into()))
            .title("随译 · 截图识别")
            .inner_size(OCR_W, OCR_H)
            .position(lx, ly)
            .min_inner_size(380.0, 360.0)
            .decorations(false)
            .transparent(true)
            .shadow(true)
            .resizable(true)
            .effects(tauri::utils::config::WindowEffectsConfig {
                effects: vec![tauri::utils::WindowEffect::Acrylic],
                state: None,
                radius: Some(14.0),
                color: None,
                interactive: false,
            })
            .build()
            .map(|_| ())
            .map_err(|e| format!("创建结果面板失败: {e}")),
    }
}

/// 热键入口
pub fn trigger_screenshot(app: AppHandle) {
    crate::selection::log_line("screenshot: Alt+S 触发");
    std::thread::spawn(move || {
        run_screenshot(app);
    });
}

/// 前端按钮入口（与热键同流程）
#[tauri::command]
pub fn start_screenshot(app: AppHandle) -> Result<(), String> {
    // 上一轮的识别面板先收起来，别让新框选和旧结果同屏
    if let Some(win) = app.get_webview_window("ocr") {
        let _ = win.hide();
    }
    trigger_screenshot(app);
    Ok(())
}

/// 最近一次识别结果。结果面板挂载时先取它，避免事件比页面先到而丢内容。
#[tauri::command]
pub fn ocr_last(ocr_state: tauri::State<'_, OcrState>) -> Option<OcrResult> {
    ocr_state.0.lock().ok().and_then(|g| g.clone())
}

/// 关掉结果面板并清掉暂存结果
#[tauri::command]
pub fn ocr_close(app: AppHandle, ocr_state: tauri::State<'_, OcrState>) {
    if let Some(win) = app.get_webview_window("ocr") {
        let _ = win.hide();
    }
    if let Ok(mut g) = ocr_state.0.lock() {
        *g = None;
    }
}

fn run_screenshot(app: AppHandle) {
    let session = match build_session(&app) {
        Ok(s) => s,
        Err(e) => {
            crate::selection::log_line(&format!("screenshot: 抓屏失败 {e}"));
            return;
        }
    };

    // 会话必须写进 Tauri 托管状态：命令层（get_screenshot / finish_region）读的是同一份。
    // 不要再引入第二个 static 存储，否则覆盖层取不到冻结帧，框选也无法裁剪。
    let (w, h, sf) = (session.w, session.h, session.sf);
    let state = app.state::<Mutex<Option<ShotSession>>>();
    match state.lock() {
        Ok(mut g) => *g = Some(session),
        Err(e) => {
            crate::selection::log_line(&format!("screenshot: 会话写入失败 {e}"));
            return;
        }
    }
    crate::selection::log_line(&format!("screenshot: 冻结帧就绪 {w}x{h} sf={sf}"));

    if std::env::var_os("SUIYI_DEBUG_FRAME").is_some() {
        dump_frame(&app);
    }

    if let Err(e) = show_overlay(&app) {
        crate::selection::log_line(&format!("screenshot: 覆盖层创建失败 {e}"));
    }
}

/// 多块显示器的并集矩形（物理像素）。负坐标（屏幕在主的左侧/上方）与屏幕之间的空隙
/// 都按原样保留：BitBlt 抓的就是整块虚拟桌面，空隙会抓成黑色，用户也不会去框那里。
/// 抽成纯函数是为了能单测——这行算式错了，跨屏裁剪就会整块错位。
fn union_rect(rects: &[(i32, i32, u32, u32)]) -> Option<(i32, i32, i32, i32)> {
    let mut it = rects.iter();
    let (x0, y0, w0, h0) = *it.next()?;
    let (mut min_x, mut min_y) = (x0, y0);
    let (mut max_x, mut max_y) = (x0 + w0 as i32, y0 + h0 as i32);
    for (x, y, w, h) in it {
        min_x = min_x.min(*x);
        min_y = min_y.min(*y);
        max_x = max_x.max(*x + *w as i32);
        max_y = max_y.max(*y + *h as i32);
    }
    Some((min_x, min_y, max_x - min_x, max_y - min_y))
}

/// 抓取所有显示器并集区域的画面，组装一次截图会话
fn build_session(app: &AppHandle) -> Result<ShotSession, String> {
    let monitors = app
        .available_monitors()
        .map_err(|e| format!("枚举显示器失败: {e}"))?;
    let rects: Vec<(i32, i32, u32, u32)> = monitors
        .iter()
        .map(|m| {
            let p = m.position();
            let s = m.size();
            (p.x, p.y, s.width, s.height)
        })
        .collect();
    let (vx, vy, vw, vh) = union_rect(&rects).ok_or("没有检测到显示器")?;
    if vw <= 0 || vh <= 0 {
        return Err(format!("显示器尺寸无效: {vw}x{vh}"));
    }

    let rgba = capture_rect(vx, vy, vw, vh)?;
    let data_url = rgba_to_jpeg_dataurl(&rgba, vw, vh)?;
    // 先按主屏缩放系数顶着，真正的值在覆盖层窗口建好之后写入（见 show_overlay）
    let sf = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);
    crate::selection::log_line(&format!(
        "screenshot: 抓取虚拟桌面 ({vx},{vy}) {vw}x{vh}（{} 块屏）",
        rects.len()
    ));
    Ok(ShotSession {
        vs: (vx, vy, vw, vh),
        sf,
        w: vw,
        h: vh,
        rgba,
        data_url,
    })
}

/// 诊断辅助：把冻结帧写到 `%APPDATA%/com.suiyi.dev/last-shot.jpg`。
/// 只在设置了环境变量 `SUIYI_DEBUG_FRAME` 时执行，平时不产生磁盘写入。
fn dump_frame(app: &AppHandle) {
    let state = app.state::<Mutex<Option<ShotSession>>>();
    let Ok(g) = state.lock() else { return };
    let Some(s) = g.as_ref() else { return };
    let Some(img) = image::RgbaImage::from_raw(s.w as u32, s.h as u32, s.rgba.clone()) else {
        return;
    };
    let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
    let path = std::env::var("APPDATA")
        .map(|d| std::path::Path::new(&d).join("com.suiyi.dev").join("last-shot.jpg"))
        .unwrap_or_default();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match rgb.save(&path) {
        Ok(_) => crate::selection::log_line(&format!("screenshot: 冻结帧已保存 → {}", path.display())),
        Err(e) => crate::selection::log_line(&format!("screenshot: 冻结帧保存失败 {e}")),
    }
}

/// 创建/复用覆盖层窗口，正好盖住鼠标所在的那块显示器
fn show_overlay(app: &AppHandle) -> Result<(), String> {
    let (vsx, vsy, vsw, vsh) = {
        let state = app.state::<Mutex<Option<ShotSession>>>();
        let g = state.lock().map_err(|e| e.to_string())?;
        let s = g.as_ref().ok_or("会话不存在")?;
        (s.vs.0, s.vs.1, s.vs.2, s.vs.3)
    };

    let win = match app.get_webview_window("overlay") {
        Some(win) => win,
        None => WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("overlay.html".into()))
            .title("随译 · 截图框选")
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .maximizable(false)
            .visible(false)
            .build()
            .map_err(|e| format!("创建覆盖层失败: {e}"))?,
    };

    // 用物理像素定位与定尺寸：窗口正好贴合显示器矩形，不经过逻辑坐标换算，
    // 多屏与混合 DPI 下都不会偏。
    win.set_position(tauri::PhysicalPosition::new(vsx, vsy))
        .map_err(|e| format!("覆盖层定位失败: {e}"))?;
    // 目标：客户区精确等于显示器尺寸。set_size 给的是窗口外框尺寸，带边框或阴影时
    // 客户区会差几像素；这里量一次差多少再补回去，最多迭代三轮。
    let mut req_w = vsw.max(1);
    let mut req_h = vsh.max(1);
    for _ in 0..3 {
        win.set_size(tauri::PhysicalSize::new(req_w as u32, req_h as u32))
            .map_err(|e| format!("覆盖层设定尺寸失败: {e}"))?;
        let inner = win
            .inner_size()
            .map_err(|e| format!("读取覆盖层尺寸失败: {e}"))?;
        let dw = inner.width as i32 - vsw;
        let dh = inner.height as i32 - vsh;
        if dw == 0 && dh == 0 {
            break;
        }
        req_w = (req_w - dw).max(1);
        req_h = (req_h - dh).max(1);
    }
    win.show().map_err(|e| format!("覆盖层显示失败: {e}"))?;
    win.set_focus().map_err(|e| format!("覆盖层聚焦失败: {e}"))?;

    // 覆盖层窗口自己的缩放系数：前端回传的是 CSS 像素，乘它才是物理像素偏移。
    // 混合 DPI 下窗口只有一个系数，而帧图铺满窗口正好是 1:1 物理像素，所以裁剪不会错位。
    let sf = win.scale_factor().unwrap_or(1.0);
    if let Ok(mut g) = app.state::<Mutex<Option<ShotSession>>>().lock() {
        if let Some(s) = g.as_mut() {
            s.sf = sf;
        }
    }
    log_overlay_geometry(&win, sf, vsx, vsy, vsw, vsh);
    Ok(())
}

/// 记录覆盖层窗口的实际几何，用于核对定位是否与显示器矩形一致
fn log_overlay_geometry(
    win: &tauri::WebviewWindow,
    sf: f64,
    vsx: i32,
    vsy: i32,
    vsw: i32,
    vsh: i32,
) {
    let outer = win
        .outer_position()
        .map(|p| format!("({},{})", p.x, p.y))
        .unwrap_or_else(|e| format!("读取失败 {e}"));
    let inner = win
        .inner_size()
        .map(|s| format!("{}x{}", s.width, s.height))
        .unwrap_or_else(|e| format!("读取失败 {e}"));
    crate::selection::log_line(&format!(
        "screenshot: 覆盖层 outer={outer} inner={inner} 期望物理=({vsx},{vsy}) {vsw}x{vsh} sf={sf}"
    ));
}

fn hide_overlay(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("overlay") {
        let _ = win.hide();
    }
}

// ==================== 前端命令 ====================

/// 覆盖层加载后获取冻结帧
#[tauri::command]
pub fn get_screenshot(state: tauri::State<'_, Mutex<Option<ShotSession>>>) -> Result<ScreenshotPayload, String> {
    let g = state.lock().map_err(|e| e.to_string())?;
    let s = g.as_ref().ok_or("没有进行中的截图会话")?;
    Ok(ScreenshotPayload {
        data_url: s.data_url.clone(),
        vs_x: s.vs.0,
        vs_y: s.vs.1,
        vs_w: s.vs.2,
        vs_h: s.vs.3,
        scale_hint: s.sf,
    })
}

/// 取消框选
#[tauri::command]
pub fn cancel_screenshot(
    app: AppHandle,
    state: tauri::State<'_, Mutex<Option<ShotSession>>>,
) -> Result<(), String> {
    hide_overlay(&app);
    if let Ok(mut g) = state.lock() {
        *g = None; // 取消即释放冻结帧，避免整屏位图常驻内存
    }
    Ok(())
}

// ==================== 选区几何与 OCR 预处理 ====================

/// 矩形（图像像素）：x, y, w, h
type Rect = (i32, i32, i32, i32);

/// 把选区向外扩 pad 像素，并裁剪到帧内
fn expand_rect(sel: Rect, vw: i32, vh: i32, pad: i32) -> Rect {
    let x = (sel.0 - pad).max(0);
    let y = (sel.1 - pad).max(0);
    let x2 = (sel.0 + sel.2 + pad).min(vw);
    let y2 = (sel.1 + sel.3 + pad).min(vh);
    (x, y, (x2 - x).max(1), (y2 - y).max(1))
}

/// 从整帧 RGBA 里裁一块（逐行拷贝）
fn crop_rgba(rgba: &[u8], stride: i32, r: Rect) -> Vec<u8> {
    let mut out = Vec::with_capacity((r.2 * r.3 * 4) as usize);
    for row in 0..r.3 {
        let start = ((r.1 + row) * stride + r.0) as usize * 4;
        let end = start + r.2 as usize * 4;
        out.extend_from_slice(&rgba[start..end]);
    }
    out
}

/// 这一行算不算「用户框到的」：重叠面积占该行 30% 以上，或者行中心落在选区内。
/// 外扩会把邻行一起送给引擎，靠这个把它们摘掉。
fn line_belongs(line: (f64, f64, f64, f64), sel: (f64, f64, f64, f64)) -> bool {
    let (lx, ly, lw, lh) = line;
    let (sx, sy, sw, sh) = sel;
    let ix = (lx + lw).min(sx + sw) - lx.max(sx);
    let iy = (ly + lh).min(sy + sh) - ly.max(sy);
    let inter = ix.max(0.0) * iy.max(0.0);
    if inter / (lw * lh).max(1.0) >= 0.3 {
        return true;
    }
    let cx = lx + lw / 2.0;
    let cy = ly + lh / 2.0;
    cx >= sx && cx <= sx + sw && cy >= sy && cy <= sy + sh
}

/// 放大若干倍：小图上的小字识别率明显更差
fn scale_rgba(rgba: &[u8], w: i32, h: i32, factor: u32) -> Vec<u8> {
    let Some(img) = image::RgbaImage::from_raw(w as u32, h as u32, rgba.to_vec()) else {
        return rgba.to_vec();
    };
    let out = image::imageops::resize(
        &img,
        (w as u32) * factor,
        (h as u32) * factor,
        image::imageops::FilterType::Triangle,
    );
    out.into_raw()
}

/// 反色（只翻 RGB，alpha 保持）
fn invert_rgba(rgba: &[u8]) -> Vec<u8> {
    let mut out = rgba.to_vec();
    for px in out.chunks_exact_mut(4) {
        px[0] = 255 - px[0];
        px[1] = 255 - px[1];
        px[2] = 255 - px[2];
    }
    out
}

fn char_count(lines: &[OcrLine]) -> usize {
    lines.iter().map(|l| l.text.chars().count()).sum()
}

/// 识别一块 RGBA，返回 (行, 实际用上的语言标签)
fn ocr_rgba(rgba: &[u8], w: i32, h: i32, lang: &str) -> Result<(Vec<OcrLine>, String), String> {
    let png = rgba_to_png_bytes(rgba, w, h)?;
    ocr_bytes_lang(&png, lang)
}

/// 依次试原图 → 2 倍放大 → 反色，一旦认出行就不再折腾，返回实际用的放大倍数。
///
/// 三遍都认不出才返回 Err（真正的识别故障）；认出来但内容为空是正常结果。
fn ocr_best(
    rgba: &[u8],
    w: i32,
    h: i32,
    lang: &str,
) -> Result<(Vec<OcrLine>, String, u32), String> {
    let mut best: Option<(Vec<OcrLine>, String, u32)> = None;
    let mut last_err: Option<String> = None;
    for stage in 0..3u32 {
        let (buf, aw, ah, scale) = match stage {
            0 => (rgba.to_vec(), w, h, 1),
            1 => (
                scale_rgba(rgba, w, h, OCR_MAX_SCALE),
                w * OCR_MAX_SCALE as i32,
                h * OCR_MAX_SCALE as i32,
                OCR_MAX_SCALE,
            ),
            _ => (invert_rgba(rgba), w, h, 1),
        };
        match ocr_rgba(&buf, aw, ah, lang) {
            Ok((lines, used)) => {
                let count = char_count(&lines);
                if best
                    .as_ref()
                    .map(|(b, _, _)| char_count(b) < count)
                    .unwrap_or(true)
                {
                    best = Some((lines, used, scale));
                }
                if count > 0 {
                    break;
                }
            }
            Err(e) => last_err = Some(e),
        }
    }
    best.ok_or_else(|| last_err.unwrap_or_else(|| "OCR 没有返回结果".into()))
}

/// 一个字都没认出来时，把送进引擎的那块图存到配置目录：用户反馈"识别不出来"时
/// 有据可查，不用再靠猜。
fn dump_empty_crop(app: &AppHandle, png: &[u8]) {
    let Ok(dir) = app.path().app_config_dir() else {
        return;
    };
    let path = dir.join(EMPTY_CROP_FILE);
    match std::fs::write(&path, png) {
        Ok(()) => crate::selection::log_line(&format!(
            "screenshot: 未识别到文字，选区图已存 → {}",
            path.display()
        )),
        Err(e) => crate::selection::log_line(&format!("screenshot: 选区图保存失败 {e}")),
    }
}

/// 框选完成：帧图像素坐标 → 裁剪 → OCR → 投递结果面板。
///
/// 前端回传的 x/y/w/h 是**冻结帧自身的像素坐标**（覆盖层按 <img> 的渲染框与
/// naturalWidth/naturalHeight 换算出来的），与窗口 DPI、滚动条、多屏混合缩放无关，
/// 所以这里不做任何缩放系数换算。
///
/// 这个命令必须是 async：里面的裁剪、PNG 编码和 WinRT OCR 都是同步阻塞活儿，
/// 同步命令跑在主线程上，会把整个应用（包括刚建出来的结果面板）一起冻住 ——
/// 表现就是「窗口白屏、点不动、只能任务管理器杀掉」。
#[tauri::command]
pub async fn finish_region(
    app: AppHandle,
    state: tauri::State<'_, Mutex<Option<ShotSession>>>,
    ocr_state: tauri::State<'_, OcrState>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<OcrPayload, String> {
    let (session, crop_png, crop_w, crop_h, region_rgba, region_w, region_h, sel_in_region) = {
        let mut g = state.lock().map_err(|e| e.to_string())?;
        let s = g.as_mut().ok_or("没有进行中的截图会话")?;
        let sx = (x.round() as i32).clamp(0, s.w);
        let sy = (y.round() as i32).clamp(0, s.h);
        let sw = (w.round() as i32).clamp(0, s.w - sx);
        let sh = (h.round() as i32).clamp(0, s.h - sy);
        if sw < 4 || sh < 4 {
            return Err("选区太小".into());
        }
        let tight = (sx, sy, sw, sh);
        // 紧框图：面板展示与「原图覆盖」用，行矩形也在这个坐标系里
        let crop = crop_rgba(&s.rgba, s.w, tight);
        let crop_png = rgba_to_png_bytes(&crop, sw, sh)?;
        // OCR 输入：向外扩一圈。紧框单行的条状图 Windows OCR 会直接返回 0 行，
        // 框一行文字偏偏是最常见的用法。
        let region = expand_rect(tight, s.w, s.h, OCR_PAD);
        let region_rgba = crop_rgba(&s.rgba, s.w, region);
        let session = ShotSession {
            vs: s.vs,
            sf: s.sf,
            w: sw,
            h: sh,
            rgba: crop,
            data_url: String::new(),
        };
        (
            session,
            crop_png,
            sw,
            sh,
            region_rgba,
            region.2,
            region.3,
            (
                (sx - region.0) as f64,
                (sy - region.1) as f64,
                sw as f64,
                sh as f64,
            ),
        )
    };

    hide_overlay(&app);
    crate::selection::log_line(&format!(
        "screenshot: 选区 {crop_w}x{crop_h}，送识别区域 {region_w}x{region_h}（外扩 {OCR_PAD}px）"
    ));

    // 装了 OCR 插件就优先用插件（插件收到紧框图，行矩形与展示图同一坐标系）；
    // 插件失败不让整条链路挂掉，回落到系统离线 OCR。
    let plugin_result = plugin_ocr(&app, &crop_png);
    if let Err(e) = &plugin_result {
        crate::selection::log_line(&format!(
            "screenshot: OCR 插件失败（{e}），回落到系统 OCR"
        ));
    }

    let mut error: Option<String> = None;
    let (engine, lang, lines) = match plugin_result.ok().flatten() {
        // 插件不回传语言，留空让界面别乱猜
        Some((name, l)) => (format!("plugin:{name}"), String::new(), l),
        None => match ocr_best(&region_rgba, region_w, region_h, "en-US") {
            Ok((all, used, scale)) => {
                let k = scale as f64;
                // 外扩带进来的邻行按矩形摘掉，再把坐标平移回紧框图的坐标系
                let filtered: Vec<OcrLine> = all
                    .into_iter()
                    .filter(|l| line_belongs((l.x / k, l.y / k, l.w / k, l.h / k), sel_in_region))
                    .map(|l| {
                        let lx = (l.x / k - sel_in_region.0).clamp(0.0, crop_w as f64);
                        let ly = (l.y / k - sel_in_region.1).clamp(0.0, crop_h as f64);
                        let lw = (l.w / k).clamp(1.0, (crop_w as f64 - lx).max(1.0));
                        let lh = (l.h / k).clamp(1.0, (crop_h as f64 - ly).max(1.0));
                        OcrLine {
                            text: l.text,
                            x: lx,
                            y: ly,
                            w: lw,
                            h: lh,
                        }
                    })
                    .collect();
                ("windows".to_string(), used, filtered)
            }
            Err(e) => {
                // 识别故障不再直接吞掉：面板照常打开，把原因写给它看
                crate::selection::log_line(&format!("screenshot: OCR 失败 {e}"));
                error = Some(e);
                ("windows".to_string(), String::new(), Vec::new())
            }
        },
    };
    let text = lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n");
    crate::selection::log_line(&format!("screenshot: OCR 完成 {} 行 / {} 字符", lines.len(), text.chars().count()));
    if text.trim().is_empty() && error.is_none() {
        // 没认出字是一件需要事后能查的事：把送进引擎的图留下
        dump_empty_crop(&app, &crop_png);
    }

    // 选区图存成 data URL：结果面板的「原图覆盖」要把它铺回去。
    // 行坐标与它在同一个像素空间，前端按百分比定位就够，不用碰 DPI 换算。
    use base64::Engine as _;
    let result = OcrResult {
        crop_url: format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&crop_png)
        ),
        crop_w,
        crop_h,
        text: text.clone(),
        lines: lines.clone(),
        engine,
        lang,
        error,
    };

    // 裁剪会话留给「重新识别」，结果留给结果面板
    if let Ok(mut g) = state.lock() {
        *g = Some(ShotSession { data_url: String::new(), ..session });
    }
    if let Ok(mut g) = ocr_state.0.lock() {
        *g = Some(result.clone());
    }

    // 结果面板就近弹出。识别为空也照样打开：面板里那个「未识别到文字」
    // 的状态比什么都不弹更有用，用户可以直接重新框选。
    match show_ocr_panel(&app) {
        Ok(()) => {
            // 不 sleep 等前端挂载：面板自己会在挂载时用 ocr_last 兜一次，
            // 事件比页面先到也不会丢内容，这里 sleep 只会白白卡住调用线程。
            let _ = app.emit("ocr-set-source", &result);
        }
        Err(e) => {
            // 面板开不出来就退回弹窗，至少别让用户白框一次
            crate::selection::log_line(&format!("screenshot: 结果面板打开失败（{e}），退回弹窗"));
            if !text.trim().is_empty() {
                let _ = crate::selection::ensure_popup_at_cursor(&app);
                let _ = app.emit(
                    "popup-set-source",
                    serde_json::json!({ "text": text, "autoTranslate": true, "kind": "screenshot" }),
                );
            }
        }
    }
    Ok(OcrPayload { text, lines })
}

// ==================== 屏幕捕获（GDI） ====================

/// 用 OCR 插件识别选区。没有可用插件时返回 Ok(None)，由调用方回落到系统 OCR。
/// 返回 (插件名, 行)。插件名要给结果面板显示识别引擎用。
fn plugin_ocr(app: &AppHandle, png: &[u8]) -> Result<Option<(String, Vec<OcrLine>)>, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("获取配置目录失败: {e}"))?;
    let Some(p) = crate::plugin::first_enabled(&dir, crate::plugin::PluginKind::Ocr) else {
        return Ok(None);
    };
    let main = p.main.clone().ok_or("OCR 插件缺少入口脚本")?;

    use base64::Engine as _;
    let args = serde_json::json!({
        "pngBase64": base64::engine::general_purpose::STANDARD.encode(png)
    })
    .to_string();
    let permissions = p.permissions.clone();
    let raw = tauri::async_runtime::block_on(tauri::async_runtime::spawn_blocking(move || {
        crate::plugin_js::call(std::path::Path::new(&main), "ocr", &args, &permissions)
    }))
    .map_err(|e| format!("OCR 插件调用失败: {e}"))??;

    let text = crate::plugin_js::text_of(&raw)?;
    crate::selection::log_line(&format!(
        "screenshot: OCR 插件「{}」返回 {} 字符",
        p.name,
        text.chars().count()
    ));

    // 插件只给文字、没有行级坐标。这里如实留零，
    // 原图覆盖模式据此改成整块铺在选区上，而不是假装知道每行在哪
    Ok(Some((
        p.name.clone(),
        text.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| OcrLine {
                text: l.to_string(),
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            })
            .collect(),
    )))
}

#[cfg(windows)]
pub fn capture_rect(vx: i32, vy: i32, vw: i32, vh: i32) -> Result<Vec<u8>, String> {
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
        SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
    };

    if vw <= 0 || vh <= 0 {
        return Err(format!("抓取区域尺寸无效: {vw}x{vh}"));
    }

    unsafe {
        let hdc_screen = GetDC(None);
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = vw;
        bmi.bmiHeader.biHeight = -vh; // top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbmp = match CreateDIBSection(Some(hdc_screen), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(h) => h,
            Err(e) => {
                let _ = ReleaseDC(None, hdc_screen);
                return Err(format!("CreateDIBSection 失败: {e}"));
            }
        };
        let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
        if hdc_mem.is_invalid() {
            let _ = DeleteObject(hbmp.into());
            let _ = ReleaseDC(None, hdc_screen);
            return Err("CreateCompatibleDC 失败".into());
        }
        let old = SelectObject(hdc_mem, hbmp.into());
        let blt = BitBlt(hdc_mem, 0, 0, vw, vh, Some(hdc_screen), vx, vy, SRCCOPY);

        let mut rgba = Vec::with_capacity((vw * vh * 4) as usize);
        if blt.is_ok() && !bits.is_null() {
            let slice = std::slice::from_raw_parts(bits as *const [u8; 4], (vw * vh) as usize);
            for px in slice {
                rgba.push(px[2]); // B
                rgba.push(px[1]); // G
                rgba.push(px[0]); // R
                rgba.push(255); // A
            }
        }
        let _ = SelectObject(hdc_mem, old);
        let _ = DeleteObject(hbmp.into());
        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(None, hdc_screen);

        blt.map_err(|e| format!("BitBlt 失败: {e}"))?;
        Ok(rgba)
    }
}

#[cfg(not(windows))]
pub fn capture_rect(_vx: i32, _vy: i32, _vw: i32, _vh: i32) -> Result<Vec<u8>, String> {
    Err("当前平台暂不支持屏幕捕获".into())
}

// ==================== 图像编码 ====================

pub fn rgba_to_jpeg_dataurl(rgba: &[u8], w: i32, h: i32) -> Result<String, String> {
    let b64 = rgba_to_jpeg_base64(rgba, w, h)?;
    Ok(format!("data:image/jpeg;base64,{b64}"))
}

pub fn rgba_to_jpeg_base64(rgba: &[u8], w: i32, h: i32) -> Result<String, String> {
    let img = image::RgbaImage::from_raw(w as u32, h as u32, rgba.to_vec())
        .ok_or("图像数据尺寸不匹配")?;
    // JPEG 不支持 Alpha 通道，先转 RGB8
    let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
    let mut cursor = std::io::Cursor::new(Vec::new());
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 88);
    image::DynamicImage::ImageRgb8(rgb)
        .write_with_encoder(encoder)
        .map_err(|e| format!("JPEG 编码失败: {e}"))?;
    use base64::Engine as _;
    Ok(base64::engine::general_purpose::STANDARD.encode(cursor.into_inner()))
}

pub fn rgba_to_png_bytes(rgba: &[u8], w: i32, h: i32) -> Result<Vec<u8>, String> {
    let img = image::RgbaImage::from_raw(w as u32, h as u32, rgba.to_vec())
        .ok_or("图像数据尺寸不匹配")?;
    let mut cursor = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|e| format!("PNG 编码失败: {e}"))?;
    Ok(cursor.into_inner())
}

// ==================== OCR（Windows.Media.OCR） ====================

#[derive(Debug, Clone)]
struct RawOcrLine {
    text: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[cfg(windows)]
pub fn ocr_bytes_lang(img: &[u8], lang: &str) -> Result<(Vec<OcrLine>, String), String> {
    // WinRT 的 OCR 必须在 MTA 线程上跑：Tauri 的命令线程可能已被 WebView 初始化成 STA，
    // 在那里调用 CoInitializeEx 会返回 RPC_E_CHANGED_MODE；而在 STA 上直接 await 异步结果
    // （下面的 .get()）又会因缺少消息泵而死锁。所以固定换到独立线程执行。
    let img = img.to_vec();
    let lang = lang.to_string();
    std::thread::spawn(move || ocr_bytes_inner(&img, &lang))
        .join()
        .map_err(|_| "OCR 线程异常退出".to_string())?
}

/// 只要识别出的行（调用方不关心实际用了哪个语言包时用这个）
#[cfg(windows)]
pub fn ocr_bytes(img: &[u8], lang: &str) -> Result<Vec<OcrLine>, String> {
    ocr_bytes_lang(img, lang).map(|(lines, _)| lines)
}

#[cfg(windows)]
fn ocr_bytes_inner(img: &[u8], lang: &str) -> Result<(Vec<OcrLine>, String), String> {
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
    use windows::core::HSTRING;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    unsafe {
        // 0x80010106 RPC_E_CHANGED_MODE：线程已有 COM 环境，但单元模型不同。
        // 这种情况可以继续，Windows.Media.OCR 在两种单元模型下都可用。
        const RPC_E_CHANGED_MODE: i32 = 0x8001_0106u32 as i32;
        match CoInitializeEx(None, COINIT_MULTITHREADED).ok() {
            Ok(()) => {}
            Err(e) if e.code().0 == RPC_E_CHANGED_MODE => {}
            Err(e) => {
                return Err(format!("COM 初始化失败 (0x{:08X}): {e}", e.code().0 as u32));
            }
        }
    }

    let stream = InMemoryRandomAccessStream::new().map_err(|e| format!("创建内存流失败: {e}"))?;
    // 必须把写入器绑定到内存流：无参 DataWriter::new() 没有输出目标，
    // StoreAsync 会直接返回 ERROR_INVALID_OPERATION（0x800710DD）。
    let writer =
        DataWriter::CreateDataWriter(&stream).map_err(|e| format!("创建写入器失败: {e}"))?;
    writer
        .WriteBytes(img)
        .map_err(|e| format!("写入图像字节失败: {e}"))?;
    writer
        .StoreAsync()
        .map_err(|e| format!("StoreAsync 调用失败: {e}"))?
        .get()
        .map_err(|e| format!("StoreAsync 等待失败: {e}"))?;
    writer
        .FlushAsync()
        .map_err(|e| format!("FlushAsync 调用失败: {e}"))?
        .get()
        .map_err(|e| format!("FlushAsync 等待失败: {e}"))?;
    writer
        .DetachStream()
        .map_err(|e| format!("DetachStream 失败: {e}"))?;

    // DataWriter 写完会把内存流的游标留在末尾，解码器必须从起点读
    stream
        .Seek(0)
        .map_err(|e| format!("重置流位置失败: {e}"))?;

    let decoder = BitmapDecoder::CreateAsync(&stream)
        .map_err(|e| format!("创建解码器失败: {e}"))?
        .get()
        .map_err(|e| format!("解码器初始化失败: {e}"))?;
    let bitmap = decoder
        .GetSoftwareBitmapAsync()
        .map_err(|e| format!("读取位图失败: {e}"))?
        .get()
        .map_err(|e| format!("读取位图等待失败: {e}"))?;

    let engine = OcrEngine::TryCreateFromLanguage(
        &Language::CreateLanguage(&HSTRING::from(lang)).map_err(|e| e.to_string())?,
    )
    .ok()
    .or_else(|| OcrEngine::TryCreateFromUserProfileLanguages().ok())
    .ok_or("系统未安装可用的 OCR 语言包（设置 → 时间和语言 → 语言）")?;
    // 实际用上的语言包要报给界面：装了哪些语言包决定了能认出哪种文字，
    // 报错时说清楚这一点，用户才知道该去装什么
    let used_lang = engine
        .RecognizerLanguage()
        .ok()
        .and_then(|l| l.LanguageTag().ok())
        .map(|t| t.to_string())
        .unwrap_or_default();

    let result = engine
        .RecognizeAsync(&bitmap)
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;

    let view = result.Lines().map_err(|e| e.to_string())?;
    let mut raw: Vec<RawOcrLine> = Vec::new();
    for i in 0..view.Size().map_err(|e| e.to_string())? {
        let line = view.GetAt(i).map_err(|e| e.to_string())?;
        let text = line.Text().map_err(|e| e.to_string())?.to_string();
        // Windows API 的 OcrLine 没有矩形，用其词级矩形合并出行包围盒
        let words = line.Words().map_err(|e| e.to_string())?;
        let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for widx in 0..words.Size().map_err(|e| e.to_string())? {
            let word = words.GetAt(widx).map_err(|e| e.to_string())?;
            let r = word.BoundingRect().map_err(|e| e.to_string())?;
            minx = minx.min(r.X);
            miny = miny.min(r.Y);
            maxx = maxx.max(r.X + r.Width);
            maxy = maxy.max(r.Y + r.Height);
        }
        if minx == f32::MAX {
            continue;
        }
        raw.push(RawOcrLine {
            text,
            x: minx as f64,
            y: miny as f64,
            w: (maxx - minx) as f64,
            h: (maxy - miny) as f64,
        });
    }
    // 按阅读顺序排序（先按 y 分组容差，再按 x）
    raw.sort_by(|a, b| {
        let band = (a.y - b.y).abs() < 14.0;
        if band { a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal) } else { a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal) }
    });
    Ok((
        raw.into_iter()
            .map(|l| OcrLine { text: l.text, x: l.x, y: l.y, w: l.w, h: l.h })
            .collect(),
        used_lang,
    ))
}

#[cfg(not(windows))]
pub fn ocr_bytes(_img: &[u8], _lang: &str) -> Result<Vec<OcrLine>, String> {
    Err("当前平台暂不支持 OCR".into())
}

#[cfg(not(windows))]
pub fn ocr_bytes_lang(_img: &[u8], _lang: &str) -> Result<(Vec<OcrLine>, String), String> {
    Err("当前平台暂不支持 OCR".into())
}

#[cfg(all(test, windows))]
mod tests {
    /// 并集矩形：副屏在主屏左侧时坐标是负的，别被 min/max 写反
    #[test]
    fn union_rect_handles_negative_origin() {
        // 主屏 1920x1080 在 (0,0)，副屏 1920x1080 挂在它左边
        let u = super::union_rect(&[(0, 0, 1920, 1080), (-1920, 0, 1920, 1080)]).unwrap();
        assert_eq!(u, (-1920, 0, 3840, 1080));
    }

    /// 上下错位与中间空隙：并集要把两块屏都框进来
    #[test]
    fn union_rect_covers_offsets_and_gaps() {
        // 副屏在右上，底部比主屏高
        let u = super::union_rect(&[(0, 0, 1920, 1080), (1920, -704, 1920, 1080)]).unwrap();
        assert_eq!(u, (0, -704, 3840, 1784));
    }

    /// 单屏时并集就是它自己
    #[test]
    fn union_rect_single_monitor() {
        assert_eq!(
            super::union_rect(&[(100, 200, 1600, 900)]).unwrap(),
            (100, 200, 1600, 900)
        );
        assert!(super::union_rect(&[]).is_none());
    }

    /// 送识别前要向外扩一圈，且不能扩出帧外
    #[test]
    fn expand_rect_pads_and_clamps() {
        assert_eq!(
            super::expand_rect((100, 100, 200, 20), 1000, 800, 32),
            (68, 68, 264, 84)
        );
        // 左上角起框：只能往右下扩
        assert_eq!(
            super::expand_rect((0, 0, 200, 20), 1000, 800, 32),
            (0, 0, 232, 52)
        );
        // 右下角起框：不能超出帧
        assert_eq!(
            super::expand_rect((900, 780, 100, 20), 1000, 800, 32),
            (868, 748, 132, 52)
        );
    }

    /// 外扩带进来的邻行必须被过滤掉，用户框到的行必须留下
    #[test]
    fn line_belongs_keeps_only_selected_lines() {
        // 区域坐标系里的选区：宽 400、高 24，紧框一行
        let sel = (32.0, 32.0, 400.0, 24.0);
        assert!(super::line_belongs((36.0, 34.0, 300.0, 19.0), sel), "框内的行要留");
        assert!(!super::line_belongs((36.0, 2.0, 300.0, 19.0), sel), "上一行要滤掉");
        assert!(!super::line_belongs((36.0, 60.0, 300.0, 19.0), sel), "下一行要滤掉");
        // 大部分在选区外、中心也在外面的行同样滤掉
        let s2 = (0.0, 0.0, 100.0, 100.0);
        assert!(!super::line_belongs((-70.0, 40.0, 80.0, 20.0), s2));
        // 压在左边界上、有近一半落在选区里的行要留（用户确实框到了它）
        assert!(super::line_belongs((-40.0, 40.0, 80.0, 20.0), s2));
    }

    /// 结果面板按这套字段名取值，序列化键改了就前端就断了，在这里钉住
    #[test]
    fn ocr_result_serializes_to_the_keys_the_panel_reads() {
        let r = super::OcrResult {
            crop_url: "data:image/png;base64,AAAA".into(),
            crop_w: 320,
            crop_h: 200,
            text: "Hello".into(),
            lines: vec![super::OcrLine {
                text: "Hello".into(),
                x: 1.0,
                y: 2.0,
                w: 3.0,
                h: 4.0,
            }],
            engine: "windows".into(),
            lang: "en-US".into(),
            error: None,
        };
        let v: serde_json::Value = serde_json::to_value(&r).expect("OcrResult 应能序列化");
        for key in ["cropUrl", "cropW", "cropH", "text", "lines", "engine", "lang", "error"] {
            assert!(v.get(key).is_some(), "缺字段 {key}");
        }
        let line = &v["lines"][0];
        for key in ["text", "x", "y", "w", "h"] {
            assert!(line.get(key).is_some(), "行缺字段 {key}");
        }
    }

    /// OCR 固定样本测试，覆盖 COM 单元选择、内存流写入、位图解码与识别引擎四段。
    /// 样本是白底黑字的 "Hello World" 与一行中文；系统装有任一中英文 OCR 语言包即可。
    #[test]
    fn ocr_reads_fixture_sample() {
        let png = include_bytes!("../tests/fixtures/ocr-sample.png");
        // 精简的 CI 镜像常常没装 OCR 语言包，这种情况跳过而不是判失败
        let lines = match super::ocr_bytes(png, "en-US") {
            Ok(l) => l,
            Err(e) if e.contains("语言包") => {
                eprintln!("跳过：当前环境没有可用的 OCR 语言包（{e}）");
                return;
            }
            Err(e) => panic!("OCR 调用失败: {e}"),
        };
        let text = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        // OCR 对小写 l 与大写 I、数字 1 常有混淆，断言前先归一化，只验证"认得出内容"。
        // 系统没装 en-US 语言包时会自动回退到用户语言（本机为 zh-Hans-CN），中英文都能认。
        let normalized = text.to_lowercase().replace('i', "l").replace('1', "l");
        assert!(
            normalized.contains("hello") && normalized.contains("world"),
            "未识别出预期的英文文本，实际输出: {text:?}"
        );
        // 中文那行能不能认出来取决于系统装没装中文语言包（CI 镜像通常只有英文包），
        // 所以只做提示，不作为断言。
        if !(text.contains("随") || text.contains("译")) {
            println!("提示：当前环境的 OCR 语言包认不出样例里的中文，实际输出: {text:?}");
        }
    }
}
