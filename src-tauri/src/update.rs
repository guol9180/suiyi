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
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const RELEASES_API: &str = "https://api.github.com/repos/guol9180/suiyi/releases/latest";
const DOWNLOAD_PAGE: &str = "https://suiyi.imhgl.com/";
const ASSET_NAME: &str = "SuiYi-Setup-x64.exe";
/// 安装包最小体积。比这还小一定不是完整产物（含离线 OCR 模型，实际约 22MB）
const MIN_INSTALLER_BYTES: u64 = 500 * 1024;
/// 镜像站上的下载目录（Pages 每次部署会把最新安装包同步到这里）
const MIRROR_BASE: &str = "https://suiyi.imhgl.com/dl/";
/// 下载时多久没收到数据算卡死
const STALL_TIMEOUT: Duration = Duration::from_secs(60);
/// 每个地址最多试几次
const DOWNLOAD_ATTEMPTS: u32 = 2;
/// 进度事件的最小间隔。22MB 有近三千个 chunk，一块一发会让界面每秒重渲染上千次，
/// 进度条看着就在抖。百分比每跨 1 也会补发一次，所以慢网速下依然走动。
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
/// 用户主动取消时返回的原因。界面按「自己点的取消」处理，不当故障展示
pub const CANCELLED: &str = "已取消下载";
/// 半成品文件后缀：先写 .part，校验通过才改名成正式安装包
const PART_SUFFIX: &str = ".part";
/// 分段下载的并发连接数。镜像与 GitHub 走同一条 Fastly 线路，实测单连接被压到
/// ~28KB/s（23MB 要 15 分钟），4 连接并发实测合计 159KB/s——限速按连接算，多开有效
const PARALLEL_CONNECTIONS: usize = 6;
/// 文件小于这个值不分段：块太小往返耗时占比高，单连接顺序下更省
const PARALLEL_MIN_BYTES: u64 = 2 * 1024 * 1024;
/// 单段至少这么大方值得独立成段（段数会被它压下来）
const MIN_CHUNK_BYTES: u64 = 512 * 1024;
/// 每个分段自身的重试次数（Range 不变，重写该段区间，不影响其他段）
const CHUNK_ATTEMPTS: u32 = 3;

/// 更新下载的进程级状态。
///
/// 两件事必须挡在 Rust 这一侧，光靠界面拦不住：
/// - 单飞：关于页卸载再回来时按钮会复原，用户再点一次就有两个下载同时写同一个
///   文件，各自的已收字节交替上报，进度条必然左右横跳。
/// - 取消：请求跑在 Rust 的任务里，界面拿不到句柄，只能靠一个标志位喊停。
#[derive(Default, Clone)]
pub struct UpdateState(Arc<UpdateStateInner>);

#[derive(Default)]
struct UpdateStateInner {
    busy: AtomicBool,
    cancel: AtomicBool,
}

impl UpdateState {
    /// 抢占下载权；已经有下载在跑就返回 false
    fn begin(&self) -> bool {
        self.0
            .busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    fn finish(&self) {
        self.0.cancel.store(false, Ordering::SeqCst);
        self.0.busy.store(false, Ordering::SeqCst);
    }

    /// 取消正在进行的下载；没有下载在跑时什么都不做
    fn request_cancel(&self) -> bool {
        if !self.0.busy.load(Ordering::SeqCst) {
            return false;
        }
        self.0.cancel.store(true, Ordering::SeqCst);
        true
    }

    fn is_cancelled(&self) -> bool {
        self.0.cancel.load(Ordering::SeqCst)
    }
}

/// 单飞守卫：正常返回、报错、panic 都会把下载权还回去
struct BusyGuard(UpdateState);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.finish();
    }
}

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
        if name.ends_with(".exe") {
            urls.push(format!("{MIRROR_BASE}{name}"));
        }
    }
    urls.push(asset_url.to_string());
    urls.dedup();
    urls
}

/// 一条下载进度。`state` 为 `retrying` 时表示正在换地址或重试，
/// 界面据此停在当前高水位并标注「正在重试」，而不是把进度条打回 0。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
    /// 第几次尝试（跨地址连续计数），从 1 开始
    pub attempt: u32,
    /// 正在下载的主机名：出错时用户能看出卡在哪条线路上
    pub candidate: String,
    pub state: &'static str,
}

/// 候选地址的主机名，只用于展示与日志
fn host_of(url: &str) -> String {
    url.split("//")
        .nth(1)
        .and_then(|s| s.split('/').next())
        .unwrap_or(url)
        .to_string()
}

/// 落盘之后的完整性检查。抽成独立函数是为了能直接测这几个边界：
/// 少收了字节、体积明显不对、拿到的不是 PE 文件。
fn verify_installer(path: &Path, got: u64, total: u64, min_bytes: u64) -> Result<(), String> {
    use std::io::Read;

    if total > 0 && got != total {
        return Err(format!("传输不完整（{got}/{total} 字节）"));
    }
    if got < min_bytes {
        return Err(format!("文件不完整（只有 {} KB）", got / 1024));
    }
    // MZ 头：确认拿到的是 Windows 可执行文件，而不是一个错误页
    let mut head = [0u8; 2];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .map_err(|e| format!("读取下载文件失败：{e}"))?;
    if &head != b"MZ" {
        return Err("拿到的不是可执行文件".into());
    }
    Ok(())
}

/// 从单个地址拉一次，写进 `part`。进度经 `on_progress` 上报（已节流）。
#[allow(clippy::too_many_arguments)]
async fn download_once(
    client: &reqwest::Client,
    url: &str,
    part: &Path,
    min_bytes: u64,
    attempt: u32,
    host: &str,
    state: &UpdateState,
    on_progress: &mut (impl FnMut(DownloadProgress) + Send),
) -> Result<(), String> {
    use futures_util::StreamExt;
    use std::io::Write;

    if state.is_cancelled() {
        return Err(CANCELLED.to_string());
    }
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("连接失败（{e}）"))?;
    if !resp.status().is_success() {
        return Err(format!("服务返回 {}", resp.status().as_u16()));
    }
    let total = resp.content_length().unwrap_or(0);
    let mut file = std::fs::File::create(part).map_err(|e| format!("创建文件失败：{e}"))?;
    // 先报一条「总量已知、一个字节还没收」：界面立刻能画出 0%，
    // 而不是在总量未知时先摆一个满格进度条再跳回去
    on_progress(DownloadProgress {
        downloaded: 0,
        total,
        attempt,
        candidate: host.to_string(),
        state: "downloading",
    });
    let mut got: u64 = 0;
    let mut last_emit = Instant::now();
    let mut last_pct: Option<u64> = None;
    let mut stream = resp.bytes_stream();
    loop {
        // 卡死检测：传了一半没动静，比等一个总超时更早给出反馈
        let chunk = match tokio::time::timeout(STALL_TIMEOUT, stream.next()).await {
            Ok(Some(chunk)) => chunk.map_err(|e| format!("传输中断（{e}）"))?,
            Ok(None) => break,
            Err(_) => return Err(format!("超过 {} 秒没有数据", STALL_TIMEOUT.as_secs())),
        };
        if state.is_cancelled() {
            return Err(CANCELLED.to_string());
        }
        file.write_all(&chunk).map_err(|e| format!("写盘失败：{e}"))?;
        got += chunk.len() as u64;
        let pct = (total > 0).then(|| got * 100 / total);
        // 首块必发，之后按「跨 1% 或满 100ms」节流
        if last_pct != pct || last_emit.elapsed() >= PROGRESS_INTERVAL {
            last_emit = Instant::now();
            last_pct = pct;
            on_progress(DownloadProgress {
                downloaded: got,
                total,
                attempt,
                candidate: host.to_string(),
                state: "downloading",
            });
        }
    }
    drop(file);
    // 收尾必发一条：否则最后一段（含 100%）可能被节流吃掉
    on_progress(DownloadProgress {
        downloaded: got,
        total,
        attempt,
        candidate: host.to_string(),
        state: "downloading",
    });
    verify_installer(part, got, total, min_bytes)
}

/// 把 total 切成至多 max_conns 个首尾相接的闭区间。纯函数抽出便于单测：
/// 段数被最小段长压住（大文件也不至于切出上百段），余数均摊到前面的段。
fn chunk_plan(total: u64, max_conns: usize) -> Vec<(u64, u64)> {
    if total == 0 {
        return Vec::new();
    }
    let min_side = ((total + MIN_CHUNK_BYTES - 1) / MIN_CHUNK_BYTES) as usize;
    let n = max_conns.min(min_side).max(1);
    let base = total / n as u64;
    let rem = total % n as u64;
    let mut out = Vec::with_capacity(n);
    let mut lo = 0u64;
    for i in 0..n {
        let len = base + if (i as u64) < rem { 1 } else { 0 };
        out.push((lo, lo + len - 1));
        lo += len;
    }
    out
}

/// 拉一段 Range 到 part 文件的对应偏移。`received` 累计本次已收字节，
/// 重试时调用方据此把共享计数减回去，进度条才不会虚高。
async fn download_chunk(
    client: &reqwest::Client,
    url: &str,
    lo: u64,
    hi: u64,
    part: &Path,
    got: &std::sync::atomic::AtomicU64,
    state: &UpdateState,
    received: &mut u64,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use std::io::{Seek, SeekFrom, Write};

    let resp = client
        .get(url)
        .header(reqwest::header::RANGE, format!("bytes={lo}-{hi}"))
        .send()
        .await
        .map_err(|e| format!("连接失败（{e}）"))?;
    if resp.status().as_u16() != 206 {
        return Err(format!("分段请求被拒（{}）", resp.status().as_u16()));
    }
    let expect = hi - lo + 1;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(part)
        .map_err(|e| format!("打开临时文件失败：{e}"))?;
    file.seek(SeekFrom::Start(lo))
        .map_err(|e| format!("定位写入位置失败：{e}"))?;
    let mut stream = resp.bytes_stream();
    loop {
        let chunk = match tokio::time::timeout(STALL_TIMEOUT, stream.next()).await {
            Ok(Some(chunk)) => chunk.map_err(|e| format!("传输中断（{e}）"))?,
            Ok(None) => break,
            Err(_) => return Err(format!("超过 {} 秒没有数据", STALL_TIMEOUT.as_secs())),
        };
        if state.is_cancelled() {
            return Err(CANCELLED.to_string());
        }
        file.write_all(&chunk).map_err(|e| format!("写盘失败：{e}"))?;
        *received += chunk.len() as u64;
        got.fetch_add(chunk.len() as u64, Ordering::SeqCst);
    }
    if *received != expect {
        return Err(format!("分段不完整（{received}/{expect}）"));
    }
    Ok(())
}

/// 多连接分段下载：镜像与 GitHub 同源限速按连接算，单连接只有几十 KB/s，
/// 多开连接是唯一不改托管就能拿到的提速。探测到服务不支持 Range 或文件太小
/// 时返回 Err，调用方无缝回退单连接（download_once）。
///
/// 进度条看到的依然是同一条曲线：各段的已收字节汇进同一个计数器，这里统一节流上报。
#[allow(clippy::too_many_arguments)]
async fn download_parallel(
    client: &reqwest::Client,
    url: &str,
    part: &Path,
    min_bytes: u64,
    attempt: u32,
    host: &str,
    state: &UpdateState,
    on_progress: &mut (impl FnMut(DownloadProgress) + Send),
) -> Result<(), String> {
    use futures_util::StreamExt;

    // 探测：Range 支持与文件总长。206 才是真分段，200 说明服务忽略 Range 全量返回
    let probe = client
        .get(url)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .send()
        .await
        .map_err(|e| format!("连接失败（{e}）"))?;
    if probe.status().as_u16() != 206 {
        return Err(format!(
            "服务不支持分段下载（{}）",
            probe.status().as_u16()
        ));
    }
    let total: u64 = probe
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.rsplit('/').next())
        .and_then(|t| t.parse().ok())
        .ok_or("服务未返回分段总长")?;
    if total < PARALLEL_MIN_BYTES {
        return Err(format!("文件只有 {total} 字节，不值得分段"));
    }
    drop(probe);

    let plan = chunk_plan(total, PARALLEL_CONNECTIONS);
    // 预分配到位，各段只写自己的区间，互不越界
    {
        let file = std::fs::File::create(part).map_err(|e| format!("创建文件失败：{e}"))?;
        file.set_len(total).map_err(|e| format!("预留文件空间失败：{e}"))?;
    }

    let got = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let mut handles = Vec::with_capacity(plan.len());
    for (lo, hi) in plan.iter().copied() {
        let client = client.clone();
        let url = url.to_string();
        let part = part.to_path_buf();
        let got = got.clone();
        let state = state.clone();
        handles.push(tokio::spawn(async move {
            let mut received: u64 = 0;
            for round in 1..=CHUNK_ATTEMPTS {
                if state.is_cancelled() {
                    return Err(CANCELLED.to_string());
                }
                match download_chunk(&client, &url, lo, hi, &part, &got, &state, &mut received)
                    .await
                {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        // 该段没到不回退进度：把已计入共享计数的字节撤回再重试
                        got.fetch_sub(received, Ordering::SeqCst);
                        received = 0;
                        if e == CANCELLED {
                            return Err(e);
                        }
                        if round < CHUNK_ATTEMPTS {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        } else {
                            return Err(e);
                        }
                    }
                }
            }
            unreachable!("分段重试循环必然在内部返回")
        }));
    }

    on_progress(DownloadProgress {
        downloaded: 0,
        total,
        attempt,
        candidate: host.to_string(),
        state: "downloading",
    });

    let mut set = handles.into_iter().collect::<futures_util::stream::FuturesUnordered<_>>();
    loop {
        // 事件驱动 + 100ms 兜底：段完成立刻反映到进度条，纯下载期也有心跳
        let event = tokio::time::timeout(PROGRESS_INTERVAL, set.next()).await;
        if state.is_cancelled() {
            return Err(CANCELLED.to_string());
        }
        match event {
            Ok(Some(joined)) => {
                match joined.map_err(|e| format!("下载任务异常：{e}")) {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => return Err(e),
                    Err(e) => return Err(e),
                }
            }
            Ok(None) => break, // 全部段完成
            Err(_) => {}       // 100ms 到点：只刷进度
        }
        let now = got.load(Ordering::SeqCst);
        on_progress(DownloadProgress {
            downloaded: now,
            total,
            attempt,
            candidate: host.to_string(),
            state: "downloading",
        });
    }

    let got_total = got.load(Ordering::SeqCst);
    if got_total != total {
        return Err(format!("传输不完整（{got_total}/{total} 字节）"));
    }
    verify_installer(part, got_total, total, min_bytes)
}

/// 依次尝试候选地址，返回下载好的安装包路径。
///
/// 每次重试前先发一条 `retrying`：进度条会停在当前高水位而不是被打回 0，
/// 用户看到的是「连接断了，正在重试第 2 次」，而不是进度条自己来回横跳。
async fn fetch_installer(
    client: &reqwest::Client,
    candidates: &[String],
    dir: &Path,
    min_bytes: u64,
    state: &UpdateState,
    mut on_progress: impl FnMut(DownloadProgress) + Send,
) -> Result<PathBuf, String> {
    let path = dir.join(ASSET_NAME);
    let part = dir.join(format!("{ASSET_NAME}{PART_SUFFIX}"));
    let _ = std::fs::remove_file(&part);
    let mut reasons: Vec<String> = Vec::new();
    let mut attempt_no: u32 = 0;

    for candidate in candidates {
        let host = host_of(candidate);
        for attempt in 1..=DOWNLOAD_ATTEMPTS {
            attempt_no += 1;
            crate::selection::log_line(&format!(
                "update: 下载尝试 {attempt}/{DOWNLOAD_ATTEMPTS} ← {candidate}"
            ));
            if attempt_no > 1 {
                on_progress(DownloadProgress {
                    downloaded: 0,
                    total: 0,
                    attempt: attempt_no,
                    candidate: host.clone(),
                    state: "retrying",
                });
            }
            // 先走多连接分段：慢线路上这是数量级的差距。探测到不支持 Range
            // 或文件太小时并行函数自己返回 Err，这里无缝降级回单连接
            let result = match download_parallel(
                client,
                candidate,
                &part,
                min_bytes,
                attempt_no,
                &host,
                state,
                &mut on_progress,
            )
            .await
            {
                Ok(()) => Ok(()),
                Err(e) if e == CANCELLED => Err(e),
                Err(pe) => {
                    crate::selection::log_line(&format!(
                        "update: 分段下载未走通（{pe}），降级单连接 ← {host}"
                    ));
                    let _ = std::fs::remove_file(&part);
                    download_once(
                        client,
                        candidate,
                        &part,
                        min_bytes,
                        attempt_no,
                        &host,
                        state,
                        &mut on_progress,
                    )
                    .await
                }
            };
            match result {
                Ok(()) => {
                    std::fs::rename(&part, &path).map_err(|e| {
                        let _ = std::fs::remove_file(&part);
                        format!("保存安装包失败：{e}")
                    })?;
                    crate::selection::log_line(&format!("update: 下载完成 ← {host}"));
                    return Ok(path);
                }
                Err(e) => {
                    // 半截文件一律删掉：留着只会被误当成可用安装包
                    let _ = std::fs::remove_file(&part);
                    if e == CANCELLED {
                        return Err(e);
                    }
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

/// 下载安装包到临时目录，返回本地路径。进度通过 `update-progress` 推给界面。
///
/// 单飞：同一时刻只允许一个下载。已经有下载在跑时直接返回可读错误，
/// 而不是让第二个下载去截断第一个的文件。
#[tauri::command]
pub async fn download_update(
    app: AppHandle,
    state: tauri::State<'_, UpdateState>,
    url: String,
) -> Result<String, String> {
    // 先把状态克隆成自有句柄：后面要跨 await，不能一直借着 State 的引用
    let state = state.inner().clone();
    if !state.begin() {
        return Err("已经有一个更新在下载中，等它跑完或先取消".into());
    }
    let _guard = BusyGuard(state.clone());

    let dir = std::env::temp_dir().join("suiyi-update");
    std::fs::create_dir_all(&dir).map_err(|e| format!("准备临时目录失败：{e}"))?;
    let client = download_client()?;
    let candidates = download_candidates(&url);

    let emit_state = state.clone();
    let path = fetch_installer(
        &client,
        &candidates,
        &dir,
        MIN_INSTALLER_BYTES,
        &state,
        move |p| {
            // 取消之后别再往界面推进度，免得界面在「已取消」后又跳一下
            if emit_state.is_cancelled() {
                return;
            }
            let _ = app.emit("update-progress", p);
        },
    )
    .await?;
    Ok(path.to_string_lossy().to_string())
}

/// 用户点了「取消下载」：置位后由下载循环自己收尾，这里立刻返回。
/// 返回是否有下载被取消，界面不需要据此改状态（它自己知道是用户点的）。
#[tauri::command]
pub fn cancel_update(state: tauri::State<'_, UpdateState>) -> bool {
    state.request_cancel()
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
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};

    /// 只带 Content-Length 的最小 HTTP 响应，body 由调用方决定。
    /// 和 translator.rs 的假服务端一样用 std 的 TcpListener，不引测试依赖。
    ///
    /// `declared` 与 `send` 故意可以不一样：声明 32KB 却只发 8KB 就是在模拟中途断线。
    ///
    /// 支持多连接与 Range：并行下载的探测、六段并发、串行重试会先后打好几个
    /// 连接进来，每个连接各开一个线程按需响应；带 Range 头回 206 与 Content-Range，
    /// 不带就回 200 全量——与真实 CDN 的行为一致。
    fn serve_once(declared: usize, send: usize, chunk: usize, delay: Duration) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            // 测试进程短命，accept 循环随进程一起消亡，不需要停机协议
            while let Ok((sock, _)) = listener.accept() {
                std::thread::spawn(move || {
                    let mut sock = sock;
                    let mut req = vec![0u8; 8192];
                    let _ = sock.read(&mut req);
                    let text = String::from_utf8_lossy(&req);
                    let range = text
                        .lines()
                        .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                        .and_then(|l| {
                            let spec = l.split_once(':')?.1.trim().strip_prefix("bytes=")?;
                            let (lo, hi) = spec.trim().split_once('-')?;
                            Some((
                                lo.trim().parse::<usize>().ok()?,
                                hi.trim().parse::<usize>().ok()?,
                            ))
                        });
                    // 一段连续字节：从 from 开始发 count 个，[0,1] 恒为 MZ 头
                    let emit = |sock: &mut TcpStream, from: usize, count: usize| -> bool {
                        let mut sent = 0usize;
                        while sent < count {
                            let n = chunk.min(count - sent);
                            let mut buf = vec![0u8; n];
                            if from + sent == 0 && n >= 2 {
                                buf[0] = b'M';
                                buf[1] = b'Z';
                            }
                            if sock.write_all(&buf).is_err() {
                                return false;
                            }
                            let _ = sock.flush();
                            sent += n;
                            if !delay.is_zero() {
                                std::thread::sleep(delay);
                            }
                        }
                        true
                    };
                    match range {
                        Some((lo, hi)) => {
                            let hi = hi.min(send.saturating_sub(1));
                            if lo >= send || hi < lo {
                                return;
                            }
                            let len = hi - lo + 1;
                            let head = format!(
                                "HTTP/1.1 206 Partial Content\r\n\
                                 Content-Type: application/octet-stream\r\n\
                                 Content-Range: bytes {lo}-{hi}/{declared}\r\n\
                                 Content-Length: {len}\r\nConnection: close\r\n\r\n"
                            );
                            if sock.write_all(head.as_bytes()).is_err() {
                                return;
                            }
                            let _ = sock.flush();
                            emit(&mut sock, lo, len);
                        }
                        None => {
                            let head = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
                                 Content-Length: {declared}\r\nConnection: close\r\n\r\n"
                            );
                            let _ = sock.write_all(head.as_bytes());
                            let _ = sock.flush();
                            emit(&mut sock, 0, send);
                        }
                    }
                });
            }
        });
        format!("http://{addr}/SuiYi-Setup-x64.exe")
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("suiyi-update-test-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn verify_installer_rejects_truncated_file() {
        let dir = temp_dir("verify");
        let path = dir.join("a.exe");
        std::fs::write(&path, b"MZxxxx").unwrap();
        let err = verify_installer(&path, 6, 100, 4).unwrap_err();
        assert!(err.contains("传输不完整"), "{err}");

        // 体积对得上、但明显不是完整安装包
        let err = verify_installer(&path, 6, 6, 500 * 1024).unwrap_err();
        assert!(err.contains("文件不完整"), "{err}");

        // 不是 PE 文件（比如把某个错误页当成了安装包）
        std::fs::write(&path, b"<!DOCTYPE html>").unwrap();
        let err = verify_installer(&path, 15, 15, 4).unwrap_err();
        assert!(err.contains("可执行文件"), "{err}");

        std::fs::write(&path, b"MZ\x90\x00payload").unwrap();
        assert!(verify_installer(&path, 11, 11, 4).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn progress_never_goes_backwards_within_an_attempt() {
        let dir = temp_dir("monotonic");
        let url = serve_once(64 * 1024, 64 * 1024, 4 * 1024, Duration::ZERO);
        let state = UpdateState::default();
        let mut seen: Vec<(u64, u64, u32, String)> = Vec::new();
        let client = download_client().unwrap();
        let path = fetch_installer(&client, &[url], &dir, 64, &state, |p| {
            seen.push((p.downloaded, p.total, p.attempt, p.state.to_string()));
        })
        .await
        .unwrap();

        assert!(path.exists(), "成功后应该有正式安装包：{path:?}");
        assert!(!dir.join(format!("{ASSET_NAME}{PART_SUFFIX}")).exists());
        assert!(seen.len() >= 2, "至少要有首末两条进度：{seen:?}");
        assert_eq!(seen[0].0, 0, "第一条先报总量与 0%，别让界面先摆一个满格：{seen:?}");
        assert_eq!(seen[0].1, 64 * 1024, "第一条就要带上总量：{seen:?}");
        let downloaded: Vec<u64> = seen.iter().map(|s| s.0).collect();
        assert!(
            downloaded.windows(2).all(|w| w[0] <= w[1]),
            "同一次尝试内进度只能往前：{downloaded:?}"
        );
        assert_eq!(*downloaded.last().unwrap(), 64 * 1024, "收尾必发 100%");
        assert!(seen.iter().all(|s| s.3 == "downloading"));
        assert!(seen.iter().all(|s| s.2 == 1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 中途断线：重试前先发一条 retrying，且不留下半截文件
    #[tokio::test]
    async fn retries_after_dropped_connection_and_marks_the_retry() {
        let dir = temp_dir("retry");
        // 第一个地址声明 32KB 却只发 8KB 就断，第二个地址发完整包
        let broken = serve_once(32 * 1024, 8 * 1024, 8 * 1024, Duration::ZERO);
        let ok = serve_once(32 * 1024, 32 * 1024, 32 * 1024, Duration::ZERO);
        let state = UpdateState::default();
        let mut states: Vec<String> = Vec::new();
        let mut attempts: Vec<u32> = Vec::new();
        let client = download_client().unwrap();
        let path = fetch_installer(&client, &[broken, ok], &dir, 1024, &state, |p| {
            states.push(p.state.to_string());
            attempts.push(p.attempt);
        })
        .await
        .unwrap();

        assert!(path.exists());
        assert!(
            states.iter().any(|s| s == "retrying"),
            "重试要明确告诉界面，别让进度条自己往回跳：{states:?}"
        );
        assert_eq!(attempts.first().copied(), Some(1));
        assert!(
            attempts.last().copied().unwrap_or(0) > 1,
            "成功那一次不该是第一次尝试：{attempts:?}"
        );
        assert!(
            attempts.windows(2).all(|w| w[0] <= w[1]),
            "尝试编号只能往前走：{attempts:?}"
        );
        assert!(!dir.join(format!("{ASSET_NAME}{PART_SUFFIX}")).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 取消：立刻停下、返回可读原因、不留半截文件
    #[tokio::test]
    async fn cancel_stops_the_download_and_leaves_no_partial_file() {
        let dir = temp_dir("cancel");
        // 一次只发 1KB 且带间隔，保证取消有机会在下载中途生效
        let url = serve_once(256 * 1024, 256 * 1024, 1024, Duration::from_millis(5));
        let state = UpdateState::default();
        state.begin();
        let cancelling = state.clone();
        let client = download_client().unwrap();
        let err = fetch_installer(&client, &[url], &dir, 1024, &state, move |p| {
            // 收到第一块就当成用户点了取消
            if p.downloaded > 0 {
                cancelling.request_cancel();
            }
        })
        .await
        .unwrap_err();

        assert_eq!(err, CANCELLED, "取消要有自己的原因，不能被当成故障");
        assert!(!dir.join(ASSET_NAME).exists(), "取消后不该留下安装包");
        assert!(
            !dir.join(format!("{ASSET_NAME}{PART_SUFFIX}")).exists(),
            "取消后不该留下半截文件"
        );
        assert!(state.is_cancelled(), "取消位要到收尾才清");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn second_download_is_blocked_while_one_is_running() {
        let state = UpdateState::default();
        assert!(state.begin(), "空着的时候应该抢得到");
        assert!(!state.begin(), "已经在下载时第二次要被挡住");
        assert!(state.request_cancel(), "下载中取消要生效");
        assert!(state.is_cancelled());
        state.finish();
        assert!(!state.is_cancelled(), "收尾要把取消位清掉，别影响下一次");
        assert!(state.begin(), "跑完之后又能抢到");
        state.finish();

        // 没有下载在跑时取消是空操作，不该留下「下一轮立刻取消」的脏标志
        let idle = UpdateState::default();
        assert!(!idle.request_cancel());
        assert!(!idle.is_cancelled());
    }

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

    /// 分段计划：段数被连接数与最小段长共同压住，区间首尾相接且不重不漏
    #[test]
    fn chunk_plan_covers_exactly_and_respects_limits() {
        let plan = super::chunk_plan(23 * 1024 * 1024, super::PARALLEL_CONNECTIONS);
        assert_eq!(plan.len(), super::PARALLEL_CONNECTIONS, "大文件切满连接数");
        assert_eq!(plan.first().unwrap().0, 0, "从 0 开始");
        assert_eq!(plan.last().unwrap().1, 23 * 1024 * 1024 - 1, "到 total-1 结束");
        for w in plan.windows(2) {
            assert_eq!(w[0].1 + 1, w[1].0, "相邻段首尾相接：{plan:?}");
        }
        for (lo, hi) in &plan {
            assert!(hi - lo + 1 >= super::MIN_CHUNK_BYTES, "段长不低于最小段：{plan:?}");
        }

        // 小文件只切一段（等于退回单连接，调用方还有探测兜底）
        let small = super::chunk_plan(super::MIN_CHUNK_BYTES, super::PARALLEL_CONNECTIONS);
        assert_eq!(small, vec![(0, super::MIN_CHUNK_BYTES - 1)]);

        // total 小于最小段：同样一段覆盖
        let tiny = super::chunk_plan(1024, super::PARALLEL_CONNECTIONS);
        assert_eq!(tiny, vec![(0, 1023)]);

        // 10KB 连一个最小段都凑不齐：只切一段，退化为顺序下载
        let micro = super::chunk_plan(10_000, super::PARALLEL_CONNECTIONS);
        assert_eq!(micro, vec![(0, 9_999)]);

        // 不能整除时余数摊给前面的段，区间仍然不重不漏
        // （1MB 出头才能切出三段：MIN_CHUNK_BYTES*2+1024 的最小段数是 3）
        let odd = super::chunk_plan(super::MIN_CHUNK_BYTES * 2 + 1024, 3);
        assert_eq!(odd.len(), 3);
        assert_eq!(odd.first().unwrap().0, 0);
        assert_eq!(odd.last().unwrap().1, super::MIN_CHUNK_BYTES * 2 + 1023);
        let covered: u64 = odd.iter().map(|(lo, hi)| hi - lo + 1).sum();
        assert_eq!(covered, super::MIN_CHUNK_BYTES * 2 + 1024, "总覆盖量必须等于文件大小");

        // 连接数比最小段数还多时被段长压住：2MB 减一字节的最小段数是 2
        let capped = super::chunk_plan(super::MIN_CHUNK_BYTES * 2 - 1, super::PARALLEL_CONNECTIONS);
        assert_eq!(capped.len(), 2, "2MB 减一字节只够两个最小段：{capped:?}");
    }

    /// 分段并发的完整链路：3MB 大文件探测到 206 后走六段并发，
    /// 进度首条带总量、单调推进、收尾 100%，落盘字节与声明一致
    #[tokio::test]
    async fn parallel_download_succeeds_with_monotonic_progress() {
        let dir = temp_dir("parallel");
        let size = 3 * 1024 * 1024;
        let url = serve_once(size, size, 256 * 1024, Duration::ZERO);
        let state = UpdateState::default();
        let mut seen: Vec<(u64, u64)> = Vec::new();
        let client = download_client().unwrap();
        let path = fetch_installer(&client, &[url], &dir, 1024, &state, |p| {
            seen.push((p.downloaded, p.total));
        })
        .await
        .unwrap();

        assert!(path.exists(), "成功后应该有正式安装包：{path:?}");
        assert_eq!(
            std::fs::read(&path).unwrap().len(),
            size,
            "落盘字节必须与声明一致"
        );
        assert_eq!(
            seen.first().unwrap().1,
            size as u64,
            "第一条就要带上总量：{seen:?}"
        );
        let downloaded: Vec<u64> = seen.iter().map(|s| s.0).collect();
        assert!(
            downloaded.windows(2).all(|w| w[0] <= w[1]),
            "分段汇聚的进度也只能往前：{downloaded:?}"
        );
        assert_eq!(*downloaded.last().unwrap(), size as u64, "收尾必发 100%");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
