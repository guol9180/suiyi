//! 检查更新 / 下载安装包 / 拉起安装程序。
//!
//! 这里不走 Tauri 官方 updater 插件：那套要求仓库里配一个签名私钥 Secret，
//! 而这个仓库没有。改成直接读 GitHub Releases 的公开接口：
//! - `check_update`：取 releases/latest，比对 tag 与本机版本
//! - `download_update`：把 Setup 包下到临时目录，边下边把进度推给界面
//! - `install_update`：校验下载到的确实是 PE 可执行文件，退出随译并拉起安装向导
//!
//! 安装交给安装器自己的向导，不做静默安装：静默安装会用默认目录 `%LOCALAPPDATA%`，
//! 而用户很可能当初装在了别的盘（本机就装在 D:\software\SuiYi），静默重装会变成两份。

use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const RELEASES_API: &str = "https://api.github.com/repos/guol9180/suiyi/releases/latest";
const DOWNLOAD_PAGE: &str = "https://suiyi.imhgl.com/";
const ASSET_NAME: &str = "SuiYi-Setup-x64.exe";
/// 安装包最小体积。比这还小一定不是完整产物（正常 3MB 左右）
const MIN_INSTALLER_BYTES: u64 = 500 * 1024;
/// 镜像站上的下载目录（Pages 每次部署会把最新安装包同步到这里）
const MIRROR_BASE: &str = "https://suiyi.imhgl.com/dl/";
/// 下载时多久没收到数据算卡死
const STALL_TIMEOUT: Duration = Duration::from_secs(60);
/// 每个地址最多试几次
const DOWNLOAD_ATTEMPTS: u32 = 2;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub has_update: bool,
    /// release 正文，界面上原样展示
    pub notes: String,
    pub published_at: String,
    /// 安装包直链
    pub asset_url: String,
    /// release 页面 / 失败时的兜底下载页
    pub page_url: String,
}

/// 版本号比较：0.8.1 > 0.8.0，0.10.0 > 0.9.9。
/// 只按点分段做数值比较，非数字段按 0 处理 —— 我们自己的 tag 都是 x.y.z，够用。
fn version_gt(a: &str, b: &str) -> bool {
    fn parse(s: &str) -> Vec<u64> {
        s.trim()
            .trim_start_matches('v')
            .split(['.', '-', '+'])
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    }
    let (va, vb) = (parse(a), parse(b));
    for i in 0..va.len().max(vb.len()) {
        let (x, y) = (
            va.get(i).copied().unwrap_or(0),
            vb.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

/// 查版本用的客户端：JSON 很小，给个总超时足够
fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        // GitHub API 强制要求 User-Agent，缺了会直接 403
        .user_agent("SuiYi-Updater")
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("初始化网络失败: {e}"))
}

/// 下载安装包用的客户端。
///
/// 关键区别：**不能设总超时**。之前和查版本共用一个 20 秒总超时的客户端，
/// 3MB 的安装包在国内网络下慢一点就必然被掐断，用户看到的就是
/// 「下载失败：error sending request for url(...)」。这里只限制连接建立时间，
/// 传输过程中的卡死由 STALL_TIMEOUT 判断。
fn download_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("SuiYi-Updater")
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("初始化网络失败: {e}"))
}

/// 查一次最新版本
#[tauri::command]
pub async fn check_update() -> Result<UpdateInfo, String> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let resp = client()?
        .get(RELEASES_API)
        .send()
        .await
        .map_err(|e| format!("连不上更新服务：{e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(match status.as_u16() {
            403 => "更新服务暂时不可用（请求被限流），稍后再试".to_string(),
            404 => "还没发布过正式版本".to_string(),
            code => format!("更新服务返回 {code}"),
        });
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析更新信息失败：{e}"))?;
    let latest = body
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim_start_matches('v')
        .to_string();
    let page_url = body
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or(DOWNLOAD_PAGE)
        .to_string();
    // 资产名在 CI 里是固定归一化的，找不到也退回固定直链
    let asset_url = body
        .get("assets")
        .and_then(|a| a.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(ASSET_NAME))
                .and_then(|a| a.get("browser_download_url").and_then(|u| u.as_str()))
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| {
            format!("https://github.com/guol9180/suiyi/releases/latest/download/{ASSET_NAME}")
        });
    Ok(UpdateInfo {
        has_update: !latest.is_empty() && version_gt(&latest, &current),
        latest,
        notes: body
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        published_at: body
            .get("published_at")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        asset_url,
        page_url,
        current,
    })
}

/// 候选下载地址：先走国内镜像（Pages 同步的那份），失败再回 GitHub 原始资产地址。
///
/// GitHub 的 release 资产实际落在 objects.githubusercontent.com 上，国内经常连不上；
/// 镜像挂在项目自己的域名下，速度快得多。两个都失败才报错，并把各自的原因带回去。
fn download_candidates(asset_url: &str) -> Vec<String> {
    let mut urls: Vec<String> = Vec::new();
    if let Some(name) = asset_url.rsplit('/').next() {
        if name.ends_with(".exe") || name.ends_with(".msi") || name.ends_with(".zip") {
            urls.push(format!("{MIRROR_BASE}{name}"));
        }
    }
    urls.push(asset_url.to_string());
    urls.dedup();
    urls
}

/// 从候选地址里下安装包，返回本地路径与用到的地址。进度通过 update-progress 推给界面。
#[tauri::command]
pub async fn download_update(app: AppHandle, url: String) -> Result<String, String> {
    use futures_util::StreamExt;
    use std::io::Write;

    let dir = std::env::temp_dir().join("suiyi-update");
    std::fs::create_dir_all(&dir).map_err(|e| format!("准备临时目录失败：{e}"))?;
    let path = dir.join(ASSET_NAME);
    let client = download_client()?;
    let candidates = download_candidates(&url);
    let mut reasons: Vec<String> = Vec::new();

    for candidate in &candidates {
        for attempt in 1..=DOWNLOAD_ATTEMPTS {
            let host = candidate
                .split("//")
                .nth(1)
                .and_then(|s| s.split('/').next())
                .unwrap_or(candidate)
                .to_string();
            crate::selection::log_line(&format!(
                "update: 下载尝试 {attempt}/{DOWNLOAD_ATTEMPTS} ← {candidate}"
            ));
            let attempt_result: Result<(), String> = async {
                let resp = client
                    .get(candidate)
                    .send()
                    .await
                    .map_err(|e| format!("连接失败（{e}）"))?;
                if !resp.status().is_success() {
                    return Err(format!("服务返回 {}", resp.status().as_u16()));
                }
                let total = resp.content_length().unwrap_or(0);
                let mut file =
                    std::fs::File::create(&path).map_err(|e| format!("创建文件失败：{e}"))?;
                let mut got: u64 = 0;
                let mut stream = resp.bytes_stream();
                loop {
                    // 卡死检测：传了一半没动静，比等一个总超时更早给出反馈
                    let chunk = match tokio::time::timeout(STALL_TIMEOUT, stream.next()).await {
                        Ok(Some(chunk)) => chunk.map_err(|e| format!("传输中断（{e}）"))?,
                        Ok(None) => break,
                        Err(_) => return Err(format!("超过 {} 秒没有数据", STALL_TIMEOUT.as_secs())),
                    };
                    file.write_all(&chunk).map_err(|e| format!("写盘失败：{e}"))?;
                    got += chunk.len() as u64;
                    let _ = app.emit(
                        "update-progress",
                        serde_json::json!({ "downloaded": got, "total": total }),
                    );
                }
                drop(file);
                if got < MIN_INSTALLER_BYTES {
                    return Err(format!("文件不完整（只有 {} KB）", got / 1024));
                }
                // MZ 头：确认拿到的是 Windows 可执行文件，而不是一个错误页
                let mut head = [0u8; 2];
                {
                    use std::io::Read;
                    std::fs::File::open(&path)
                        .and_then(|mut f| f.read_exact(&mut head))
                        .map_err(|e| format!("读取下载文件失败：{e}"))?;
                }
                if &head != b"MZ" {
                    return Err("拿到的不是可执行文件".into());
                }
                Ok(())
            }
            .await;

            match attempt_result {
                Ok(()) => {
                    crate::selection::log_line(&format!("update: 下载完成 ← {host}"));
                    return Ok(path.to_string_lossy().to_string());
                }
                Err(e) => {
                    crate::selection::log_line(&format!("update: {host} 第 {attempt} 次失败：{e}"));
                    reasons.push(format!("{host}：{e}"));
                }
            }
        }
    }

    Err(format!(
        "下载失败：{}。可以点「打开下载页」手动安装。",
        reasons.join("；")
    ))
}

/// 启动安装向导并退出随译。
///
/// 必须先退出：安装器要覆盖正在运行的 suiyi.exe，Windows 不允许。装完由安装向导
/// 最后一步的「运行 SuiYi」把它带回来（Tauri 的 NSIS 模板默认勾选）。
#[tauri::command]
pub fn install_update(app: AppHandle, path: String) -> Result<(), String> {
    let installer = std::path::PathBuf::from(&path);
    if !installer.exists() {
        return Err("安装包不存在，请重新下载".into());
    }
    let mut cmd = std::process::Command::new(&installer);
    // 让安装向导自己选目录：用户当初可能装在非默认路径，静默安装会变成两份
    cmd.current_dir(std::env::temp_dir());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // 别弹出控制台黑框
        cmd.creation_flags(0x0800_0000);
    }
    cmd.spawn()
        .map_err(|e| format!("启动安装程序失败：{e}"))?;
    crate::selection::log_line("update: 已拉起安装向导，退出随译");
    app.exit(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::version_gt;

    #[test]
    fn version_compare_handles_leading_v_and_multidigit() {
        assert!(version_gt("v0.8.1", "0.8.0"));
        assert!(version_gt("0.10.0", "0.9.9"));
        assert!(version_gt("0.8.0", "0.7.9"));
        assert!(!version_gt("0.8.0", "0.8.0"), "相同版本不算有新版本");
        assert!(!version_gt("0.7.9", "0.8.0"), "旧版本不能算更新");
        // 带后缀的预发布版本按数字段比较，0.8.0-rc1 == 0.8.0，不提示更新
        assert!(!version_gt("0.8.0-rc1", "0.8.0"));
    }

    /// 下载地址：先试国内镜像（同一个文件名），再回落到 GitHub 原始地址
    #[test]
    fn download_candidates_prefers_the_mirror() {
        let urls = super::download_candidates(
            "https://github.com/guol9180/suiyi/releases/download/v0.9.0/SuiYi-Setup-x64.exe",
        );
        assert_eq!(urls.len(), 2, "镜像 + 原始地址：{urls:?}");
        assert_eq!(urls[0], "https://suiyi.imhgl.com/dl/SuiYi-Setup-x64.exe");
        assert!(urls[1].starts_with("https://github.com/"));

        // 不是安装包后缀的地址不瞎拼镜像
        let only_github = super::download_candidates("https://example.com/whatever");
        assert_eq!(only_github, vec!["https://example.com/whatever".to_string()]);
    }
}
