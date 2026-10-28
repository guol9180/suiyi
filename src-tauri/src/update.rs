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
/// npm 包名：安装包随发版发布到 npm，npmmirror（阿里云 CDN）自动同步。
/// GitHub Pages 与 npmjs 同源（Fastly）被限速到 ~60KB/s 的线路封顶后，
/// 这是免费方案里国内最快的一条。包内只有一个文件 package/SuiYi-Setup-x64.exe
const NPM_PACKAGE: &str = "suiyi-setup-x64";
/// npmmirror 的 registry 根地址（国内 CDN，官方 registry 的同步镜像）
const NPM_MIRROR_REGISTRY: &str = "https://registry.npmmirror.com";
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

/// 一个可尝试的下载源。exe 直链下载完就是安装包；npm 源要下载 tgz 后解出安装包
#[derive(Debug, Clone)]
enum DownloadSource {
    Exe(String),
    /// registry 根地址（不含包名）：取 /<pkg>/latest 的 JSON，再下载 dist.tarball 解包
    NpmPackage { registry: String },
}

/// 候选源顺序：npm 镜像（阿里云 CDN，主力）→ Pages exe → GitHub release exe。
/// 后两者是 npm 源不可用时的兜底（同一文件的另两份拷贝）
fn download_candidates(asset_url: &str) -> Vec<DownloadSource> {
    vec![
        DownloadSource::NpmPackage { registry: NPM_MIRROR_REGISTRY.to_string() },
        DownloadSource::Exe(format!("{MIRROR_BASE}{ASSET_NAME}")),
        DownloadSource::Exe(asset_url.to_string()),
    ]
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
/// `expect_pe` 标记载荷是不是 PE 安装包：npm 的 tgz 载荷只验完整性，跳过 MZ 检查。
#[allow(clippy::too_many_arguments)]
async fn download_once(
    client: &reqwest::Client,
    url: &str,
    part: &Path,
    min_bytes: u64,
    attempt: u32,
    host: &str,
    state: &UpdateState,
    expect_pe: bool,
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
    if expect_pe {
        verify_installer(part, got, total, min_bytes)
    } else if total > 0 && got != total {
        Err(format!("传输不完整（{got}/{total} 字节）"))
    } else {
        Ok(())
    }
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
    expect_pe: bool,
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
    if !expect_pe {
        return Ok(());
    }
    verify_installer(part, got_total, total, min_bytes)
}

/// 从 npm registry 拉安装包：/<pkg>/latest 的 JSON 给出 tarball 地址，
/// tarball 用与 exe 相同的分段并发下载，解包走系统自带的 tar（Win10 1803+
/// 自带 bsdtar），解出 package/SuiYi-Setup-x64.exe 挪到 part 位，
/// 交给统一的完整性校验与改名。任何一步失败都由上层降级到 exe 直链候选。
#[allow(clippy::too_many_arguments)]
async fn fetch_from_npm(
    client: &reqwest::Client,
    registry: &str,
    part: &Path,
    dir: &Path,
    min_bytes: u64,
    attempt: u32,
    host: &str,
    state: &UpdateState,
    on_progress: &mut (impl FnMut(DownloadProgress) + Send),
) -> Result<(), String> {
    use serde_json::Value;

    let meta_url = format!("{registry}/{NPM_PACKAGE}/latest");
    let resp = client
        .get(&meta_url)
        .send()
        .await
        .map_err(|e| format!("连接失败（{e}）"))?;
    if !resp.status().is_success() {
        return Err(format!("包信息请求失败（{}）", resp.status().as_u16()));
    }
    let meta: Value = resp
        .json()
        .await
        .map_err(|e| format!("包信息解析失败：{e}"))?;
    let tarball = meta
        .pointer("/dist/tarball")
        .and_then(|v| v.as_str())
        .ok_or("包信息里没有 tarball 地址")?
        .to_string();

    let tgz = dir.join(format!("{NPM_PACKAGE}.tgz"));
    let tgz_part = dir.join(format!("{NPM_PACKAGE}.tgz{PART_SUFFIX}"));
    let _ = std::fs::remove_file(&tgz_part);
    let _ = std::fs::remove_file(&tgz);

    // tarball 是 gzip 压缩包不是 PE：跳过 MZ 校验，只验完整性
    let dl = match download_parallel(
        client,
        &tarball,
        &tgz_part,
        0,
        attempt,
        host,
        state,
        false,
        on_progress,
    )
    .await
    {
        Ok(()) => Ok(()),
        Err(e) => {
            crate::selection::log_line(&format!(
                "update: tarball 分段未走通（{e}），降级单连接 ← {host}"
            ));
            let _ = std::fs::remove_file(&tgz_part);
            download_once(
                client,
                &tarball,
                &tgz_part,
                0,
                attempt,
                host,
                state,
                false,
                on_progress,
            )
            .await
        }
    };
    dl?;
    std::fs::rename(&tgz_part, &tgz).map_err(|e| format!("保存压缩包失败：{e}"))?;

    let out = std::process::Command::new("tar")
        .args(["-xzf"])
        .arg(&tgz)
        .arg("-C")
        .arg(dir)
        .output()
        .map_err(|e| format!("系统缺少 tar 工具（Win10 1803+ 自带）：{e}"))?;
    if !out.status.success() {
        return Err(format!(
            "解包失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }

    let packed = dir.join("package").join(ASSET_NAME);
    std::fs::rename(&packed, part).map_err(|e| format!("提取安装包失败：{e}"))?;
    let _ = std::fs::remove_file(&tgz);
    let size = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    verify_installer(part, size, 0, min_bytes)
}

/// 依次尝试候选源，返回下载好的安装包路径。
///
/// 每次重试前先发一条 `retrying`：进度条会停在当前高水位而不是被打回 0，
/// 用户看到的是「连接断了，正在重试第 2 次」，而不是进度条自己来回横跳。
async fn fetch_installer(
    client: &reqwest::Client,
    candidates: &[DownloadSource],
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
        let host = match candidate {
            DownloadSource::Exe(url) => host_of(url),
            DownloadSource::NpmPackage { registry } => host_of(registry),
        };
        for attempt in 1..=DOWNLOAD_ATTEMPTS {
            attempt_no += 1;
            crate::selection::log_line(&format!(
                "update: 下载尝试 {attempt}/{DOWNLOAD_ATTEMPTS} ← {candidate:?}"
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
            let result = match candidate {
                DownloadSource::NpmPackage { registry } => {
                    fetch_from_npm(
                        client,
                        registry,
                        &part,
                        dir,
                        min_bytes,
                        attempt_no,
                        &host,
                        state,
                        &mut on_progress,
                    )
                    .await
                }
                DownloadSource::Exe(url) => {
                    match download_parallel(
                        client,
                        url,
                        &part,
                        min_bytes,
                        attempt_no,
                        &host,
                        state,
                        true,
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
                                url,
                                &part,
                                min_bytes,
                                attempt_no,
                                &host,
                                state,
                                true,
                                &mut on_progress,
                            )
                            .await
                        }
                    }
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

    /// 与 serve_once 同一协议（200 全量、206 分段），但发送调用方给的真实字节：
    /// npm 链路测试用真 tgz，解包环节才有意义
    fn serve_bytes(content: Vec<u8>, chunk: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let content = Arc::new(content);
        std::thread::spawn(move || {
            while let Ok((sock, _)) = listener.accept() {
                let content = content.clone();
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
                    let send = content.len();
                    let (head, from, count) = match range {
                        Some((lo, hi)) => {
                            let hi = hi.min(send.saturating_sub(1));
                            if lo >= send || hi < lo {
                                return;
                            }
                            let len = hi - lo + 1;
                            (
                                format!(
                                    "HTTP/1.1 206 Partial Content\r\n\
                                     Content-Type: application/octet-stream\r\n\
                                     Content-Range: bytes {lo}-{hi}/{send}\r\n\
                                     Content-Length: {len}\r\nConnection: close\r\n\r\n"
                                ),
                                lo,
                                len,
                            )
                        }
                        None => (
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
                                 Content-Length: {send}\r\nConnection: close\r\n\r\n"
                            ),
                            0,
                            send,
                        ),
                    };
                    if sock.write_all(head.as_bytes()).is_err() {
                        return;
                    }
                    let mut sent = 0usize;
                    while sent < count {
                        let n = chunk.min(count - sent);
                        if sock
                            .write_all(&content[from + sent..from + sent + n])
                            .is_err()
                        {
                            return;
                        }
                        sent += n;
                    }
                });
            }
        });
        format!("http://{addr}/fixture")
    }

    /// 最小 registry JSON 服务：任何路径都回同一段 JSON（npm 链路的 /<pkg>/latest）
    fn serve_json(body: String) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let body = Arc::new(body);
        std::thread::spawn(move || {
            while let Ok((mut sock, _)) = listener.accept() {
                let body = body.clone();
                // 必须先读掉请求再回包再关：未读数据会让 close 发 RST 把响应冲掉
                let mut req = vec![0u8; 8192];
                let _ = sock.read(&mut req);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = sock.write_all(head.as_bytes());
                let _ = sock.write_all(body.as_bytes());
                let _ = sock.flush();
            }
        });
        format!("http://{addr}")
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
        let path = fetch_installer(
            &client,
            &[super::DownloadSource::Exe(url)],
            &dir,
            64,
            &state,
            |p| {
                seen.push((p.downloaded, p.total, p.attempt, p.state.to_string()));
            },
        )
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
        let path = fetch_installer(
            &client,
            &[super::DownloadSource::Exe(broken), super::DownloadSource::Exe(ok)],
            &dir,
            1024,
            &state,
            |p| {
                states.push(p.state.to_string());
                attempts.push(p.attempt);
            },
        )
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
        let err = fetch_installer(
            &client,
            &[super::DownloadSource::Exe(url)],
            &dir,
            1024,
            &state,
            move |p| {
                // 收到第一块就当成用户点了取消
                if p.downloaded > 0 {
                    cancelling.request_cancel();
                }
            },
        )
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

    /// 下载源顺序：npm 镜像（阿里云 CDN）主力，Pages exe 与 GitHub exe 兜底
    #[test]
    fn download_candidates_prefers_the_npm_mirror() {
        let sources = super::download_candidates(
            "https://github.com/guol9180/suiyi/releases/download/v0.9.0/SuiYi-Setup-x64.exe",
        );
        assert_eq!(sources.len(), 3, "npm 镜像 + Pages exe + GitHub exe：{sources:?}");
        assert!(
            matches!(
                &sources[0],
                super::DownloadSource::NpmPackage { registry }
                    if registry == super::NPM_MIRROR_REGISTRY
            ),
            "第一顺位是 npmmirror：{sources:?}"
        );
        assert!(
            matches!(
                &sources[1],
                super::DownloadSource::Exe(u)
                    if *u == format!("{}{}", super::MIRROR_BASE, super::ASSET_NAME)
            ),
            "第二顺位是 Pages 镜像：{sources:?}"
        );
        assert!(
            matches!(&sources[2], super::DownloadSource::Exe(u) if u.starts_with("https://github.com/")),
            "第三顺位是 GitHub 原始地址：{sources:?}"
        );
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
        let path = fetch_installer(
            &client,
            &[super::DownloadSource::Exe(url)],
            &dir,
            1024,
            &state,
            |p| {
                seen.push((p.downloaded, p.total));
            },
        )
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

    /// npm 链路完整走一遍：registry JSON 给出 tarball 地址，下载 tgz 后
    /// 用系统 tar 解出安装包，进度条走 tarball 的量，落盘字节与 fixture 一致
    #[tokio::test]
    async fn npm_source_downloads_tarball_and_extracts_installer() {
        let dir = temp_dir("npm");
        // 现场打一个真 tgz：package/SuiYi-Setup-x64.exe（MZ 头）+ package.json
        let size = 64 * 1024;
        let pkgroot = dir.join("pkgroot");
        let pkgpkg = pkgroot.join("package");
        std::fs::create_dir_all(&pkgpkg).unwrap();
        let mut exe = vec![0u8; size];
        exe[0] = b'M';
        exe[1] = b'Z';
        std::fs::write(pkgpkg.join(ASSET_NAME), &exe).unwrap();
        std::fs::write(
            pkgpkg.join("package.json"),
            format!("{{\"name\":\"{NPM_PACKAGE}\",\"version\":\"0.0.0-test\"}}"),
        )
        .unwrap();
        let tgz = dir.join("fixture.tgz");
        let out = std::process::Command::new("tar")
            .args(["-czf"])
            .arg(&tgz)
            .arg("-C")
            .arg(&pkgroot)
            .arg("package")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "测试机要有 tar（Win10 1803+ 自带）：{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let tgz_len = std::fs::metadata(&tgz).unwrap().len() as usize;

        let tarball_url = serve_bytes(std::fs::read(&tgz).unwrap(), 8 * 1024);
        let body = format!(
            "{{\"name\":\"{NPM_PACKAGE}\",\"version\":\"0.0.0-test\",\
             \"dist\":{{\"tarball\":\"{tarball_url}\"}}}}"
        );
        let registry = serve_json(body);

        let state = UpdateState::default();
        let mut seen: Vec<(u64, u64)> = Vec::new();
        let client = download_client().unwrap();
        let path = fetch_installer(
            &client,
            &[super::DownloadSource::NpmPackage { registry }],
            &dir,
            1024,
            &state,
            |p| {
                seen.push((p.downloaded, p.total));
            },
        )
        .await
        .unwrap();

        assert!(path.exists(), "成功后应该有正式安装包：{path:?}");
        assert_eq!(
            std::fs::read(&path).unwrap().len(),
            size,
            "解出的安装包字节必须与 fixture 一致"
        );
        assert_eq!(
            seen.first().unwrap().1,
            tgz_len as u64,
            "进度条的总量是 tarball 的声明大小：{seen:?}"
        );
        assert_eq!(seen.last().unwrap().0, tgz_len as u64, "收尾 100%");
        assert!(!dir.join(format!("{NPM_PACKAGE}.tgz")).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
