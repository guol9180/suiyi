// 划词翻译弹窗（设计稿①）：无边框小窗，接收选中文本，多服务并发流式出稿
import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  listPlugins,
  listServices,
  replaceSelection,
  runActionPlugin,
  speakText,
  stopSpeaking,
  wordbookAdd,
} from "../api";
import { describeError } from "../errorText";
import {
  SOURCE_LANGS,
  TARGET_LANGS,
  swapLanguages,
  type DictionaryResult,
  type HistoryKind,
  type PluginInfo,
  type ServiceConfig,
  type TranslateResult,
} from "../types";
import { Icon } from "../components/Icon";
import { DictionaryCard } from "../components/DictionaryCard";
import "./Popup.css";

interface CardState {
  id: string;
  name: string;
  text: string;
  status: "streaming" | "done" | "error";
  elapsedMs?: number;
  error?: string;
  dictionary?: DictionaryResult;
}

export default function PopupPage() {
  const [source, setSource] = useState("");
  const [from, setFrom] = useState("自动检测");
  const [to, setTo] = useState("简体中文");
  const [services, setServices] = useState<ServiceConfig[]>([]);
  const [cards, setCards] = useState<CardState[]>([]);
  const [busy, setBusy] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [moreOpen, setMoreOpen] = useState(false);
  const [notice, setNotice] = useState("");
  const [replacing, setReplacing] = useState(false);
  const [actionPlugins, setActionPlugins] = useState<PluginInfo[]>([]);
  // 当前这轮文本的来源，历史归类用；用 ref 避免重建 doTranslate
  const kindRef = useRef<HistoryKind>("selection");

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

  // 启用的动作插件会出现在「更多」菜单里，对当前这轮译文做自定义处理
  useEffect(() => {
    void listPlugins()
      .then((list) =>
        setActionPlugins(list.filter((p) => p.ok && p.enabled && p.kind === "action")),
      )
      .catch(() => setActionPlugins([]));
  }, []);

  const doTranslate = useCallback(
    async (src: string) => {
      if (!src.trim() || services.length === 0) return;
      setBusy(true);
      setCards(services.map((s) => ({ id: s.id, name: s.name, text: "", status: "streaming" as const })));
      await Promise.allSettled(
        services.map(async (s) => {
          try {
            const r = await invoke<TranslateResult>(
              "translate_text",
              { serviceId: s.id, text: src, from, to, kind: kindRef.current },
            );
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
    const un = listen<{ text: string; autoTranslate?: boolean; kind?: HistoryKind }>(
      "popup-set-source",
      (e) => {
        setSource(e.payload.text);
        setCards([]);
        setNotice("");
        kindRef.current = e.payload.kind ?? "selection";
        if (e.payload.autoTranslate !== false) {
          // 等服务列表就绪后自动翻译
          window.setTimeout(() => void doTranslate(e.payload.text), 120);
        }
      },
    );
    return () => {
      void un.then((f) => f());
    };
  }, [doTranslate]);

  // 取词失败：主进程把原因投过来，别让用户对着一个空窗口猜。
  // （输入框转译的失败提示走轻提示窗口，不再占用这个弹窗 —— 弹窗会抢走输入焦点。）
  useEffect(() => {
    const unNotice = listen<{ text: string }>("popup-notice", (e) => {
      setSource("");
      setCards([]);
      setNotice(e.payload.text || "没取到选中文字");
    });
    return () => {
      void unNotice.then((f) => f());
    };
  }, []);

  // Esc 关闭（隐藏窗口，保留内容）；失焦自动隐藏（固定/翻译中除外，延迟复核焦点）
  const pinnedRef = useRef(false);
  const busyRef = useRef(false);
  useEffect(() => {
    pinnedRef.current = pinned;
  }, [pinned]);
  useEffect(() => {
    busyRef.current = busy;
  }, [busy]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        void getCurrentWindow().hide();
      }
    };
    const onBlur = () => {
      window.setTimeout(() => {
        if (!pinnedRef.current && !busyRef.current && !document.hasFocus()) {
          void getCurrentWindow().hide();
        }
      }, 350);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("blur", onBlur);
    };
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

  /**
   * 用第一条成功译文替换掉原应用里选中的文字。
   * 主进程会先把弹窗藏起来、把焦点送回取词时的窗口，校验通过才粘贴。
   */
  async function replaceOriginal() {
    const done = cards.find((c) => c.status === "done" && c.text);
    if (!done) return;
    setReplacing(true);
    try {
      await replaceSelection(done.text);
    } catch (e) {
      setNotice(String(e));
    } finally {
      setReplacing(false);
    }
  }

  /** 用本地语音朗读第一条成功译文（重复点击会打断上一次播放） */
  async function speakResult() {
    const done = cards.find((c) => c.status === "done" && c.text);
    if (!done) return;
    try {
      await speakText(done.text);
    } catch (e) {
      setNotice(String(e));
    }
  }

  async function stopSpeech() {
    try {
      await stopSpeaking();
    } catch {
      /* 没在播放时忽略 */
    }
  }

  /** 把「原文 → 第一条译文」推进生词本（先落本地，Anki 没开也留得住） */
  async function addToAnki() {
    const done = cards.find((c) => c.status === "done" && c.text);
    if (!done || !source.trim()) return;
    try {
      const r = await wordbookAdd(source.trim(), done.text, { source: "selection" });
      if (r.duplicate) setNotice(r.error ?? "这条已经在生词本里了");
      else if (r.synced) setNotice("已加入生词本并同步到 Anki");
      else setNotice("已加入生词本，等 Anki 可用时自动同步");
    } catch (e) {
      setNotice(String(e));
    }
  }

  /** 运行动作插件，把它返回的提示展示出来 */
  async function runAction(p: PluginInfo) {
    const done = cards.find((c) => c.status === "done" && c.text);
    if (!done) return;
    try {
      const msg = await runActionPlugin(p.id, source, done.text);
      setNotice(msg || `已执行「${p.name}」`);
    } catch (e) {
      setNotice(String(e));
    }
  }

  /** 用必应搜索选中文本（后续可在设置里换搜索引擎） */
  function searchWeb() {
    if (source.trim()) void openUrl(`https://www.bing.com/search?q=${encodeURIComponent(source.trim())}`);
  }

  /** 文本像 URL 就直接打开，否则交给搜索引擎 */
  function openInBrowser() {
    const t = source.trim();
    if (!t) return;
    const url = /^https?:\/\//i.test(t)
      ? t
      : /^[\w-]+(\.[\w-]+)+(:\d+)?(\/\S*)?$/.test(t)
        ? `https://${t}`
        : `https://www.bing.com/search?q=${encodeURIComponent(t)}`;
    void openUrl(url);
  }

  return (
    <div className="popup-root">
      <div className="popup-card">
        {/* 头部：只剩拖拽把手与窗口按钮，语言选择移到下一行避免与拖动冲突 */}
        <div className="pop-head" data-tauri-drag-region>
          <span className="grip"><Icon name="grip" size="sm" /></span>
          <span className="pop-title">划词翻译</span>
          <span style={{ flex: 1 }} />
          <button
            className={`pop-icon${pinned ? " pin-on" : ""}`}
            title={pinned ? "取消固定" : "固定窗口"}
            aria-label={pinned ? "取消固定" : "固定窗口"}
            onClick={() => void togglePin()}
          >
            <Icon name="pin" size="sm" />
          </button>
          <button
            className="pop-icon"
            title="关闭 (Esc)"
            aria-label="关闭"
            onClick={() => void getCurrentWindow().hide()}
          >
            <Icon name="close" size="sm" />
          </button>
        </div>

        {/* 语言方向 */}
        <div className="pop-langrow">
          <select className="pop-lang" value={from} onChange={(e) => setFrom(e.target.value)}>
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
          <select className="pop-lang" value={to} onChange={(e) => setTo(e.target.value)}>
            {TARGET_LANGS.map((l) => (
              <option key={l} value={l}>{l}</option>
            ))}
          </select>
          <span style={{ flex: 1 }} />
          {services.length > 0 && (
            <span className="chip ok mini"><span className="dot" />{services.length} 服务并发</span>
          )}
        </div>

        {notice && (
          <div className="pop-notice">
            <Icon name="alert" size="sm" />
            <span>{notice}</span>
          </div>
        )}

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
                  <button
                    className="btn mini"
                    title="复制这条译文"
                    onClick={() => void navigator.clipboard.writeText(c.text)}
                  >
                    <Icon name="copy" size="sm" />
                  </button>
                )}
              </div>
              {c.error ? (
                // 弹窗窄，只放结论；完整原文留在历史记录里，这里悬停可看
                <div className="rt rc-err" title={c.error}>{describeError(c.error).title}</div>
              ) : c.dictionary ? (
                <DictionaryCard dict={c.dictionary} />
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
            <Icon name="copy" size="sm" />复制
          </button>
          <button
            className="btn mini"
            title="用第一条译文替换原应用里选中的文字"
            disabled={replacing || !cards.some((c) => c.status === "done" && c.text)}
            onClick={() => void replaceOriginal()}
          >
            <Icon name="swap" size="sm" />替换原文
          </button>
          <span style={{ flex: 1 }} />
          <button
            className={`btn mini${moreOpen ? " open" : ""}`}
            aria-expanded={moreOpen}
            onClick={() => setMoreOpen((v) => !v)}
          >
            <Icon name="more" size="sm" />更多
          </button>
        </div>
        {moreOpen && (
          <div className="more-menu">
            <button
              className="mi"
              disabled={!cards.some((c) => c.status === "done" && c.text)}
              onClick={() => { setMoreOpen(false); void speakResult(); }}
            >
              <Icon name="speaker" size="sm" />朗读译文
            </button>
            <button className="mi" onClick={() => { setMoreOpen(false); void stopSpeech(); }}>
              <Icon name="pause" size="sm" />停止朗读
            </button>
            <button
              className="mi"
              disabled={!source.trim() || !cards.some((c) => c.status === "done" && c.text)}
              onClick={() => { setMoreOpen(false); void addToAnki(); }}
            >
              <Icon name="bookmark" size="sm" />加入生词本
            </button>
            {actionPlugins.map((p) => (
              <button
                key={p.id}
                className="mi"
                disabled={!cards.some((c) => c.status === "done" && c.text)}
                onClick={() => { setMoreOpen(false); void runAction(p); }}
              >
                <Icon name="bolt" size="sm" />{p.name}
              </button>
            ))}
            <div className="sep" />
            <button className="mi" onClick={() => { setMoreOpen(false); searchWeb(); }}>
              <Icon name="search" size="sm" />搜索选中文本
            </button>
            <button className="mi" onClick={() => { setMoreOpen(false); openInBrowser(); }}>
              <Icon name="globe" size="sm" />在浏览器打开
            </button>
            <button
              className="mi"
              disabled={busy || !source}
              onClick={() => { setMoreOpen(false); void doTranslate(source); }}
            >
              <Icon name="refresh" size="sm" />重新翻译
            </button>
          </div>
        )}
        <div className="pop-status">
          <span className="dot" />
          {busy ? "流式输出中" : "就绪"}，Esc 关闭，拖动标题栏移动
        </div>
      </div>
    </div>
  );
}
