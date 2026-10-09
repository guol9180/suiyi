import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import SettingsPage from "./pages/Settings";
import TranslatePage from "./pages/Translate";
import {
  checkUpdate,
  closeAction,
  getSettings,
  hotkeyStatus,
  retryHotkeys,
  setHotkey,
  type HotkeyStatus,
  type UpdateInfo,
} from "./api";
import { Icon } from "./components/Icon";
import { formatAccel, suggestionFor } from "./hotkeySuggest";
import "./App.css";

type Tab = "translate" | "settings";

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
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [updateDismissed, setUpdateDismissed] = useState(false);
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

  // 启动后自动查一次更新：只提示，不自动下载
  useEffect(() => {
    let alive = true;
    void getSettings()
      .then((s) => {
        if (!alive || !s.autoCheckUpdate) return;
        return checkUpdate()
          .then((info) => {
            if (alive && info.hasUpdate) setUpdate(info);
          })
          .catch(() => {
            // 离线、被限流都当没更新：这不是用户此刻要做的事，不打扰
          });
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
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
      {update?.hasUpdate && !updateDismissed && (
        <div className="update-banner">
          <Icon name="refresh" size="sm" />
          <span>
            发现新版本 v{update.latest}（当前 v{update.current}）
          </span>
          <span style={{ flex: 1 }} />
          <button
            className="btn mini"
            onClick={() => {
              setTab("settings");
              setAboutSignal((n) => n + 1);
            }}
          >
            去关于页更新
          </button>
          <button className="mini-as-link" title="以后再说" onClick={() => setUpdateDismissed(true)}>
            <Icon name="close" size="sm" />
          </button>
        </div>
      )}
      <div className="tab-body">
        {tab === "translate" ? (
          <TranslatePage onOpenSettings={() => setTab("settings")} />
        ) : (
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
