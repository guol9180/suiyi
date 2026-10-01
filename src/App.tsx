import { useState } from "react";
import SettingsPage from "./pages/Settings";
import TranslatePage from "./pages/Translate";
import "./App.css";

type Tab = "translate" | "settings";

export default function App() {
  const [tab, setTab] = useState<Tab>("translate");
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
