/**
 * 关于页（设计稿第 8 节）。
 *
 * 放三件对用户真正有用的事：这是谁、我的数据在哪、出问题怎么把线索给我。
 * 不写「感谢使用」这类空话。本项目许可是 MIT（见仓库 LICENSE），
 * 这里只列实际用到的第三方组件与它们的许可。
 */
import { useEffect, useState } from "react";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  appPaths,
  getSettings,
  hotkeyStatus,
  tailLog,
  type AppPaths,
  type HotkeyStatus,
} from "../api";
import {
  cancelUpdateDownload,
  checkForUpdate,
  dismissUpdateError,
  installDownloadedUpdate,
  setAboutVisible,
  startUpdateDownload,
  useUpdateSnapshot,
} from "../updateState";
import { Icon } from "../components/Icon";
import { formatAccel } from "../hotkeySuggest";
import "./About.css";

const LINKS: Array<{ label: string; hint: string; url: string }> = [
  { label: "下载页", hint: "suiyi.imhgl.com", url: "https://suiyi.imhgl.com/" },
  { label: "GitHub 源码", hint: "guol9180/suiyi", url: "https://github.com/guol9180/suiyi" },
  {
    label: "更新日志",
    hint: "全部版本与变更",
    url: "https://github.com/guol9180/suiyi/releases",
  },
];

const DOWNLOAD_PAGE = "https://suiyi.imhgl.com/";

export default function AboutPage({ forceCheckSignal = 0 }: { forceCheckSignal?: number } = {}) {
  /** 更新流程的状态在 updateState 里，这里只是它的一个视图 */
  const upd = useUpdateSnapshot();
  const [paths, setPaths] = useState<AppPaths | null>(null);
  const [hotkeys, setHotkeys] = useState<HotkeyStatus[]>([]);
  const [notice, setNotice] = useState("");
  /** 下载中（含重试）这一个阶段在两个地方用到，提一嘴省得写两遍 */
  const downloading = upd.phase === "downloading" || upd.phase === "retrying";

  useEffect(() => {
    void appPaths().then(setPaths).catch(() => setPaths(null));
    void hotkeyStatus().then(setHotkeys).catch(() => setHotkeys([]));
    // 启动时已经查过一次，这里只在用户开着自动检查时顺手再对一次
    void getSettings()
      .then((s) => {
        if (s.autoCheckUpdate) void checkForUpdate();
      })
      .catch(() => {});
  }, []);

  // 关于页开着的时候下载完就直接拉安装向导；切走了就停下等用户点，
  // 免得用户在翻翻译页时应用突然退出
  useEffect(() => {
    setAboutVisible(true);
    return () => setAboutVisible(false);
  }, []);

  // 托盘菜单点「检查更新」：不管自动检查开没开，都当场查一次
  useEffect(() => {
    if (forceCheckSignal > 0) void checkForUpdate();
  }, [forceCheckSignal]);

  /** 把版本、系统、路径、热键状态与日志尾部拼成一段可粘贴的文本 */
  async function copyDiagnostics() {
    const log = await tailLog(40).catch(() => "");
    const lines = [
      `随译 SuiYi v${__APP_VERSION__}`,
      `系统：${navigator.userAgent}`,
      `配置目录：${paths?.configDir ?? "（读不到）"}`,
      `日志文件：${paths?.logFile ?? "（读不到）"}`,
      `插件目录：${paths?.pluginsDir ?? "（读不到）"}`,
      "",
      "热键：",
      ...hotkeys.map(
        (h) => `  ${h.label} ${formatAccel(h.accelerator)} ${h.registered ? "已注册" : `未注册（${h.error ?? "原因未知"}）`}`,
      ),
      "",
      "日志尾部：",
      log || "（暂无日志）",
    ];
    try {
      await navigator.clipboard.writeText(lines.join("\n"));
      setNotice("诊断信息已复制");
    } catch (e) {
      setNotice(`复制失败：${e}`);
    }
  }

  async function reveal(path: string | undefined) {
    if (!path) return;
    try {
      await revealItemInDir(path);
    } catch (e) {
      setNotice(`打开失败：${e}`);
    }
  }

  return (
    <div className="panel about">
      <div className="card">
        <div className="ab-brand">
          <svg className="ab-mark" viewBox="0 0 1024 1024" role="img" aria-label="随译 SuiYi">
            <defs>
              <linearGradient id="ab-bubble" gradientUnits="userSpaceOnUse" x1="111" y1="420" x2="968" y2="420">
                <stop offset="0" stopColor="#855CFF" />
                <stop offset=".5" stopColor="#4E74FE" />
                <stop offset="1" stopColor="#3BA6FF" />
              </linearGradient>
              <mask id="ab-gap" maskUnits="userSpaceOnUse" x="0" y="0" width="1024" height="1024">
                <rect width="1024" height="1024" fill="#fff" />
                <g fill="#000" stroke="#000" strokeWidth="20" strokeLinejoin="round">
                  <rect x="111" y="107" width="857" height="677" rx="106" />
                  <path d="M339.5 752L339.5 890Q339.5 935 383 900L533 782Z" />
                </g>
              </mask>
            </defs>
            <rect x="57" y="179" width="857" height="677" rx="106" fill="#C9D6FF" mask="url(#ab-gap)" />
            <g fill="url(#ab-bubble)">
              <rect x="111" y="107" width="857" height="677" rx="106" />
              <path d="M339.5 752L339.5 890Q339.5 935 383 900L533 782Z" />
            </g>
            <path
              transform="translate(254.1 88.1) scale(0.4475)"
              fill="#FFFFFF"
              d="M166.9 1264.8L138.2 1152.2L158.7 1113.2L334.8 967.8C341 984.2 348.5 1002.7 357.4 1023.1C366.3 1043.6 374.4 1059.3 382 1070.2C339.6 1105.1 305.3 1133.9 279 1156.8C252.8 1179.6 232.3 1197.7 217.6 1211C202.9 1224.4 191.8 1235.1 184.3 1243.3C176.8 1251.5 171 1258.7 166.9 1264.8ZM39.9 630.9L236.5 630.9L236.5 748.7L39.9 748.7ZM357.4 359.6L862.2 359.6L862.2 465.1L357.4 465.1ZM827.4 359.6L848.9 359.6L869.4 354.5L948.2 397.5C921.6 452.8 887.8 502.8 846.8 547.5C805.9 592.2 759.6 632.3 708.1 667.8C656.6 703.3 601.6 734 543.2 760C484.9 785.9 425.3 807.4 364.5 824.5C360.4 814.2 354.8 802.6 347.6 789.7C340.5 776.7 332.8 763.9 324.6 751.3C316.4 738.6 308.2 728.2 300 720C357.4 707.1 413.2 689.8 467.5 668.3C521.7 646.8 572.4 621.4 619.5 592C666.6 562.7 707.9 529.9 743.4 493.7C778.9 457.5 806.9 419 827.4 378ZM519.2 412.8C549.2 460.6 588.3 503.6 636.4 541.9C684.5 580.1 740 612.7 802.8 639.6C865.6 666.6 933.2 686.9 1005.6 700.6C997.4 708.8 988.5 719 978.9 731.3C969.4 743.6 960.3 756.2 951.8 769.2C943.3 782.2 935.9 793.8 929.8 804C854.7 785.6 785.4 760 721.9 727.2C658.4 694.4 601.6 654.7 551.4 607.9C501.2 561.1 458.8 508.1 423.9 448.7ZM411.6 842.9L911.4 842.9L911.4 949.4L411.6 949.4ZM354.3 1026.2L980 1026.2L980 1134.8L354.3 1134.8ZM594.9 762L715.8 762L715.8 1280.2L594.9 1280.2ZM166.9 1264.8C163.5 1255.9 158 1246 150.5 1235.1C143 1224.2 135 1213.3 126.5 1202.3C117.9 1191.4 110.6 1183.2 104.4 1177.8C115.4 1168.2 126.8 1153.7 138.8 1134.2C150.7 1114.8 156.7 1091.4 156.7 1064.1L156.7 630.9L275.5 630.9L275.5 1130.7C275.5 1130.7 271.9 1133.7 264.7 1139.9C257.5 1146 248.5 1154.4 237.6 1165C226.6 1175.5 215.9 1186.6 205.3 1198.2C194.7 1209.8 185.7 1221.6 178.2 1233.6C170.7 1245.5 166.9 1255.9 166.9 1264.8Z"
            />
            <path
              fill="#FFFFFF"
              d="M315 196C316.9 246.4 327.6 257.1 378 259C327.6 260.9 316.9 271.6 315 322C313.1 271.6 302.4 260.9 252 259C302.4 257.1 313.1 246.4 315 196Z"
            />
            <circle cx="776" cy="559" r="111" fill="#FFFFFF" />
            <g fill="#2B52E0" fillRule="evenodd">
              <path d="M695 620L741 494L769 494L815 620ZM755 515.9L772.6 564L737.4 564ZM730.9 582L779.1 582L793 620L717 620Z" />
              <path d="M823 494L845 494L845 620L823 620Z" />
            </g>
          </svg>
          <div>
            <div className="ab-name">随译 SuiYi</div>
            <div className="ab-ver">v{__APP_VERSION__}</div>
            <div className="ab-tag">Windows 桌面翻译工具：划词、截图、输入框三个入口，译文来自你自己配置的服务。</div>
          </div>
        </div>

        <div className="ab-links">
          {LINKS.map((l) => (
            <button key={l.label} className="ab-link" onClick={() => void openUrl(l.url)}>
              <span className="ab-link-name">{l.label}</span>
              <span className="ab-link-hint">{l.hint}</span>
              <Icon name="arrow-right" size="sm" />
            </button>
          ))}
          <button
            className="ab-link"
            disabled={upd.phase === "checking"}
            onClick={() => void checkForUpdate()}
          >
            <span className="ab-link-name">获取更新</span>
            <span className="ab-link-hint">
              {upd.phase === "checking" ? "正在检查…" : `当前 v${__APP_VERSION__}`}
            </span>
            <Icon name="refresh" size="sm" />
          </button>
        </div>
      </div>

      <div className="card">
        <div className="card-head">
          <b>更新</b>
          <span className="m">从 GitHub Releases 读取最新版本</span>
        </div>
        <div className="ab-upd">
          <span className="ab-upd-cur">当前版本 v{__APP_VERSION__}</span>
          {upd.phase === "available" && upd.info && (
            <span className="chip acc mini">发现 v{upd.info.latest}</span>
          )}
          {upd.checkedAt > 0 && upd.phase !== "checking" && upd.info && !upd.info.hasUpdate && (
            <span className="chip ok mini">
              <Icon name="check" size="sm" />
              已是最新
            </span>
          )}
          <span style={{ flex: 1 }} />
          <button
            className="btn mini"
            disabled={upd.phase === "checking" || downloading}
            onClick={() => void checkForUpdate()}
          >
            <Icon name="refresh" size="sm" />
            {upd.phase === "checking" ? "检查中" : "检查更新"}
          </button>
          {upd.phase === "available" && (
            <button className="btn primary mini" onClick={() => void startUpdateDownload()}>
              <Icon name="save" size="sm" />
              下载并安装
            </button>
          )}
          {downloading && (
            <button className="btn mini" onClick={() => void cancelUpdateDownload()}>
              取消下载
            </button>
          )}
          {upd.phase === "ready" && (
            <button className="btn primary mini" onClick={() => void installDownloadedUpdate()}>
              <Icon name="save" size="sm" />
              立即安装
            </button>
          )}
          {upd.phase === "installing" && <span className="m">正在打开安装向导…</span>}
        </div>
        {downloading && (
          <div className="ab-upd-prog">
            <div className="ab-upd-bar">
              <span
                style={{
                  // 总量未知时留空，别先摆一个满格再跳回来
                  width: upd.percent >= 0 ? `${upd.percent}%` : "0%",
                  opacity: upd.phase === "retrying" ? 0.55 : 1,
                }}
              />
            </div>
            <span className="m ab-upd-pct">
              {upd.percent >= 0 ? `${upd.percent}%` : `${Math.round(upd.downloaded / 1024)} KB`}
            </span>
          </div>
        )}
        {upd.phase === "retrying" && (
          <div className="thint">
            连接断了，正在重试第 {upd.attempt} 次（{upd.candidate}）。进度会停在这里，不会重头再来。
          </div>
        )}
        {upd.info?.hasUpdate && upd.info.notes.trim() && (
          <details className="ab-upd-notes">
            <summary>更新说明</summary>
            <pre>{upd.info.notes.trim()}</pre>
          </details>
        )}
        {downloading && (
          <div className="thint">
            下载在后台进行，切到别的页面也不会中断。
          </div>
        )}
        {upd.phase === "ready" && (
          <div className="thint">
            安装包已下好。点「立即安装」会退出随译并打开安装向导，装完在向导最后一步勾选「运行 SuiYi」即可。
          </div>
        )}
        {upd.phase === "error" && (
          <div className="terr">
            <Icon name="alert" size="sm" />
            <span>{upd.error}</span>
            {upd.errorFrom === "check" ? (
              <button className="btn mini" onClick={() => void checkForUpdate()}>
                重新检查
              </button>
            ) : (
              <button className="btn mini" onClick={() => void startUpdateDownload()}>
                重试下载
              </button>
            )}
            <button className="btn mini" onClick={() => void openUrl(upd.info?.pageUrl ?? DOWNLOAD_PAGE)}>
              打开下载页
            </button>
            <button className="mini-as-link" title="知道了" onClick={dismissUpdateError}>
              <Icon name="close" size="sm" />
            </button>
          </div>
        )}
      </div>

      <div className="card">
        <div className="card-head"><b>隐私</b></div>
        <ul className="ab-list">
          <li>API Key 只存进 Windows 凭据管理器（命名空间 suiyi），配置文件里没有任何密钥。</li>
          <li>截图识别在你这台机器上完成，截图不会上传；只有识别出来的文字会发给你配置的翻译服务。</li>
          <li>翻译请求直接发往你填的 Base URL，随译中间不经过任何服务器。</li>
        </ul>
      </div>

      <div className="card">
        <div className="card-head">
          <b>数据与诊断</b>
          <span className="m">出问题时把这几行连同日志一起给我</span>
          <span style={{ flex: 1 }} />
          <button className="btn mini" onClick={() => void copyDiagnostics()}>
            <Icon name="copy" size="sm" />复制诊断信息
          </button>
        </div>
        {[
          ["配置目录", paths?.configDir],
          ["日志文件", paths?.logFile],
          ["插件目录", paths?.pluginsDir],
        ].map(([label, value]) => (
          <div className="ab-path" key={label as string}>
            <span className="ab-path-label">{label}</span>
            <code className="ab-path-value">{value ?? "（读不到）"}</code>
            <button
              className="btn mini"
              disabled={!value}
              onClick={() => void reveal(value as string | undefined)}
            >
              在文件夹中显示
            </button>
          </div>
        ))}
      </div>

      <div className="card">
        <div className="card-head"><b>快捷键</b><span className="m">当前生效的组合</span></div>
        <div className="ab-keys">
          {hotkeys.length === 0 && <div className="empty-hint">还没读到热键状态</div>}
          {hotkeys.map((h) => (
            <div className="ab-key" key={h.id}>
              <span className="ab-key-label">{h.label}</span>
              <span className="ab-key-combo">
                {h.accelerator.split("+").map((k, i) => (
                  <span className="kbd" key={i}>{formatAccel(k)}</span>
                ))}
              </span>
              {!h.registered && <span className="chip err mini">未注册</span>}
            </div>
          ))}
        </div>
      </div>

      <div className="card">
        <div className="card-head"><b>第三方组件</b></div>
        <div className="ab-thanks">
          Tauri（MIT / Apache-2.0）、React（MIT）、Tokio / reqwest（MIT / Apache-2.0）、
          rusqlite（MIT）、rquickjs（MIT）、keyring（MIT / Apache-2.0）、
          PaddleOCR 模型与 RapidOCR（Apache-2.0）、ONNX Runtime（MIT）。
          <br />
          随译 SuiYi 本身以 MIT 许可发布，全文见仓库里的 LICENSE。
          产品名与商标归各自所有者所有。
        </div>
      </div>

      {notice && <div className="toast ok">{notice}</div>}
    </div>
  );
}
