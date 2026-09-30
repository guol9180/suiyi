//! M4 朗读：用 Windows 本地语音合成器（离线、无额外成本）朗读文本。
//!
//! 流程：`SpeechSynthesizer` 把文本合成为 WAV → 写进临时文件 → `PlaySoundW`
//! 异步播放。选本地引擎是刻意的：朗读不该消耗翻译额度，也不该要求联网。
//!
//! 与 OCR 同理，WinRT 调用固定在独立 MTA 线程上跑：
//! 命令线程可能已被 WebView 初始化成 STA，在 STA 上 await 异步结果会死锁。

use std::time::Duration;

use crate::selection::log_line;

/// 合成结果落在这里，播放期间必须存在，因此用固定路径反复覆盖
fn wav_path() -> std::path::PathBuf {
    std::env::temp_dir().join("suiyi-tts.wav")
}

#[cfg(windows)]
pub fn speak(text: &str) -> Result<(), String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("没有可朗读的文本".into());
    }
    if text.chars().count() > 2000 {
        return Err("文本过长，朗读上限 2000 字".into());
    }
    let spoken: String = text.chars().take(2000).collect();
    let wav = std::thread::spawn(move || synthesize(&spoken))
        .join()
        .map_err(|_| "朗读线程异常退出".to_string())??;

    let path = wav_path();
    std::fs::write(&path, &wav).map_err(|e| format!("写入临时音频失败: {e}"))?;
    log_line(&format!("speech: 合成 {} 字节，开始播放", wav.len()));
    play_file(&path)
}

/// 把文本合成为 WAV 字节。与播放分开，便于无副作用地测试本地语音是否可用。
#[cfg(windows)]
pub fn synthesize(text: &str) -> Result<Vec<u8>, String> {
    use windows::core::HSTRING;
    use windows::Media::SpeechSynthesis::SpeechSynthesizer;
    use windows::Storage::Streams::DataReader;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    unsafe {
        const RPC_E_CHANGED_MODE: i32 = 0x8001_0106u32 as i32;
        match CoInitializeEx(None, COINIT_MULTITHREADED).ok() {
            Ok(()) => {}
            Err(e) if e.code().0 == RPC_E_CHANGED_MODE => {}
            Err(e) => return Err(format!("COM 初始化失败 (0x{:08X}): {e}", e.code().0 as u32)),
        }
    }

    let synth = SpeechSynthesizer::new().map_err(|e| format!("创建语音合成器失败: {e}"))?;
    let stream = synth
        .SynthesizeTextToStreamAsync(&HSTRING::from(text))
        .map_err(|e| format!("发起合成失败: {e}"))?
        .get()
        .map_err(|e| format!("合成失败: {e}"))?;

    let size = stream.Size().map_err(|e| format!("读取音频长度失败: {e}"))?;
    if size == 0 {
        return Err("合成结果为空".into());
    }
    let input = stream
        .GetInputStreamAt(0)
        .map_err(|e| format!("打开音频流失败: {e}"))?;
    let reader = DataReader::CreateDataReader(&input).map_err(|e| format!("创建读取器失败: {e}"))?;
    reader
        .LoadAsync(size as u32)
        .map_err(|e| format!("加载音频失败: {e}"))?
        .get()
        .map_err(|e| format!("加载音频等待失败: {e}"))?;

    let mut buf = vec![0u8; size as usize];
    reader
        .ReadBytes(&mut buf)
        .map_err(|e| format!("读取音频失败: {e}"))?;
    Ok(buf)
}

#[cfg(windows)]
fn play_file(path: &std::path::Path) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME};

    let wide: Vec<u16> = path
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SND_ASYNC：立刻返回，播放交给系统；文件在播放期间保留
    let ok = unsafe { PlaySoundW(PCWSTR(wide.as_ptr()), None, SND_FILENAME | SND_ASYNC) };
    if !ok.as_bool() {
        return Err("播放失败，系统没有可用的音频输出".into());
    }
    // 给系统一点时间把文件读进内存，再允许后续操作
    std::thread::sleep(Duration::from_millis(60));
    Ok(())
}

#[cfg(windows)]
pub fn stop() {
    use windows::core::PCWSTR;
    use windows::Win32::Media::Audio::{PlaySoundW, SND_PURGE};
    let _ = unsafe { PlaySoundW(PCWSTR::null(), None, SND_PURGE) };
}

#[cfg(not(windows))]
pub fn speak(_text: &str) -> Result<(), String> {
    Err("当前平台暂不支持朗读".into())
}

#[cfg(not(windows))]
pub fn stop() {}

// ==================== 命令层 ====================

/// 朗读一段文本（本地离线合成）
#[tauri::command]
pub async fn speak_text(text: String) -> Result<(), String> {
    // 合成是阻塞式的，丢到阻塞线程池，别占住 async 运行时
    tauri::async_runtime::spawn_blocking(move || speak(&text))
        .await
        .map_err(|e| format!("朗读任务失败: {e}"))?
}

/// 停止当前朗读
#[tauri::command]
pub fn stop_speaking() {
    stop();
}

#[cfg(test)]
mod tests {
    use super::wav_path;

    #[test]
    fn wav_path_is_under_temp_dir() {
        let p = wav_path();
        assert!(p.ends_with("suiyi-tts.wav"));
        assert_eq!(p.parent(), Some(std::env::temp_dir().as_path()));
    }

    /// 真实验收本机语音：只合成不播放，避免打扰。
    /// 显式运行：cargo test synthesizes_audio -- --ignored --nocapture
    #[test]
    #[ignore]
    #[cfg(windows)]
    fn synthesizes_audio_offline() {
        let wav = super::synthesize("这是随译的朗读测试").expect("本地语音合成失败");
        assert!(wav.len() > 1000, "合成结果过短: {} 字节", wav.len());
        assert_eq!(&wav[0..4], b"RIFF", "不是合法的 WAV 数据");
        println!("SPEECH_OK | 合成 {} 字节", wav.len());
    }
}
