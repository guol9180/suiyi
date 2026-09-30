// S0.8 最小翻译界面：输入 → 各启用服务并发流式出稿（对应设计稿①的卡片形态）
import { useEffect, useState } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { listServices } from "../api";
import { SOURCE_LANGS, TARGET_LANGS, swapLanguages, type ServiceConfig } from "../types";
import { Icon } from "../components/Icon";
import "./Translate.css";

/** 打开划词弹窗并投递文本（M1 将由取词服务自动调用） */
export async function openTranslatePopup(sample: string) {
  let win = await WebviewWindow.getByLabel("popup");
  if (!win) {
    win = new WebviewWindow("popup", {
      url: "popup.html",
      title: "随译 · 划词翻译",
      width: 430,
      height: 540,
      minWidth: 360,
      minHeight: 400,
      decorations: false,
      transparent: true,
      shadow: true,
      center: true,
      resizable: true,
    });
    // 新窗口挂载需要一点时间，再投递文本
    await new Promise((r) => setTimeout(r, 600));
  } else {
    await win.show();
    await win.setFocus();
  }
  await emit("popup-set-source", { text: sample, autoTranslate: true });
}

interface CardState {
  id: string;
  name: string;
  text: string;
  status: "streaming" | "done" | "error";
  elapsedMs?: number;
  error?: string;
}

export default function TranslatePage() {
  const [services, setServices] = useState<ServiceConfig[]>([]);
  const [from, setFrom] = useState("自动检测");
  const [to, setTo] = useState("简体中文");
  const [text, setText] = useState("");
  const [cards, setCards] = useState<CardState[]>([]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void listServices().then((f) =>
      setServices(
        f.services
          .filter((s) => s.enabled && s.kind === "translation")
          .sort((a, b) => a.order - b.order),
      ),
    );
  }, []);

  // 流式增量事件 → 追加到对应卡片
  useEffect(() => {
    const un = listen<{ serviceId: string; delta: string }>("translate-delta", (e) => {
      setCards((cs) =>
        cs.map((c) =>
          c.id === e.payload.serviceId
            ? { ...c, text: c.text + e.payload.delta, status: "streaming" }
            : c,
        ),
      );
    });
    return () => {
      void un.then((f) => f());
    };
  }, []);

  async function doTranslate() {
    if (!text.trim() || busy) return;
    if (services.length === 0) {
      setCards([
        {
          id: "__none",
          name: "提示",
          text: "还没有已启用的服务。请到「设置 → 服务配置」启用并填好 API Key。",
          status: "error",
        },
      ]);
      return;
    }
    setBusy(true);
    setCards(
      services.map((s) => ({ id: s.id, name: s.name, text: "", status: "streaming" as const })),
    );
    await Promise.allSettled(
      services.map(async (s) => {
        try {
          const r = await invoke<{ serviceId: string; text: string; elapsedMs: number }>(
            "translate_text",
            { serviceId: s.id, text, from, to },
          );
          setCards((cs) =>
            cs.map((c) =>
              c.id === s.id
                ? { ...c, text: r.text, status: "done", elapsedMs: r.elapsedMs }
                : c,
            ),
          );
        } catch (e) {
          setCards((cs) =>
            cs.map((c) => (c.id === s.id ? { ...c, status: "error", error: String(e) } : c)),
          );
        }
      }),
    );
    setBusy(false);
  }

  return (
    <div className="translate-page">
      <div className="lang-row">
        <select className="inp" value={from} onChange={(e) => setFrom(e.target.value)}>
          {SOURCE_LANGS.map((l) => (
            <option key={l} value={l}>{l}</option>
          ))}
        </select>
        <button
          className="swap"
          title="互换语言方向"
          aria-label="互换语言方向"
          onClick={() => {
            const next = swapLanguages(from, to);
            setFrom(next.from);
            setTo(next.to);
          }}
        >
          <Icon name="swap" size="sm" />
        </button>
        <select className="inp" value={to} onChange={(e) => setTo(e.target.value)}>
          {TARGET_LANGS.map((l) => (
            <option key={l} value={l}>{l}</option>
          ))}
        </select>
        <span style={{ flex: 1 }} />
        {services.length > 0 && <span className="chip ok">{services.length} 个服务并发</span>}
        <button className="btn mini" onClick={() => void listServices().then((f) =>
          setServices(f.services.filter((s) => s.enabled && s.kind === "translation").sort((a, b) => a.order - b.order)),
        )}><Icon name="refresh" size="sm" />刷新服务</button>
      </div>

      <textarea
        className="inp src-area"
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && e.ctrlKey) void doTranslate();
        }}
        placeholder="输入要翻译的文本…（Ctrl+Enter 翻译）"
      />

      <div className="actions">
        <button className="btn primary" disabled={busy || !text.trim()} onClick={() => void doTranslate()}>
          {busy ? "翻译中…" : "翻 译"}
        </button>
        <button
          className="btn"
          onClick={() => void openTranslatePopup(text.trim() || "The quick brown fox jumps over the lazy dog.")}
        >
          弹窗预览
        </button>
        <button className="btn" onClick={() => void invoke("start_screenshot").catch(console.error)}>
          <Icon name="frame" size="sm" />截图识别
        </button>
        <span className="muted">流式输出 · 多服务并发对比 · Ctrl+Enter 翻译</span>
      </div>

      <div className="cards">
        {cards.map((c) => (
          <div key={c.id} className="rescard">
            <div className="rh">
              <span className={`dot${c.status === "error" ? " err" : ""}`} />
              <b>{c.name}</b>
              {c.status === "done" && c.elapsedMs != null && <span>{(c.elapsedMs / 1000).toFixed(1)}s</span>}
              {c.status === "streaming" && <span>流式生成中…</span>}
              <span style={{ flex: 1 }} />
              {c.text && (
                <button className="btn mini" onClick={() => void navigator.clipboard.writeText(c.text)}>
                  <Icon name="copy" size="sm" />复制
                </button>
              )}
            </div>
            {c.error ? (
              <div className="rt rc-err">{c.error}</div>
            ) : (
              <div className="rt">
                {c.text}
                {c.status === "streaming" && <span className="caret" />}
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
