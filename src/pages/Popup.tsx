// 划词翻译弹窗（设计稿①）：无边框小窗，接收选中文本，多服务并发流式出稿
import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listServices } from "../api";
import type { ServiceConfig } from "../types";
import "./Popup.css";

interface CardState {
  id: string;
  name: string;
  text: string;
  status: "streaming" | "done" | "error";
  elapsedMs?: number;
  error?: string;
}

const FROM_LANGS = ["自动检测", "中文", "English", "日本語"];
const TO_LANGS = ["简体中文", "English", "日本語"];

export default function PopupPage() {
  const [source, setSource] = useState("");
  const [from, setFrom] = useState("自动检测");
  const [to, setTo] = useState("简体中文");
  const [services, setServices] = useState<ServiceConfig[]>([]);
  const [cards, setCards] = useState<CardState[]>([]);
  const [busy, setBusy] = useState(false);
  const [pinned, setPinned] = useState(false);

  const refreshServices = useCallback(async () => {
    try {
      const f = await listServices();
      setServices(
        f.services.filter((s) => s.enabled && s.kind === "translation").sort((a, b) => a.order - b.order),
      );
    } catch {
      /* 弹窗静默 */
    }
  }, []);

  useEffect(() => {
    void refreshServices();
  }, [refreshServices]);

  const doTranslate = useCallback(
    async (src: string) => {
      if (!src.trim() || services.length === 0) return;
      setBusy(true);
      setCards(services.map((s) => ({ id: s.id, name: s.name, text: "", status: "streaming" as const })));
      await Promise.allSettled(
        services.map(async (s) => {
          try {
            const r = await invoke<{ serviceId: string; text: string; elapsedMs: number }>(
              "translate_text",
              { serviceId: s.id, text: src, from, to },
            );
            setCards((cs) =>
              cs.map((c) =>
                c.id === s.id ? { ...c, text: r.text, status: "done", elapsedMs: r.elapsedMs } : c,
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
    },
    [services, from, to],
  );

  // 主窗口（或未来的取词服务）投递选中文本
  useEffect(() => {
    const un = listen<{ text: string; autoTranslate?: boolean }>("popup-set-source", (e) => {
      setSource(e.payload.text);
      setCards([]);
      if (e.payload.autoTranslate !== false) {
        // 等服务列表就绪后自动翻译
        window.setTimeout(() => void doTranslate(e.payload.text), 120);
      }
    });
    return () => {
      void un.then((f) => f());
    };
  }, [doTranslate]);

  // Esc 关闭（隐藏窗口，保留内容）
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        void getCurrentWindow().hide();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  async function togglePin() {
    const next = !pinned;
    setPinned(next);
    try {
      await getCurrentWindow().setAlwaysOnTop(next);
    } catch {
      /* 忽略 */
    }
  }

  function copyResult() {
    const done = cards.find((c) => c.status === "done" && c.text) ?? cards.find((c) => c.text);
    if (done) void navigator.clipboard.writeText(done.text);
  }

  return (
    <div className="popup-root">
      <div className="popup-card">
        {/* 头部：语言方向 + 状态 + 拖拽区 */}
        <div className="pop-head" data-tauri-drag-region>
          <select className="pop-lang" value={from} onChange={(e) => setFrom(e.target.value)}>
            {FROM_LANGS.map((l) => (
              <option key={l} value={l}>{l}</option>
            ))}
          </select>
          <span className="arrow">→</span>
          <select className="pop-lang" value={to} onChange={(e) => setTo(e.target.value)}>
            {TO_LANGS.map((l) => (
              <option key={l} value={l}>{l}</option>
            ))}
          </select>
          <span style={{ flex: 1 }} />
          {services.length > 0 && (
            <span className="chip ok mini">{services.length} 服务并发</span>
          )}
          <button
            className={`pop-icon${pinned ? " pin-on" : ""}`}
            title={pinned ? "取消固定" : "固定窗口"}
            onClick={() => void togglePin()}
          >
            📌
          </button>
          <button className="pop-icon" title="关闭 (Esc)" onClick={() => void getCurrentWindow().hide()}>
            ✕
          </button>
        </div>

        {/* 原文 */}
        {source && (
          <div className="pop-src">
            <div className="lab">原文</div>
            <div className="src-text">{source}</div>
          </div>
        )}

        {/* 结果卡片 */}
        <div className="pop-cards">
          {cards.length === 0 && (
            <div className="pop-empty">
              {source ? "正在翻译…" : "等待选中文本…\n在任意应用中选中文字后按 Alt+D，\n或从主窗口投递文本。"}
            </div>
          )}
          {cards.map((c) => (
            <div key={c.id} className="rescard">
              <div className="rh">
                <span className={`dot${c.status === "error" ? " err" : ""}`} />
                <b>{c.name}</b>
                {c.status === "done" && c.elapsedMs != null && (
                  <span>{(c.elapsedMs / 1000).toFixed(1)}s</span>
                )}
                {c.status === "streaming" && <span>流式生成中…</span>}
                <span style={{ flex: 1 }} />
                {c.text && (
                  <button className="btn mini" onClick={() => void navigator.clipboard.writeText(c.text)}>
                    ⧉
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

        {/* 底部动作条 */}
        <div className="pop-actions">
          <button className="btn primary mini" onClick={copyResult} disabled={!cards.some((c) => c.text)}>
            ⧉ 复制
          </button>
          <button className="btn mini disabled-soon" title="M3 里程碑开放">⇄ 替换原文</button>
          <button className="btn mini disabled-soon" title="M4 里程碑开放">🔊 朗读</button>
          <button className="btn mini disabled-soon" title="M3 里程碑开放">＋ 生词本</button>
          <button
            className="btn mini"
            disabled={busy || !source}
            onClick={() => void doTranslate(source)}
          >
            ↻ 重译
          </button>
        </div>
        <div className="pop-status">
          <span className="dot" />
          {busy ? "流式输出中" : "就绪"} · Esc 关闭 · 拖动标题栏移动
        </div>
      </div>
    </div>
  );
}
