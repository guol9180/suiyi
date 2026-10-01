// S0.8 最小翻译界面：输入 → 各启用服务并发流式出稿（对应设计稿①的卡片形态）
import { useCallback, useEffect, useState } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { listServices } from "../api";
import { recordResult } from "../lastResult";
import {
  SOURCE_LANGS,
  TARGET_LANGS,
  swapLanguages,
  type DictionaryResult,
  type ServiceConfig,
  type TranslateResult,
} from "../types";
import { Icon } from "../components/Icon";
import { DictionaryCard } from "../components/DictionaryCard";
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

interface CardState {
  id: string;
  name: string;
  text: string;
  status: "streaming" | "done" | "error";
  elapsedMs?: number;
  error?: string;
  dictionary?: DictionaryResult;
}

/** invoke 失败时可能抛字符串也可能抛 Error，统一成一句能给用户看的话 */
function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export default function TranslatePage(props: { onOpenSettings?: () => void } = {}) {
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

  const doneCount = cards.filter((c) => c.status === "done").length;
  const failedCount = cards.filter((c) => c.status === "error").length;
  const settledCount = doneCount + failedCount;

  return (
    <div className="translate-page">
      <div className="lang-group">
        <div className="row-label">翻译设置</div>
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
        {services.length > 0 && (
          settledCount > 0 ? (
            <span className={`chip ${failedCount > 0 ? "warn" : "ok"}`}>
              {doneCount} / {services.length} 服务成功
            </span>
          ) : (
            <span className="chip">{services.length} 个服务待命</span>
          )
        )}
        <button className="btn mini" onClick={() => void listServices().then((f) =>
          setServices(f.services.filter((s) => s.enabled && s.kind === "translation").sort((a, b) => a.order - b.order)),
          )}><Icon name="refresh" size="sm" />刷新服务</button>
        </div>
      </div>

      <textarea
        className="inp src-area"
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && e.ctrlKey) void doTranslate();
        }}
        placeholder="输入要翻译的文本…"
      />

      <div className="actions">
        <button className="btn primary" disabled={busy || !text.trim()} onClick={() => void doTranslate()}>
          {busy ? "翻译中…" : "翻 译"}
        </button>
        <button className="btn" onClick={() => void invoke("start_screenshot").catch(console.error)}>
          <Icon name="frame" size="sm" />截图识别
        </button>
        <span className="muted">Ctrl+Enter 翻译</span>
      </div>

      <div className="cards">
        {/* 一个服务都没启用时的空态：设计稿附录 A 的「无服务」是带下一步动作的，
            这里同样给一个能直接点的入口，不让用户自己找路 */}
        {services.length === 0 && cards.length === 0 && (
          <div className="emptybox">
            还没有可用的翻译服务
            <br />
            请到「设置 → 服务配置」启用并填写 API Key
            {props.onOpenSettings && (
              <span style={{ display: "block", marginTop: 10 }}>
                <button className="btn primary mini" onClick={props.onOpenSettings}>
                  <Icon name="sliders" size="sm" />
                  打开设置
                </button>
              </span>
            )}
          </div>
        )}
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
              <>
                <div className="rt rc-err">{c.error}</div>
                <div className="rc-foot">
                  <span className="muted">配额限流可在「设置 → 服务配置」用测试连接排查</span>
                  <button className="btn mini" onClick={() => void retryOne(c.id)}>
                    <Icon name="refresh" size="sm" />重试
                  </button>
                </div>
              </>
            ) : c.dictionary ? (
              <DictionaryCard dict={c.dictionary} />
            ) : (
              <div className="rt">
                {c.text}
                {c.status === "streaming" && <span className="caret" />}
              </div>
            )}
            {c.status === "done" && c.text && !c.dictionary && (
              <div className="rc-meta">输出 {c.text.length} 字</div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
