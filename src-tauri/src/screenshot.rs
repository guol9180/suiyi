//! M2 截图识别：Alt+S 冻结屏幕 → 覆盖层框选 → 裁剪 → Windows.Media.OCR → 弹窗翻译
//!
//! 流程：
//! 1. 热键触发：GDI 抓取整个虚拟屏幕（冻结帧），JPEG 编码存入会话；
//! 2. 创建全屏覆盖层窗口（无边框、置顶），前端加载冻结帧供框选；
//! 3. 松开鼠标：前端回传逻辑坐标 → Rust 换算物理坐标裁剪原图 → OCR；
//! 4. 隐藏覆盖层，OCR 文本投递给划词弹窗自动翻译。

use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

/// 进行中的截图会话（同一时刻最多一个）
pub struct ShotSession {
    /// 虚拟屏幕原点与尺寸（物理像素）：x, y, w, h
    pub vs: (i32, i32, i32, i32),
    /// 主显示器缩放系数
    pub sf: f64,
    pub w: i32,
    pub h: i32,
    pub rgba: Vec<u8>,
    /// 冻结帧 JPEG 的 data URL（前端展示用）
    pub data_url: String,
}

pub static SHOT: Mutex<Option<ShotSession>> = Mutex::new(None);

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
    trigger_screenshot(app);
    Ok(())
}

fn run_screenshot(app: AppHandle) {
    let build = || -> Result<ShotSession, String> {
            let mut s = capture_virtual_screen()?;
            s.sf = app
                .primary_monitor()
                .ok()
                .flatten()
                .map(|m| m.scale_factor())
                .unwrap_or(1.0);
            s.data_url = rgba_to_jpeg_dataurl(&s.rgba, s.w, s.h)?;
            Ok(s)
        };
        match build() {
            Ok(session) => {
                if let Some(old) = SHOT.lock().ok().and_then(|mut g| g.take()) {
                    let _ = old; // 丢弃旧会话
                }
                if let Ok(mut g) = SHOT.lock() {
                    *g = Some(session);
                }
                // 诊断：冻结帧落盘（定位黑屏问题）
                if let Ok(g) = SHOT.lock() {
                    if let Some(s) = g.as_ref() {
                        if let Some(img) = image::RgbaImage::from_raw(s.w as u32, s.h as u32, s.rgba.clone()) {
                            let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
                            let path = std::env::var("APPDATA")
                                .map(|d| std::path::Path::new(&d).join("com.suiyi.dev").join("last-shot.jpg"))
                                .unwrap_or_default();
                            if let Some(parent) = path.parent() {
                                let _ = std::fs::create_dir_all(parent);
                            }
                            match rgb.save(&path) {
                                Ok(_) => crate::selection::log_line(&format!(
                                    "screenshot: 冻结帧已保存 {}x{} → {}",
                                    s.w, s.h, path.display()
                                )),
                                Err(e) => crate::selection::log_line(&format!(
                                    "screenshot: 冻结帧保存失败 {e}"
                                )),
                            }
                        }
                    }
                }
                if let Err(e) = show_overlay(&app) {
                    crate::selection::log_line(&format!("screenshot: 覆盖层创建失败 {e}"));
                }
            }
            Err(e) => {
                crate::selection::log_line(&format!("screenshot: 抓屏失败 {e}"));
            }
        }
}

/// 创建/复用全屏覆盖层窗口（位于虚拟屏幕原点，尺寸=虚拟屏幕逻辑尺寸）
fn show_overlay(app: &AppHandle) -> Result<(), String> {
    let (vsx, vsy, vsw, vsh, sf) = {
        let g = SHOT.lock().map_err(|e| e.to_string())?;
        let s = g.as_ref().ok_or("会话不存在")?;
        (s.vs.0, s.vs.1, s.vs.2, s.vs.3, s.sf)
    };
    let (lx, ly, lw, lh) = (vsx as f64 / sf, vsy as f64 / sf, vsw as f64 / sf, vsh as f64 / sf);
    match app.get_webview_window("overlay") {
        Some(win) => {
            let _ = win.set_position(tauri::LogicalPosition::new(lx, ly));
            let _ = win.set_size(tauri::LogicalSize::new(lw, lh));
            let _ = win.show();
            let _ = win.set_focus();
            Ok(())
        }
        None => WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("overlay.html".into()))
            .title("随译 · 截图框选")
            .position(lx, ly)
            .inner_size(lw, lh)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .maximizable(false)
            .build()
            .map(|_| ())
            .map_err(|e| format!("创建覆盖层失败: {e}")),
    }
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
pub fn cancel_screenshot(app: AppHandle) -> Result<(), String> {
    hide_overlay(&app);
    Ok(())
}

/// 框选完成：逻辑坐标 → 物理裁剪 → OCR → 投递弹窗
#[tauri::command]
pub fn finish_region(
    app: AppHandle,
    state: tauri::State<'_, Mutex<Option<ShotSession>>>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<OcrPayload, String> {
    let (session, png) = {
        let mut g = state.lock().map_err(|e| e.to_string())?;
        let s = g.as_mut().ok_or("没有进行中的截图会话")?;
        let sf = s.sf;
        // 逻辑 → 物理
        let mut px = (x * sf).round() as i32;
        let mut py = (y * sf).round() as i32;
        let mut pw = (w * sf).round() as i32;
        let mut ph = (h * sf).round() as i32;
        px = px.clamp(0, s.w);
        py = py.clamp(0, s.h);
        pw = pw.clamp(0, s.w - px);
        ph = ph.clamp(0, s.h - py);
        if pw < 4 || ph < 4 {
            return Err("选区太小".into());
        }
        // 裁剪 RGBA（行拷贝）
        let mut crop = Vec::with_capacity((pw * ph * 4) as usize);
        for row in 0..ph {
            let start = ((py + row) * s.w + px) as usize * 4;
            let end = start + pw as usize * 4;
            crop.extend_from_slice(&s.rgba[start..end]);
        }
        let png = rgba_to_png_bytes(&crop, pw, ph)?;
        let session = ShotSession {
            vs: s.vs,
            sf: s.sf,
            w: pw,
            h: ph,
            rgba: crop,
            data_url: String::new(),
        };
        (session, png)
    };

    hide_overlay(&app);

    // OCR（英文优先，失败回退用户语言）
    let lines = match ocr_bytes(&png, "en-US") {
        Ok(l) => l,
        Err(e) => {
            crate::selection::log_line(&format!("screenshot: OCR 失败 {e}"));
            return Err(format!("OCR 识别失败: {e}"));
        }
    };
    let text = lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n");
    crate::selection::log_line(&format!("screenshot: OCR 完成 {} 行 / {} 字符", lines.len(), text.chars().count()));

    // 存一份裁剪会话（供后续「重新识别」等），并投递弹窗
    if let Ok(mut g) = state.lock() {
        *g = Some(ShotSession { data_url: String::new(), ..session });
    }
    if text.trim().is_empty() {
        return Ok(OcrPayload { text: String::new(), lines });
    }

    // 弹窗在光标附近弹出并自动翻译
    let _ = crate::selection::ensure_popup_at_cursor(&app);
    std::thread::sleep(std::time::Duration::from_millis(150));
    let _ = app.emit(
        "popup-set-source",
        serde_json::json!({ "text": text, "autoTranslate": true }),
    );
    Ok(OcrPayload { text, lines })
}

// ==================== 屏幕捕获（GDI） ====================

#[cfg(windows)]
pub fn capture_virtual_screen() -> Result<ShotSession, String> {
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
        SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };

    let (vx, vy, vw, vh) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    if vw <= 0 || vh <= 0 {
        return Err("无法获取虚拟屏幕尺寸".into());
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
        Ok(ShotSession {
            vs: (vx, vy, vw, vh),
            sf: 1.0,
            w: vw,
            h: vh,
            rgba,
            data_url: String::new(),
        })
    }
}

#[cfg(not(windows))]
pub fn capture_virtual_screen() -> Result<ShotSession, String> {
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
pub fn ocr_bytes(img: &[u8], lang: &str) -> Result<Vec<OcrLine>, String> {
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
    use windows::core::HSTRING;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(|e| format!("COM 初始化失败: {e}"))?;
    }

    let stream = InMemoryRandomAccessStream::new().map_err(|e| e.to_string())?;
    let writer = DataWriter::new().map_err(|e| e.to_string())?;
    writer.WriteBytes(img).map_err(|e| e.to_string())?;
    writer
        .StoreAsync()
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    writer
        .FlushAsync()
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    writer.DetachStream().map_err(|e| e.to_string())?;

    let decoder = BitmapDecoder::CreateAsync(&stream)
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    let bitmap = decoder
        .GetSoftwareBitmapAsync()
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;

    let engine = OcrEngine::TryCreateFromLanguage(
        &Language::CreateLanguage(&HSTRING::from(lang)).map_err(|e| e.to_string())?,
    )
    .ok()
    .or_else(|| OcrEngine::TryCreateFromUserProfileLanguages().ok())
    .ok_or("系统未安装可用的 OCR 语言包（设置 → 时间和语言 → 语言）")?;

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
    Ok(raw
        .into_iter()
        .map(|l| OcrLine { text: l.text, x: l.x, y: l.y, w: l.w, h: l.h })
        .collect())
}

#[cfg(not(windows))]
pub fn ocr_bytes(_img: &[u8], _lang: &str) -> Result<Vec<OcrLine>, String> {
    Err("当前平台暂不支持 OCR".into())
}
