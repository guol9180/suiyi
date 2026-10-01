//! M4 朗读：Windows 本地语音合成 + MCI 播放，进度条上每个数字都是真的。
//!
//! 为什么从 PlaySoundW 换成 MCI：PlaySound 只负责把声音放出来，拿不到播放位置，
//! 界面上那条进度条就只能靠估算，等于编数据。MCI 能查 position 与 length，
//! 还能暂停、续播、定位，于是时长、进度、暂停状态全部来自系统。
//!
//! 与 OCR 同理，WinRT 调用固定在独立 MTA 线程上跑：
//! 命令线程可能已被 WebView 初始化成 STA，在 STA 上 await 异步结果会死锁。

use std::sync::{Mutex, OnceLock};

use serde::Serialize;

use crate::selection::log_line;

/// MCI 设备别名。同一时刻只允许一个朗读会话，固定别名就够用
const ALIAS: &str = "suiyi_tts";
/// 默认语速倍数，1.0 为原速
pub const DEFAULT_RATE: f64 = 1.0;
/// 语速上下限，界面上的 0.8× / 1.0× / 1.5× 都落在里面
pub const MIN_RATE: f64 = 0.5;
pub const MAX_RATE: f64 = 2.0;

/// 给 serde 的 default 用：老配置里没有 speech_rate 时按原速
pub fn default_rate() -> f64 {
    DEFAULT_RATE
}

/// 合成结果落在这里，播放期间必须存在，因此用固定路径反复覆盖
fn wav_path() -> std::path::PathBuf {
    std::env::temp_dir().join("suiyi-tts.wav")
}

/// 系统里可用的语音
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpeechVoice {
    pub id: String,
    pub name: String,
    /// BCP-47 语言标签，如 zh-CN
    pub language: String,
    pub gender: String,
}

/// 一次朗读会话的完整状态，界面直接照着渲染
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpeechState {
    /// 有没有正在进行的会话；false 时其余字段都无意义
    pub active: bool,
    pub playing: bool,
    pub paused: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub rate: f64,
    /// 音色的显示名，空串表示用的是系统默认
    pub voice: String,
    /// 正在朗读的文本，界面上用来显示「正在朗读：xxx」
    pub text: String,
}

impl SpeechState {
    pub fn idle() -> Self {
        Self {
            active: false,
            playing: false,
            paused: false,
            position_ms: 0,
            duration_ms: 0,
            rate: DEFAULT_RATE,
            voice: String::new(),
            text: String::new(),
        }
    }
}

/// 会话里那些 MCI 查不到的元信息
struct Session {
    duration_ms: u64,
    rate: f64,
    voice: String,
    text: String,
}

static SESSION: OnceLock<Mutex<Option<Session>>> = OnceLock::new();

fn session() -> &'static Mutex<Option<Session>> {
    SESSION.get_or_init(|| Mutex::new(None))
}

// ==================== Windows 实现 ====================

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::HSTRING;
    use windows::Media::SpeechSynthesis::SpeechSynthesizer;
    use windows::Storage::Streams::DataReader;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    /// 进线程前先按 MTA 初始化 COM：命令线程可能已经被 WebView 占成 STA
    fn init_com() -> Result<(), String> {
        unsafe {
            const RPC_E_CHANGED_MODE: i32 = 0x8001_0106u32 as i32;
            match CoInitializeEx(None, COINIT_MULTITHREADED).ok() {
                Ok(()) => Ok(()),
                Err(e) if e.code().0 == RPC_E_CHANGED_MODE => Ok(()),
                Err(e) => Err(format!("COM 初始化失败 (0x{:08X}): {e}", e.code().0 as u32)),
            }
        }
    }

    /// 发一条 MCI 命令，返回它的字符串结果
    pub fn mci(cmd: &str) -> Result<String, String> {
        use windows::core::PCWSTR;
        use windows::Win32::Media::Multimedia::{mciGetErrorStringW, mciSendStringW};

        let wide: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
        let mut buf = [0u16; 128];
        let code = unsafe { mciSendStringW(PCWSTR(wide.as_ptr()), Some(&mut buf), None) };
        if code != 0 {
            let mut msg = [0u16; 256];
            unsafe {
                let _ = mciGetErrorStringW(code, &mut msg);
            }
            let text = String::from_utf16_lossy(&msg)
                .trim_end_matches('\0')
                .trim()
                .to_string();
            return Err(if text.is_empty() {
                format!("播放命令失败（MCI {code}）")
            } else {
                text
            });
        }
        Ok(String::from_utf16_lossy(&buf)
            .trim_end_matches('\0')
            .trim()
            .to_string())
    }

    /// 关掉设备。设备本来就没开时这里会失败，属于预期，直接忽略
    fn close_quietly() {
        let _ = mci(&format!("close {ALIAS}"));
    }

    pub fn play_file(path: &std::path::Path) -> Result<u64, String> {
        close_quietly();
        let file = path.to_string_lossy().to_string();
        mci(&format!("open \"{file}\" type waveaudio alias {ALIAS}"))?;

        // 打开成功后立刻问长度；这条失败说明驱动不支持，直接当作打不开
        let duration = match mci(&format!("status {ALIAS} length")) {
            Ok(v) => v.trim().parse::<u64>().unwrap_or(0),
            Err(e) => {
                close_quietly();
                return Err(format!("读取音频长度失败: {e}"));
            }
        };

        if let Err(e) = mci(&format!("play {ALIAS}")) {
            close_quietly();
            return Err(format!("播放失败，系统没有可用的音频输出: {e}"));
        }
        Ok(duration)
    }

    fn number(cmd: &str) -> Option<u64> {
        mci(cmd).ok()?.trim().parse::<u64>().ok()
    }

    /// 读一次设备状态：(模式, 位置毫秒)
    pub fn mode_and_position() -> (String, u64) {
        let mode = mci(&format!("status {ALIAS} mode")).unwrap_or_default();
        let pos = number(&format!("status {ALIAS} position")).unwrap_or(0);
        (mode.trim().to_lowercase(), pos)
    }

    pub fn pause_device() -> Result<(), String> {
        mci(&format!("pause {ALIAS}")).map(|_| ())
    }

    pub fn resume_device() -> Result<(), String> {
        mci(&format!("resume {ALIAS}")).map(|_| ())
    }

    pub fn stop_device() {
        let _ = mci(&format!("stop {ALIAS}"));
        close_quietly();
    }

    /// 把文本合成为 WAV 字节。与播放分开，方便在不出声的前提下验收本地语音。
    pub fn synthesize(text: &str, rate: f64, voice: Option<&str>) -> Result<Vec<u8>, String> {
        init_com()?;
        let synth = SpeechSynthesizer::new().map_err(|e| format!("创建语音合成器失败: {e}"))?;

        // 指定音色：系统里找不到那一个就退回默认音色，
        // 不因为音色对不上而让整次朗读失败
        if let Some(want) = voice.filter(|v| !v.is_empty()) {
            match SpeechSynthesizer::AllVoices() {
                Ok(list) => {
                    for i in 0..list.Size().unwrap_or(0) {
                        let Ok(v) = list.GetAt(i) else { continue };
                        let is_hit = v.Id().map(|id| id.to_string() == want).unwrap_or(false)
                            || v.DisplayName()
                                .map(|n| n.to_string() == want)
                                .unwrap_or(false);
                        if is_hit {
                            let _ = synth.SetVoice(&v);
                            break;
                        }
                    }
                }
                Err(e) => log_line(&format!("speech: 读取音色列表失败，改用默认音色（{e}）")),
            }
        }

        let rate = rate.clamp(MIN_RATE, MAX_RATE);
        if (rate - DEFAULT_RATE).abs() > f64::EPSILON {
            if let Ok(opts) = synth.Options() {
                if let Err(e) = opts.SetSpeakingRate(rate) {
                    log_line(&format!("speech: 设置语速失败，按原速朗读（{e}）"));
                }
            }
        }

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
        let reader =
            DataReader::CreateDataReader(&input).map_err(|e| format!("创建读取器失败: {e}"))?;
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

    pub fn voices() -> Result<Vec<SpeechVoice>, String> {
        init_com()?;
        let list = SpeechSynthesizer::AllVoices().map_err(|e| format!("读取音色列表失败: {e}"))?;
        let mut out = Vec::new();
        for i in 0..list.Size().unwrap_or(0) {
            let Ok(v) = list.GetAt(i) else { continue };
            out.push(SpeechVoice {
                id: v.Id().map(|s| s.to_string()).unwrap_or_default(),
                name: v.DisplayName().map(|s| s.to_string()).unwrap_or_default(),
                language: v.Language().map(|s| s.to_string()).unwrap_or_default(),
                gender: v
                    .Gender()
                    .map(|g| format!("{:?}", g.0))
                    .unwrap_or_else(|_| "Unknown".into()),
            });
        }
        Ok(out)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    const UNSUPPORTED: &str = "当前平台暂不支持朗读";

    pub fn mci(_cmd: &str) -> Result<String, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn play_file(_path: &std::path::Path) -> Result<u64, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn mode_and_position() -> (String, u64) {
        (String::new(), 0)
    }
    pub fn pause_device() -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    pub fn resume_device() -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    pub fn stop_device() {}
    pub fn synthesize(_text: &str, _rate: f64, _voice: Option<&str>) -> Result<Vec<u8>, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn voices() -> Result<Vec<SpeechVoice>, String> {
        Err(UNSUPPORTED.into())
    }
}

// ==================== 会话 ====================

/// 合成并开始朗读，返回这一段的状态（含系统给出的真实时长）
pub fn speak(text: &str, rate: f64, voice: &str) -> Result<SpeechState, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("没有可朗读的文本".into());
    }
    if text.chars().count() > 2000 {
        return Err("文本过长，朗读上限 2000 字".into());
    }
    let spoken: String = text.chars().take(2000).collect();

    let rate = rate.clamp(MIN_RATE, MAX_RATE);
    let voice = voice.trim().to_string();
    let (t, v) = (spoken.clone(), voice.clone());
    let wav = std::thread::spawn(move || imp::synthesize(&t, rate, Some(&v)))
        .join()
        .map_err(|_| "朗读线程异常退出".to_string())??;

    let path = wav_path();
    std::fs::write(&path, &wav).map_err(|e| format!("写入临时音频失败: {e}"))?;
    let duration = imp::play_file(&path)?;
    log_line(&format!(
        "speech: 合成 {} 字节，时长 {}ms，开始播放",
        wav.len(),
        duration
    ));

    if let Ok(mut guard) = session().lock() {
        *guard = Some(Session {
            duration_ms: duration,
            rate,
            voice,
            text: spoken,
        });
    }
    Ok(state())
}

/// 当前朗读状态。放完之后自动收尾，界面不用自己判断什么时候结束
pub fn state() -> SpeechState {
    let Ok(mut guard) = session().lock() else {
        return SpeechState::idle();
    };
    let Some(current) = guard.as_ref() else {
        return SpeechState::idle();
    };
    let (duration_ms, rate, voice, text) = (
        current.duration_ms,
        current.rate,
        current.voice.clone(),
        current.text.clone(),
    );

    let (mode, position_ms) = imp::mode_and_position();
    // mode 停在 stopped 说明已经放完；暂停时是 paused，不会被误判成结束
    if mode != "playing" && mode != "paused" && mode != "seeking" {
        imp::stop_device();
        *guard = None;
        return SpeechState::idle();
    }

    SpeechState {
        active: true,
        playing: mode == "playing",
        paused: mode == "paused",
        position_ms,
        duration_ms,
        rate,
        voice,
        text,
    }
}

pub fn pause() -> Result<SpeechState, String> {
    if session().lock().map(|g| g.is_none()).unwrap_or(true) {
        return Err("当前没有正在朗读的内容".into());
    }
    imp::pause_device()?;
    Ok(state())
}

pub fn resume() -> Result<SpeechState, String> {
    if session().lock().map(|g| g.is_none()).unwrap_or(true) {
        return Err("当前没有暂停中的朗读".into());
    }
    imp::resume_device()?;
    Ok(state())
}

/// 停止并清空会话。没在朗读时调用它也不报错
pub fn stop() -> SpeechState {
    imp::stop_device();
    if let Ok(mut guard) = session().lock() {
        *guard = None;
    }
    SpeechState::idle()
}

// ==================== 命令层 ====================

/// 朗读一段文本（本地离线合成）。rate 传 null 用原速，voice 传 null 用系统默认音色。
#[tauri::command]
pub async fn speak_text(
    app: tauri::AppHandle,
    text: String,
    rate: Option<f64>,
    voice: Option<String>,
) -> Result<SpeechState, String> {
    let rate = rate.unwrap_or(DEFAULT_RATE);
    let voice = voice.unwrap_or_default();

    // 没显式指定就跟着设置走：界面上选了音色与语速，各处朗读都该一致
    let dir = crate::commands::config_dir(&app)?;
    let file = crate::config::load_services(&dir)?;
    let rate = if rate == DEFAULT_RATE {
        file.speech_rate.clamp(MIN_RATE, MAX_RATE)
    } else {
        rate
    };
    let voice = if voice.is_empty() {
        file.speech_voice.clone()
    } else {
        voice
    };

    // 装了语音插件就优先用插件：用户装它说明要的就是它的音色
    if let Some(p) = crate::plugin::first_enabled(&dir, crate::plugin::PluginKind::Speech) {
        if let Some(main) = p.main.clone() {
            let args = serde_json::json!({ "text": text.clone() }).to_string();
            let permissions = p.permissions.clone();
            log_line(&format!("speech: 交给语音插件「{}」朗读", p.name));
            tauri::async_runtime::spawn_blocking(move || {
                crate::plugin_js::call(
                    std::path::Path::new(&main),
                    "speak",
                    &args,
                    &permissions,
                )
                .map(|_| ())
            })
            .await
            .map_err(|e| format!("语音插件调用失败: {e}"))??;
            // 插件自己管播放，这里没有可汇报的进度
            return Ok(SpeechState {
                active: true,
                playing: true,
                rate,
                voice: format!("插件 · {}", p.name),
                text,
                ..SpeechState::idle()
            });
        }
    }

    // 合成是阻塞式的，丢到阻塞线程池，别占住 async 运行时
    tauri::async_runtime::spawn_blocking(move || speak(&text, rate, &voice))
        .await
        .map_err(|e| format!("朗读任务失败: {e}"))?
}

/// 读当前朗读进度；界面按固定间隔轮询它来画进度条
#[tauri::command]
pub fn speech_state() -> SpeechState {
    state()
}

#[tauri::command]
pub fn pause_speaking() -> Result<SpeechState, String> {
    pause()
}

#[tauri::command]
pub fn resume_speaking() -> Result<SpeechState, String> {
    resume()
}

/// 停止当前朗读
#[tauri::command]
pub fn stop_speaking() -> SpeechState {
    stop()
}

/// 列出系统里可用的本地语音
#[tauri::command]
pub async fn list_speech_voices() -> Result<Vec<SpeechVoice>, String> {
    tauri::async_runtime::spawn_blocking(imp::voices)
        .await
        .map_err(|e| format!("读取音色列表失败: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_path_is_under_temp_dir() {
        let p = wav_path();
        assert!(p.ends_with("suiyi-tts.wav"));
        assert_eq!(p.parent(), Some(std::env::temp_dir().as_path()));
    }

    #[test]
    fn stopping_without_a_session_is_harmless() {
        let s = stop();
        assert!(!s.active);
        assert!(!s.playing);
        assert_eq!(s.position_ms, 0);
        assert_eq!(s.duration_ms, 0);
    }

    #[test]
    fn rate_is_clamped_to_supported_range() {
        assert_eq!(0.1f64.clamp(MIN_RATE, MAX_RATE), MIN_RATE);
        assert_eq!(9.0f64.clamp(MIN_RATE, MAX_RATE), MAX_RATE);
        assert_eq!(1.5f64.clamp(MIN_RATE, MAX_RATE), 1.5);
    }

    #[test]
    fn empty_text_is_rejected_before_touching_audio() {
        assert!(speak("   ", 1.0, "").is_err());
    }

    #[test]
    fn pausing_without_a_session_reports_instead_of_pretending() {
        assert!(pause().is_err());
        assert!(resume().is_err());
    }

    /// 真实验收本机语音：只合成不播放，不会出声。
    /// 显式运行：cargo test synthesizes_audio -- --ignored --nocapture
    #[test]
    #[ignore]
    #[cfg(windows)]
    fn synthesizes_audio_offline() {
        let wav = imp::synthesize("这是随译的朗读测试", 1.0, None).expect("本地语音合成失败");
        assert!(wav.len() > 1000, "合成结果过短: {} 字节", wav.len());
        assert_eq!(&wav[0..4], b"RIFF", "不是合法的 WAV 数据");
        println!("SPEECH_OK | 合成 {} 字节", wav.len());
    }

    /// 验收 MCI 这一层：打开 WAV 问长度再关掉，全程不调用 play，不会出声。
    /// 显式运行：cargo test mci_reads_wav -- --ignored --nocapture
    #[test]
    #[ignore]
    #[cfg(windows)]
    fn mci_reads_wav_length_without_playing() {
        let wav = imp::synthesize("这是随译的朗读测试", 1.0, None).expect("本地语音合成失败");
        let path = wav_path();
        std::fs::write(&path, &wav).expect("写入临时音频失败");

        let _ = imp::mci(&format!("close {ALIAS}"));
        imp::mci(&format!(
            "open \"{}\" type waveaudio alias {ALIAS}",
            path.to_string_lossy()
        ))
        .expect("打开音频设备失败");
        let ms: u64 = imp::mci(&format!("status {ALIAS} length"))
            .expect("读取长度失败")
            .trim()
            .parse()
            .expect("长度不是数字");
        let _ = imp::mci(&format!("close {ALIAS}"));

        assert!(ms > 200, "时长过短，像没读到真实长度: {ms}ms");
        println!("MCI_OK | 真实时长 {ms}ms");
    }

    /// 列出本机可用音色。纯查询，不出声。
    /// 显式运行：cargo test lists_voices -- --ignored --nocapture
    #[test]
    #[ignore]
    #[cfg(windows)]
    fn lists_system_voices() {
        let vs = imp::voices().expect("读取音色列表失败");
        assert!(!vs.is_empty(), "系统里一个语音都没有");
        for v in &vs {
            assert!(!v.id.is_empty());
            println!("VOICE | {} | {} | {}", v.name, v.language, v.gender);
        }
    }
}
