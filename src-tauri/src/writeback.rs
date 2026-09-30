//! M3 输入框转译：Alt+T 把当前焦点输入框的内容翻译后原位写回
//!
//! 流程（剪贴板往返 + 焦点快照校验）：
//! 1. 记录前台窗口句柄与剪贴板；
//! 2. Ctrl+A 全选 → Ctrl+C 抓取输入框内容；
//! 3. 翻译（异步网络调用）；
//! 4. 前台句柄未变 → Ctrl+A + Ctrl+V 写回译文；恢复原剪贴板；
//! 5. 焦点已变 → 放弃写回，译文放入剪贴板兜底。
//!
//! 局限：浏览器页面焦点不在输入框时 Ctrl+A 会选中整个页面，
//! 此时抓取内容会被丢弃（页文本过长/含换行过多的保护判断在调用方）。

use arboard::Clipboard;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::selection::{log_line, simulate_ctrl_a, simulate_ctrl_c, simulate_ctrl_v};

#[cfg(windows)]
fn foreground_hwnd() -> isize {
    unsafe { GetForegroundWindow().0 as isize }
}

#[cfg(windows)]
pub fn trigger_input_translate(app: AppHandle) {
    log_line("input: Alt+T 触发");
    std::thread::spawn(move || {
        if let Err(e) = run_input_translate(&app) {
            log_line(&format!("input: 失败 {e}"));
        }
    });
}

#[cfg(not(windows))]
pub fn trigger_input_translate(_app: AppHandle) {
    log_line("input: 当前平台不支持");
}

#[cfg(windows)]
fn run_input_translate(app: &AppHandle) -> Result<(), String> {
    let fg_before = foreground_hwnd();

    let mut cb = Clipboard::new().map_err(|e| format!("剪贴板打开失败: {e}"))?;
    let prev = cb.get_text().ok();

    // 1) 抓取输入框内容
    simulate_ctrl_a().map_err(|e| format!("模拟 Ctrl+A 失败: {e}"))?;
    std::thread::sleep(Duration::from_millis(200));
    simulate_ctrl_c().map_err(|e| format!("模拟 Ctrl+C 失败: {e}"))?;
    std::thread::sleep(Duration::from_millis(350));
    let original = match cb.get_text().ok().filter(|t| !t.trim().is_empty()) {
        Some(t) => t,
        None => {
            if let Some(p) = &prev {
                let _ = cb.set_text(p.clone());
            }
            log_line("input: 未取到输入框内容（焦点可能不在可编辑区域），流程结束");
            return Ok(());
        }
    };
    // 保护判断：抓到异常巨大的内容多半是 Ctrl+A 选中了整个页面，放弃写回
    if original.chars().count() > 5000 {
        if let Some(p) = &prev {
            let _ = cb.set_text(p.clone());
        }
        log_line(&format!(
            "input: 抓取内容异常巨大（{} 字符），疑似选中了整个页面，放弃",
            original.chars().count()
        ));
        return Ok(());
    }
    log_line(&format!("input: 抓取输入框 {} 字符", original.chars().count()));

    // 2) 翻译（取第一个启用中的翻译服务）
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("获取配置目录失败: {e}"))?;
    let file = crate::config::load_services(&dir)?;
    let svc = file
        .services
        .iter()
        .find(|s| s.enabled && s.kind == crate::config::ServiceKind::Translation)
        .ok_or("没有已启用的翻译服务")?;
    let key = crate::keyring::get_api_key(&svc.id)?
        .ok_or("该服务尚未设置 API Key")?;
    let prompt = crate::translator::build_prompt(svc.prompt_template.as_deref(), "自动检测", "English", &original);
    let (translated, ms) = tauri::block_on(crate::translator::translate(
        app,
        crate::translator::TranslateParams {
            service_id: &svc.id,
            base_url: &svc.base_url,
            api_key: &key,
            model: &svc.model,
            prompt: &prompt,
            temperature: svc.temperature,
            stream: false, // 写回场景用非流式，简单可靠
            timeout: Duration::from_secs(file.timeout_secs.max(10)),
        },
    ))?;
    log_line(&format!(
        "input: 翻译完成 {} 字符（{ms}ms），开始写回",
        translated.chars().count()
    ));

    // 3) 焦点快照校验：焦点变了绝不写回
    if foreground_hwnd() != fg_before {
        let _ = cb.set_text(translated);
        log_line("input: 焦点已切换，放弃写回，译文已放入剪贴板兜底");
        return Ok(());
    }

    // 4) 写回：译文上剪贴板 → Ctrl+A 全选 → Ctrl+V 粘贴 → 恢复原剪贴板
    cb.set_text(translated)
        .map_err(|e| format!("设置剪贴板失败: {e}"))?;
    simulate_ctrl_a().map_err(|e| format!("模拟 Ctrl+A 失败: {e}"))?;
    std::thread::sleep(Duration::from_millis(150));
    simulate_ctrl_v().map_err(|e| format!("模拟 Ctrl+V 失败: {e}"))?;
    std::thread::sleep(Duration::from_millis(300));
    if let Some(p) = &prev {
        let _ = cb.set_text(p.clone());
    }
    log_line("input: 写回完成 ✓");
    Ok(())
}
