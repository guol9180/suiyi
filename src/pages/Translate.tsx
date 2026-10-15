// S0.8 最小翻译界面：输入 → 各启用服务并发流式出稿（对应设计稿①的卡片形态）
import { useCallback, useEffect, useState } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { listHistory, listServices } from "../api";
import { recordResult } from "../lastResult";
import {
  enabledTranslations,
  type CardState,
  type HistoryEntry,
  type ServiceConfig,
  type TranslateResult,
} from "../types";
import { formatTime } from "../format";
import { Icon } from "../components/Icon";
import { ResultCard } from "../components/ResultCard";
import { LangPicker } from "../components/LangPicker";
import { svcColor } from "../svcColor";
import "./Translate.css";

/** 打开划词弹窗并投递文本（M1 将由取词服务自动调用） */
export async function openTranslatePopup(sample: string) {
  let win = await WebviewWindow.getByLabel("popup");
  if (!win) {
    win = new WebviewWindow("popup", {
      url: "popup.html",
    title: "划词翻译",
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
  await emit("popup-set-source", { text: sample, autoTranslate: true, kind: "manual" });
}

/** invoke 失败时可能抛字符串也可能抛 Error，统一成一句能给用户看的话 */
function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export default function TranslatePage(props: {
  /** 本页签是否处于激活状态：页签是 keep-mounted 的，激活时重拉最近翻译，删掉的历史才不会残留 */
  active: boolean;
  onOpenSettings?: () => void;
  onOpenHistory: () => void;
}) {
  const [services, setServices] = useState<ServiceConfig[]>([]);
  const [from, setFrom] = useState("自动检测");
  const [to, setTo] = useState("简体中文");
  const [text, setText] = useState("");
  const [cards, setCards] = useState<CardState[]>([]);
  const [busy, setBusy] = useState(false);
  /** 右栏「最近翻译」：历史库的前 5 条，翻译一轮后刷新 */
  const [recent, setRecent] = useState<HistoryEntry[]>([]);

  const loadRecent = useCallback(() => {
    listHistory({ limit: 5 })
      .then(setRecent)
      .catch(() => setRecent([]));
  }, []);

  // 挂载与每次切回本页签时都重拉最近翻译：划词/截图在其他窗口产生记录，
  // 历史页里的删除也发生在别的页签——不重拉就会看到残留的「幽灵」记录
  useEffect(() => {
    if (props.active) loadRecent();
  }, [props.active, loadRecent]);

  useEffect(() => {
    void listServices().then((f) => setServices(enabledTranslations(f.services)));
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
          const r = await invoke<TranslateResult>(
            "translate_text",
            { serviceId: s.id, text, from, to, kind: "manual" },
          );
          recordResult(s.id, { ok: true, at: Date.now() });
          setCards((cs) =>
            cs.map((c) =>
              c.id === s.id
                ? {
                    ...c,
                    text: r.text,
                    status: "done",
                    elapsedMs: r.elapsedMs,
                    dictionary: r.dictionary,
                  }
                : c,
            ),
          );
        } catch (e) {
          const msg = errText(e);
          recordResult(s.id, { ok: false, error: msg, at: Date.now() });
          setCards((cs) =>
            cs.map((c) => (c.id === s.id ? { ...c, status: "error", error: msg } : c)),
          );
        }
      }),
    );
    setBusy(false);
    // 右栏最近翻译跟着这一轮刷新
    loadRecent();
  }

  /** 单独重跑一路服务：失败卡片上的「重试」 */
  const retryOne = useCallback(
    async (serviceId: string) => {
      const s = services.find((x) => x.id === serviceId);
      if (!s || !text.trim()) return;
      setCards((cs) =>
        cs.map((c) =>
          c.id === serviceId ? { ...c, status: "streaming" as const, text: "", error: undefined } : c,
        ),
      );
      try {
        const r = await invoke<TranslateResult>("translate_text", {
          serviceId: s.id,
          text,
          from,
          to,
          kind: "manual",
        });
        recordResult(s.id, { ok: true, at: Date.now() });
        setCards((cs) =>
          cs.map((c) =>
            c.id === serviceId
              ? { ...c, status: "done" as const, text: r.text, elapsedMs: r.elapsedMs, dictionary: r.dictionary }
              : c,
          ),
        );
      } catch (e) {
        const msg = errText(e);
        recordResult(serviceId, { ok: false, error: msg, at: Date.now() });
        setCards((cs) =>
          cs.map((c) => (c.id === serviceId ? { ...c, status: "error" as const, error: msg } : c)),
        );
      }
    },
    [services, text, from, to],
  );

  return (
    <div className="workbench">
      <div className="work-main">
        <div className="hero-input">
          <div className="hero-top">
            <LangPicker from={from} to={to} onChangeFrom={setFrom} onChangeTo={setTo} selectClassName="inp" />
            <span className="hero-count">{text.length} / 5000 字</span>
          </div>
          <textarea
            className="inp hero-area"
            maxLength={5000}
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && e.ctrlKey) void doTranslate();
            }}
            placeholder="输入要翻译的文本…"
          />
          <div className="hero-actions">
            <span className="hint">Ctrl+Enter 翻译</span>
            <span style={{ flex: 1 }} />
            <button className="btn mini" disabled={!text} onClick={() => setText("")}>
              清空
            </button>
            <button className="btn" onClick={() => void invoke("start_screenshot").catch(console.error)}>
              <Icon name="frame" size="sm" />截图识别
            </button>
            <button className="btn primary" disabled={busy || !text.trim()} onClick={() => void doTranslate()}>
              {busy ? "翻译中…" : "翻 译"}
            </button>
          </div>
        </div>

        <div className="resh">
          翻译结果
          <span className="m">{services.length} 个服务并发 · 失败按顺序回退</span>
          <span style={{ flex: 1 }} />
          <button className="btn mini" disabled={busy || !text.trim()} onClick={() => void doTranslate()}>
            <Icon name="refresh" size="sm" />重试全部
          </button>
        </div>

        <div className="results">
          {cards.length === 0 ? (
            <div className="onboard">
              {/* 三步引导：还没有结果时占住结果区，完成首次翻译自动让位 */}
              <div className="ob-step">
                <span className="ob-n">1</span>
                <span>到「设置 → 服务配置」选择一个提供商并启用</span>
              </div>
              <div className="ob-step">
                <span className="ob-n">2</span>
                <span>
                  在任意应用选中文字，按 <span className="kbd">Alt</span>
                  <span className="kbd">D</span> 划词翻译
                </span>
              </div>
              <div className="ob-step">
                <span className="ob-n">3</span>
                <span>
                  任意界面按 <span className="kbd">Alt</span>
                  <span className="kbd">S</span> 截图识别
                </span>
              </div>
              {props.onOpenSettings && (
                <div className="ob-act">
                  <button className="btn primary mini" onClick={props.onOpenSettings}>
                    <Icon name="sliders" size="sm" />打开设置
                  </button>
                </div>
              )}
              <div className="gnote">完成首次翻译后，这里显示结果，引导自动收起。</div>
            </div>
          ) : (
            cards.map((c) => (
              <ResultCard key={c.id} card={c} svc={svcColor(c.id)} onRetry={retryOne} copyLabel="复制" showMeta />
            ))
          )}
        </div>
      </div>

      <div className="rail">
        <div className="rail-card">
          <div className="rail-h">
            最近翻译<span className="m">最近 5 条</span>
          </div>
          {recent.length === 0 && <div className="rail-empty">还没有翻译记录</div>}
          {recent.map((h) => (
            <div
              key={h.id}
              className="quick"
              title="把原文填回输入框"
              onClick={() => setText(h.source)}
            >
              <span className="qt">{h.source}</span>
              <span className="qm">{formatTime(h.createdAt)}</span>
            </div>
          ))}
          <div className="rail-more">
            <button className="btn mini" onClick={props.onOpenHistory}>
              <Icon name="arrow-right" size="sm" />打开历史
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
