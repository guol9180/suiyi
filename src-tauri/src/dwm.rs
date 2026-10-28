//! DWM 细节修饰：去掉 Windows 给毛玻璃窗口自动描的那一圈系统边框。
//!
//! Windows 11 会给带 Acrylic 效果的窗口画 1-2px 的系统边框线，大窗上还好，
//! 在轻提示这种 66px 高的小条上就是一圈很扎眼的「丑边框」。把它设成
//! DWMWA_COLOR_NONE 即可；Windows 10 不认识这个属性，调用失败直接忽略
//! （那边本来也没有这圈线）。

#[cfg(windows)]
pub fn clear_border(win: &tauri::WebviewWindow) {
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR};

    // tauri 依赖里的 windows 0.62 与直连的 0.61 是两份类型：句柄裸指针重新包一层
    let Some(hwnd) = win.hwnd().ok().map(|h| windows::Win32::Foundation::HWND(h.0)) else {
        return;
    };
    // DWMWA_COLOR_NONE：完全去掉边框线（区别于「画成透明色」）
    let none: u32 = 0xFF_FF_FF_FE;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &none as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

#[cfg(not(windows))]
pub fn clear_border(_win: &tauri::WebviewWindow) {}
