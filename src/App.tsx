import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import SettingsPage from "./pages/Settings";
import TranslatePage from "./pages/Translate";
import HistoryPage from "./pages/HistoryPage";
import {
  closeAction,
  getSettings,
  hotkeyStatus,
  retryHotkeys,
  setHotkey,
  type HotkeyStatus,
} from "./api";
import {
  cancelUpdateDownload,
  checkForUpdate,
  installDownloadedUpdate,
  startUpdateDownload,
  useUpdateSnapshot,
} from "./updateState";
import { Icon } from "./components/Icon";
import { formatAccel, suggestionFor } from "./hotkeySuggest";
import "./App.css";

type Tab = "translate" | "history" | "settings";

export default function App() {
  const [tab, setTab] = useState<Tab>("translate");
  /** 热键注册状态：有键没注册上就在顶部提示，别让用户按半天没反应还不知道为什么 */
  const [hotkeys, setHotkeys] = useState<HotkeyStatus[]>([]);
  const [dismissed, setDismissed] = useState(false);
  const [adopting, setAdopting] = useState(false);
  /** 关窗口时问一次：直接关闭还是收进托盘 */
  const [closeAsk, setCloseAsk] = useState(false);
  const [rememberClose, setRememberClose] = useState(false);
  const [closeBusy, setCloseBusy] = useState(false);
  /** 发现有新版本时的提示条 */
  const [dismissedBanner, setDismissedBanner] = useState("");
  /** 更新流程的状态：下载在 Rust 里跑，切页、关掉关于页都不受影响 */
  const upd = useUpdateSnapshot();
  const updateBusy = upd.phase === "downloading" || upd.phase === "retrying";
  /** 递增信号：点提示条的「去关于页」时切到设置页并打开关于 */
  const [aboutSignal, setAboutSignal] = useState(0);
  /** 递增信号：托盘菜单的「检查更新」要求关于页立刻查一次 */
  const [updateCheckSignal, setUpdateCheckSignal] = useState(0);

  useEffect(() => {
    void hotkeyStatus().then(setHotkeys).catch(() => {});
    const un = listen<HotkeyStatus[]>("hotkey-status", (e) => {
      setHotkeys(e.payload);
      setDismissed(false); // 状态变了就重新提示
    });
    return () => {
      void un.then((f) => f());
    };
  }, []);

  // × 被后端拦下来（prevent_close）后问一次：直接关闭还是收进托盘。
  // 用户在设置里选了「每次询问」之外的值时，后端直接执行，不会发这个事件。
  useEffect(() => {
    const un = listen("close-requested", () => setCloseAsk(true));
    return () => {
      void un.then((f) => f());
    };
  }, []);

  // 托盘右键菜单的「检查更新」：把界面带到关于页并当场查一次
  useEffect(() => {
    const un = listen("check-update-requested", () => {
      setTab("settings");
      setAboutSignal((n) => n + 1);
      setUpdateCheckSignal((n) => n + 1);
    });
    return () => {
      void un.then((f) => f());
    };
  }, []);

  // 启动后自动查一次更新：只提示，不自动下载。查不到就什么也不说
  useEffect(() => {
    void getSettings()
      .then((s) => {
        if (!s.autoCheckUpdate) return;
        // silent：离线、被限流都当没更新，这不是用户此刻要做的事，不打扰
        return checkForUpdate(true);
      })
      .catch(() => {});
  }, []);

  const answerClose = useCallback(
    async (action: "quit" | "tray") => {
      setCloseBusy(true);
      try {
        await closeAction(action, rememberClose);
      } catch (e) {
        console.error(e);
      } finally {
        setCloseBusy(false);
        setCloseAsk(false);
      }
    },
    [rememberClose],
  );

  // Esc 关掉询问框 = 这次不关窗口
  useEffect(() => {
    if (!closeAsk) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setCloseAsk(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [closeAsk]);

  const pending = hotkeys.filter((h) => !h.registered);
  const latest = upd.info?.latest ?? "";
  /** 提示条的「这一条」：阶段或失败原因一变就是另一条，之前的「收起」不再生效 */
  const bannerKey = `${upd.phase}|${upd.error}`;
  /** 查版本的失败只在关于页说，别在顶部再弹一条 */
  const bannerCandidate =
    upd.phase === "available" ||
    updateBusy ||
    upd.phase === "ready" ||
    (upd.phase === "error" && upd.errorFrom !== "check");
  const updateBanner = bannerCandidate && bannerKey !== dismissedBanner;

  const goAbout = () => {
    setTab("settings");
    setAboutSignal((n) => n + 1);
  };

  /** 一键改用建议的组合；建议的组合也被占了就保持提示不动 */
  async function adoptSuggested() {
    setAdopting(true);
    try {
      for (const h of pending) {
        await setHotkey(h.id, suggestionFor(h.accelerator || "alt+d"));
      }
      setHotkeys(await hotkeyStatus());
    } catch (e) {
      console.error(e);
    } finally {
      setAdopting(false);
    }
  }

  return (
    <div className="app-shell">
      <nav className="tabbar">
        <button className={tab === "translate" ? "on" : ""} onClick={() => setTab("translate")}>
          翻译
        </button>
        <button className={tab === "history" ? "on" : ""} onClick={() => setTab("history")}>
          历史
        </button>
        <button className={tab === "settings" ? "on" : ""} onClick={() => setTab("settings")}>
          设置
        </button>
      </nav>
      {pending.length > 0 && !dismissed && (
        <div className="hotkey-banner">
          <Icon name="alert" size="sm" />
          <span>
            {pending.map((h) => formatAccel(h.accelerator)).join("、")}
            被其他程序占用（PixPin 这类截图/划词工具最常见），现在按了没反应。
          </span>
          <span style={{ flex: 1 }} />
          <button
            className="btn mini"
            disabled={adopting}
            title={pending
              .map((h) => `${formatAccel(h.accelerator)} → ${formatAccel(suggestionFor(h.accelerator))}`)
              .join("，")}
            onClick={() => void adoptSuggested()}
          >
            {adopting ? "切换中" : "改用推荐热键"}
          </button>
          <button className="btn mini" onClick={() => void retryHotkeys().then(setHotkeys)}>
            重试
          </button>
          <button className="mini-as-link" title="先不管" onClick={() => setDismissed(true)}>
            <Icon name="close" size="sm" />
          </button>
        </div>
      )}
      {updateBanner && (
        <div className="update-banner">
          <Icon
            name={upd.phase === "ready" ? "check" : upd.phase === "error" ? "alert" : "refresh"}
            size="sm"
          />
          {upd.phase === "available" && (
            <span>
              发现新版本 v{latest}（当前 v{upd.info?.current}）
            </span>
          )}
          {updateBusy && (
            <>
              <span>
                正在下载更新{latest ? ` v${latest}` : ""} ·{" "}
                {upd.percent >= 0 ? `${upd.percent}%` : `${Math.round(upd.downloaded / 1024)} KB`}
              </span>
              <div className="upd-track" aria-hidden="true">
                <span
                  style={{
                    // 总量未知时留空，别先摆一个满格再跳回来
                    width: upd.percent >= 0 ? `${upd.percent}%` : "0%",
                    opacity: upd.phase === "retrying" ? 0.55 : 1,
                  }}
                />
              </div>
              {upd.phase === "retrying" && <span className="upd-note">连接断了，正在重试第 {upd.attempt} 次</span>}
            </>
          )}
          {upd.phase === "ready" && <span>更新已下载，随时可以安装</span>}
          {upd.phase === "error" && <span>{upd.error}</span>}
          <span style={{ flex: 1 }} />
          {upd.phase === "available" && (
            <button className="btn mini" onClick={goAbout}>
              去关于页更新
            </button>
          )}
          {updateBusy && (
            <>
              <button className="btn mini" onClick={() => void cancelUpdateDownload()}>
                取消下载
              </button>
              <button className="btn mini" onClick={goAbout}>
                查看
              </button>
            </>
          )}
          {upd.phase === "ready" && (
            <button className="btn primary mini" onClick={() => void installDownloadedUpdate()}>
              立即安装
            </button>
          )}
          {upd.phase === "error" && upd.errorFrom !== "check" && (
            <>
              <button className="btn mini" onClick={() => void startUpdateDownload()}>
                重试
              </button>
              <button className="btn mini" onClick={goAbout}>
                查看
              </button>
            </>
          )}
          {(upd.phase === "available" || upd.phase === "ready" || upd.phase === "error") && (
            <button
              className="mini-as-link"
              title={upd.phase === "available" ? "以后再说" : "收起"}
              onClick={() => setDismissedBanner(bannerKey)}
            >
              <Icon name="close" size="sm" />
            </button>
          )}
        </div>
      )}
      <div className="tab-body">
        {tab === "translate" && <TranslatePage onOpenSettings={() => setTab("settings")} />}
        {tab === "history" && <HistoryPage />}
        {tab === "settings" && (
          <SettingsPage focusAboutSignal={aboutSignal} checkUpdateSignal={updateCheckSignal} />
        )}
      </div>

      {closeAsk && (
        <div className="ask-mask" onClick={() => setCloseAsk(false)}>
          <div
            className="ask-card"
            role="dialog"
            aria-modal="true"
            aria-label="关闭随译"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="ask-head">
              <b>关闭随译？</b>
            </div>
            <div className="ask-body">
              「收进托盘」会把窗口藏到右下角的小图标里，Alt+D / Alt+S 继续可用，单击图标就能叫回来；
              「直接关闭」会结束随译，两个热键一起失效。
            </div>
            <label className="kcheck">
              <input
                type="checkbox"
                checked={rememberClose}
                onChange={(e) => setRememberClose(e.target.checked)}
              />
              <span>记住我的选择（设置 → 通用 里可以改回来）</span>
            </label>
            <div className="ask-foot">
              <button className="btn mini" disabled={closeBusy} onClick={() => setCloseAsk(false)}>
                取消
              </button>
              <span style={{ flex: 1 }} />
              <button className="btn danger mini" disabled={closeBusy} onClick={() => void answerClose("quit")}>
                直接关闭
              </button>
              <button className="btn primary mini" disabled={closeBusy} onClick={() => void answerClose("tray")}>
                收进托盘
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
