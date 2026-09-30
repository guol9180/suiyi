//! M2 截图识别：Alt+S 冻结屏幕 → 覆盖层框选 → 裁剪 → Windows.Media.OCR → 弹窗翻译
//!
//! 流程：
//! 1. 热键触发：定位鼠标所在的那块显示器，用 GDI 抓它的画面（冻结帧），JPEG 编码存入会话；
//! 2. 在该显示器上铺一层无边框置顶覆盖层，前端加载冻结帧供框选；
//! 3. 松开鼠标：前端回传逻辑坐标 → Rust 换算物理坐标裁剪原图 → OCR；
//! 4. 隐藏覆盖层，OCR 文本投递给划词弹窗自动翻译。
//!
//! 多屏策略：只截鼠标当前所在的显示器。跨屏整抓在混合 DPI 下坐标换算容易错位，
//! 而且用户框选时本来也只关心眼前这一块屏。

use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

/// 进行中的截图会话（同一时刻最多一个）
pub struct ShotSession {
    /// 抓取区域（物理像素）：x, y, w, h，等于鼠标所在显示器的矩形
    pub vs: (i32, i32, i32, i32),
    /// 该显示器的缩放系数
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

/// 抓取鼠标所在显示器的画面，组装一次截图会话
fn build_session(app: &AppHandle) -> Result<ShotSession, String> {
    let cursor = app
        .cursor_position()
        .map_err(|e| format!("获取鼠标位置失败: {e}"))?;
    let monitor = app
        .monitor_from_point(cursor.x, cursor.y)
        .map_err(|e| format!("匹配显示器失败: {e}"))?
        .ok_or("找不到鼠标所在的显示器")?;

    let pos = monitor.position();
    let size = monitor.size();
    let sf = monitor.scale_factor();
    let (vx, vy) = (pos.x, pos.y);
    let (vw, vh) = (size.width as i32, size.height as i32);
    if vw <= 0 || vh <= 0 {
        return Err(format!("显示器尺寸无效: {vw}x{vh}"));
    }

    let rgba = capture_rect(vx, vy, vw, vh)?;
    let data_url = rgba_to_jpeg_dataurl(&rgba, vw, vh)?;
    crate::selection::log_line(&format!(
        "screenshot: 抓取显示器 ({vx},{vy}) {vw}x{vh} sf={sf}"
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
    let (vsx, vsy, vsw, vsh, sf) = {
        let state = app.state::<Mutex<Option<ShotSession>>>();
        let g = state.lock().map_err(|e| e.to_string())?;
        let s = g.as_ref().ok_or("会话不存在")?;
        (s.vs.0, s.vs.1, s.vs.2, s.vs.3, s.sf)
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

    // 装了 OCR 插件就优先用插件；插件失败不让整条链路挂掉，回落到系统离线 OCR
    let plugin_result = plugin_ocr(&app, &png);
    if let Err(e) = &plugin_result {
        crate::selection::log_line(&format!(
            "screenshot: OCR 插件失败（{e}），回落到系统 OCR"
        ));
    }
    let lines = match plugin_result.ok().flatten() {
        Some(l) => l,
        // 系统 OCR：英文优先，取不到语言包时回退用户语言
        None => match ocr_bytes(&png, "en-US") {
            Ok(l) => l,
            Err(e) => {
                crate::selection::log_line(&format!("screenshot: OCR 失败 {e}"));
                return Err(format!("OCR 识别失败: {e}"));
            }
        },
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
        serde_json::json!({ "text": text, "autoTranslate": true, "kind": "screenshot" }),
    );
    Ok(OcrPayload { text, lines })
}

// ==================== 屏幕捕获（GDI） ====================

/// 用 OCR 插件识别选区。没有可用插件时返回 Ok(None)，由调用方回落到系统 OCR。
fn plugin_ocr(app: &AppHandle, png: &[u8]) -> Result<Option<Vec<OcrLine>>, String> {
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

    // 插件只给文字、没有行级坐标；坐标留给「原图覆盖」的后续增强
    Ok(Some(
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
    ))
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
pub fn ocr_bytes(img: &[u8], lang: &str) -> Result<Vec<OcrLine>, String> {
    // WinRT 的 OCR 必须在 MTA 线程上跑：Tauri 的命令线程可能已被 WebView 初始化成 STA，
    // 在那里调用 CoInitializeEx 会返回 RPC_E_CHANGED_MODE；而在 STA 上直接 await 异步结果
    // （下面的 .get()）又会因缺少消息泵而死锁。所以固定换到独立线程执行。
    let img = img.to_vec();
    let lang = lang.to_string();
    std::thread::spawn(move || ocr_bytes_inner(&img, &lang))
        .join()
        .map_err(|_| "OCR 线程异常退出".to_string())?
}

#[cfg(windows)]
fn ocr_bytes_inner(img: &[u8], lang: &str) -> Result<Vec<OcrLine>, String> {
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

#[cfg(all(test, windows))]
mod tests {
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
