import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import SettingsPage from "./pages/Settings";
import TranslatePage from "./pages/Translate";
import { hotkeyStatus, retryHotkeys, setHotkey, type HotkeyStatus } from "./api";
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
      <div className="tab-body">
        {tab === "translate" ? (
          <TranslatePage onOpenSettings={() => setTab("settings")} />
        ) : (
          <SettingsPage />
        )}
      </div>
    </div>
  );
}
