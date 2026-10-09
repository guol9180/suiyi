//! M3 输入框转译：Alt+T 把当前焦点输入框的内容翻译后原位写回。
//!
//! 流程（剪贴板往返 + 焦点快照校验）：
//! 1. 记录前台窗口句柄与原剪贴板内容；
//! 2. Ctrl+A 全选 → Ctrl+C 抓取输入框内容；
//! 3. 翻译（写回场景固定非流式，简单可靠）；
//! 4. 前台句柄未变 → Ctrl+A → Ctrl+V 写回译文，并恢复原剪贴板；
//! 5. 句柄已变或抓到异常巨大的内容 → 放弃写回，译文放进剪贴板并弹窗告知。
//!
//! 写回成功后不弹任何窗口：新窗口会抢走输入焦点，反而打断用户。
//! 需要撤销时由目标程序自己的 Ctrl+Z 处理，比我们模拟一遍更可靠。

use arboard::Clipboard;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::selection::{
    log_line, read_clipboard_text, simulate_arrow_right, simulate_ctrl_a, simulate_ctrl_c,
    simulate_ctrl_v, wait_for_modifiers_released,
};

/// 抓到的内容超过这个长度，就认为 Ctrl+A 选中的是整个页面而不是输入框
const MAX_INPUT_CHARS: usize = 5000;

#[cfg(windows)]
fn foreground_hwnd() -> isize {
    unsafe { GetForegroundWindow().0 as isize }
}

/// 判断抓到的内容是否像「整个页面」而不是一个输入框
fn looks_like_whole_page(text: &str) -> bool {
    text.chars().count() > MAX_INPUT_CHARS
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

/// 放弃写回：把译文放进剪贴板，并在光标旁轻声告知用户。
///
/// 走轻提示而不是划词弹窗：弹窗会抢走输入框的焦点，正是我们不想打断的地方。
#[cfg(windows)]
fn degrade_to_clipboard(
    app: &AppHandle,
    cb: &mut Clipboard,
    translated: &str,
    reason: &str,
) {
    if cb.set_text(translated.to_string()).is_err() {
        log_line("input: 降级失败，连剪贴板都没写进去");
        return;
    }
    crate::notice::show(app, &format!("{reason}，译文已复制到剪贴板"), "info");
    log_line(&format!("input: 已降级为复制 + 轻提示（{reason}）"));
}

/// 提示里放不下长报错：截到 60 个字符，剩下的留给日志
fn clip(text: &str, max: usize) -> String {
    let t = text.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    format!("{}…", t.chars().take(max).collect::<String>())
}

#[cfg(windows)]
fn run_input_translate(app: &AppHandle) -> Result<(), String> {
    let fg_before = foreground_hwnd();

    // 热键按下的那一刻修饰键还按着（实测距触发只有 2ms），这时候发 Ctrl+A / Ctrl+C
    // 目标程序收到的是 Ctrl+Alt+A / Ctrl+Alt+C：全选和复制都没发生。
    let waited = wait_for_modifiers_released(1200);
    if waited > 0 {
        log_line(&format!("input: 等修饰键松开 {waited}ms"));
    }

    let mut cb = Clipboard::new().map_err(|e| format!("剪贴板打开失败: {e}"))?;
    let prev = cb.get_text().ok();

    // 1) 抓取输入框内容。先清空剪贴板：否则目标程序不响应模拟复制时，我们会把
    //    上一次残留的剪贴板内容当成"输入框内容"翻译并写回 —— 用户看到的就是
    //    "没弹窗、没翻译，直接就覆盖了"。
    if cb.clear().is_err() {
        log_line("input: 清空剪贴板失败，放弃");
        crate::notice::show(app, "没取到输入框内容：剪贴板不可用", "err");
        return Ok(());
    }
    simulate_ctrl_a().map_err(|e| format!("模拟 Ctrl+A 失败: {e}"))?;
    std::thread::sleep(Duration::from_millis(180));
    simulate_ctrl_c().map_err(|e| format!("模拟 Ctrl+C 失败: {e}"))?;

    let original = match read_clipboard_text(&mut cb, 1500) {
        Some(t) => t,
        None => {
            if let Some(p) = &prev {
                let _ = cb.set_text(p.clone());
            }
            // Ctrl+A 已经把输入框内容选中了，不收回选区的话用户下次敲字就覆盖全文
            let _ = simulate_arrow_right();
            log_line("input: 未取到输入框内容（焦点可能不在可编辑区域），流程结束");
            crate::notice::show(app, "没取到输入框内容：把光标放进输入框再按一次", "err");
            return Ok(());
        }
    };

    if looks_like_whole_page(&original) {
        if let Some(p) = &prev {
            let _ = cb.set_text(p.clone());
        }
        let _ = simulate_arrow_right();
        log_line(&format!(
            "input: 抓取内容异常巨大（{} 字符），疑似选中了整个页面，放弃",
            original.chars().count()
        ));
        crate::notice::show(app, "这段内容太长了，像是整页文本，先选中要翻译的部分", "err");
        return Ok(());
    }
    log_line(&format!("input: 抓取输入框 {} 字符", original.chars().count()));

    // 2) 翻译：取第一个启用中的翻译服务
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
    // 本地服务（Ollama 这类）不需要密钥：只有 requires_key 为真时才强制要求
    // get_api_key 返回 Result<Option<String>>：读凭据失败与"没存过"在这里都按空串处理
    let key = crate::keyring::get_api_key(&svc.id)
        .ok()
        .flatten()
        .unwrap_or_default();
    if key.trim().is_empty() && svc.requires_key {
        let msg = format!("「{}」还没填 API Key，先去设置页补一个", svc.name);
        if let Some(p) = &prev {
            let _ = cb.set_text(p.clone());
        }
        let _ = simulate_arrow_right();
        crate::notice::show(app, &msg, "err");
        log_line(&format!("input: {msg}"));
        return Ok(());
    }

    let target_lang = if file.input_target_lang.trim().is_empty() {
        crate::config::DEFAULT_INPUT_TARGET_LANG
    } else {
        file.input_target_lang.as_str()
    };
    let prompt = crate::translator::build_prompt(
        svc.prompt_template.as_deref(),
        "自动检测",
        target_lang,
        &original,
    );
    let translated = tauri::async_runtime::block_on(crate::translator::translate(
        app,
        crate::translator::TranslateParams {
            service_id: &svc.id,
            base_url: &svc.base_url,
            api_key: &key,
            model: &svc.model,
            prompt: &prompt,
            temperature: svc.temperature,
            stream: false,
            timeout: Duration::from_secs(file.timeout_secs.max(10)),
        },
    ));
    let (translated, ms) = match translated {
        Ok(v) => v,
        Err(e) => {
            // 失败也要留痕，历史页里才说得清哪一次没成
            let _ = crate::history::record(
                &dir,
                crate::history::NewEntry {
                    kind: "input".into(),
                    source: original.clone(),
                    translated: String::new(),
                    service_name: svc.name.clone(),
                    elapsed_ms: 0,
                    ok: false,
                    error: Some(e.clone()),
                },
            );
            if let Some(p) = &prev {
                let _ = cb.set_text(p.clone());
            }
            let _ = simulate_arrow_right();
            log_line(&format!("input: 翻译失败 {e}"));
            crate::notice::show(app, &format!("翻译失败：{}", clip(&e, 60)), "err");
            return Ok(());
        }
    };
    log_line(&format!(
        "input: 翻译完成 {} 字符（{ms}ms）",
        translated.chars().count()
    ));
    // 输入框转译走的是内部调用，不经命令层，历史要在这里补记
    let _ = crate::history::record(
        &dir,
        crate::history::NewEntry {
            kind: "input".into(),
            source: original.clone(),
            translated: translated.clone(),
            service_name: svc.name.clone(),
            elapsed_ms: ms as i64,
            ok: true,
            error: None,
        },
    );

    // 3) 焦点快照校验：焦点变了绝不写回，降级为复制
    if foreground_hwnd() != fg_before {
        degrade_to_clipboard(app, &mut cb, &translated, "目标窗口已失焦");
        return Ok(());
    }

    // 4) 写回：译文上剪贴板 → Ctrl+A 全选 → Ctrl+V 粘贴 → 恢复原剪贴板
    cb.set_text(translated.clone())
        .map_err(|e| format!("设置剪贴板失败: {e}"))?;
    simulate_ctrl_a().map_err(|e| format!("模拟 Ctrl+A 失败: {e}"))?;
    std::thread::sleep(Duration::from_millis(150));
    simulate_ctrl_v().map_err(|e| format!("模拟 Ctrl+V 失败: {e}"))?;
    std::thread::sleep(Duration::from_millis(300));
    if let Some(p) = &prev {
        let _ = cb.set_text(p.clone());
    }
    log_line("input: 写回完成");
    // 原位替换外加一句轻提示：静默覆盖会让人以为软件坏了
    crate::notice::show(app, "已替换 · Ctrl+Z 撤销", "ok");
    Ok(())
}

/// 弹窗里的「替换原文」：把译文粘回取词时所在的那个窗口。
///
/// 取词时记下了目标窗口句柄；这里先把弹窗藏起来让焦点回到目标窗口，
/// 校验前台窗口确实是它之后才粘贴，否则取消并恢复剪贴板。
#[cfg(windows)]
#[tauri::command]
pub fn replace_selection(app: AppHandle, text: String) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("没有可替换的译文".into());
    }
    if let Some(win) = app.get_webview_window("popup") {
        let _ = win.hide();
    }

    let target = crate::selection::last_target();
    if target == 0 {
        return Err("还没有取词目标，请重新划词后再替换".into());
    }

    let mut cb = Clipboard::new().map_err(|e| format!("剪贴板打开失败: {e}"))?;
    let prev = cb.get_text().ok();
    cb.set_text(text).map_err(|e| format!("设置剪贴板失败: {e}"))?;

    std::thread::sleep(Duration::from_millis(150));
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
        unsafe {
            let _ = SetForegroundWindow(HWND(target as *mut core::ffi::c_void));
        }
    }
    std::thread::sleep(Duration::from_millis(250));

    let restore = |cb: &mut Clipboard, prev: &Option<String>| {
        if let Some(p) = prev {
            let _ = cb.set_text(p.clone());
        }
    };

    if crate::selection::foreground_hwnd() != target {
        restore(&mut cb, &prev);
        return Err("目标窗口没有回到前台，已取消替换".into());
    }

    if let Err(e) = simulate_ctrl_v() {
        restore(&mut cb, &prev);
        return Err(format!("模拟 Ctrl+V 失败: {e}"));
    }
    std::thread::sleep(Duration::from_millis(300));
    restore(&mut cb, &prev);
    log_line("input: 替换原文完成");
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn replace_selection(_app: AppHandle, _text: String) -> Result<(), String> {
    Err("当前平台暂不支持替换原文".into())
}

#[cfg(test)]
mod tests {
    use super::looks_like_whole_page;

    #[test]
    fn rejects_page_sized_captures() {
        assert!(!looks_like_whole_page("这份报告我明天上午发给你"));
        assert!(!looks_like_whole_page(&"字".repeat(5000)));
        assert!(looks_like_whole_page(&"字".repeat(5001)));
    }
}
