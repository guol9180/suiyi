// S0.6 设置页：服务配置（对应设计稿④）
import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import "./Settings.css";
import {
  ankiStatus,
  createSamplePlugin,
  clearHistory,
  deleteHistory,
  deleteApiKey,
  deleteService,
  getApiKey,
  hotkeyStatus,
  listHistory,
  listPlugins,
  listServices,
  listSpeechVoices,
  pauseSpeaking,
  pluginsDirPath,
  reorderServices,
  resetHotkeys,
  retryHotkeys,
  resumeSpeaking,
  setHotkey,
  speakText,
  speechState,
  stopSpeaking,
  saveService,
  saveSettings,
  setApiKey,
  setPluginEnabled,
  testConnection,
  type ConnectionTest,
  type HotkeyStatus,
} from "../api";
import { wordbookAdd, wordbookList, wordbookRemove, wordbookSync } from "../api";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";
import { SpeechBar } from "../components/SpeechBar";
import { getVersion, lastOf, statusCodeOf, subscribe } from "../lastResult";
import {
  DEFAULT_PROMPT,
  EMPTY_SERVICE,
  HISTORY_KIND_LABELS,
  PLUGIN_KIND_LABELS,
  PROTOCOL_LABELS,
  serviceMeta,
  TARGET_LANGS,
  type HistoryEntry,
  type HistoryKind,
  type AnkiStatus,
  type PluginInfo,
  type Protocol,
  type ResultType,
  type ServiceConfig,
  type ServicesFile,
  type SpeechState,
  type SpeechVoice,
  type WordbookView,
} from "../types";

const SIDEBAR_MAIN = ["通用", "热键", "服务配置", "历史记录", "生词本", "语音合成", "插件"];

type Page = "general" | "hotkeys" | "services" | "history" | "wordbook" | "speech" | "plugins";
const SIDEBAR_PAGES: Record<string, Page> = {
  通用: "general",
  热键: "hotkeys",
  服务配置: "services",
  历史记录: "history",
  生词本: "wordbook",
  语音合成: "speech",
  插件: "plugins",
};

/** 三个入口的热键，id 与 src-tauri/src/hotkeys.rs 的 ENTRIES 一一对应 */
const HOTKEYS: { id: string; name: string; accel: string; desc: string }[] = [
  { id: "selection", name: "划词翻译", accel: "alt+d", desc: "取选中文字并在光标处弹出翻译窗" },
  { id: "screenshot", name: "截图识别", accel: "alt+s", desc: "冻结鼠标所在显示器，框选后离线识别" },
  { id: "input", name: "输入框转译", accel: "alt+t", desc: "翻译当前输入框内容并原位写回" },
];
/** 尚未实现的入口收进「即将推出」分组并带里程碑锁标，不再平铺成一排空壳 */
const SIDEBAR_SOON: { name: string; milestone: string }[] = [];
const SIDEBAR_TAIL = ["关于"];

/** 没有朗读会话时的状态，字段与后端 SpeechState 一致 */
const IDLE_SPEECH: SpeechState = {
  active: false,
  playing: false,
  paused: false,
  positionMs: 0,
  durationMs: 0,
  rate: 1,
  voice: "",
  text: "",
};
const DEFAULT_PREVIEW = "这是一段试听文本，用来确认音色与语速。";

/** 历史时间显示成「今天 14:22」这种更好读的形式 */
/** 把 "alt+d" 这种加速度拆成键帽上该显示的字符 */
function accelKeys(accel: string): string[] {
  const named: Record<string, string> = { ctrl: "Ctrl", alt: "Alt", shift: "Shift", super: "Win" };
  return accel.split("+").map((p) => named[p] ?? p.toUpperCase());
}

/**
 * 热键被别的程序占用时给一个替代组合。
 * 默认加 Ctrl+Alt；连 Ctrl+Alt 都占了就换成 Ctrl+Shift，规则简单可预期。
 */
function suggestionFor(accel: string): string {
  const key = accel.split("+").pop() ?? "";
  if (accel.includes("ctrl") && accel.includes("alt")) return `ctrl+shift+${key}`;
  return `ctrl+alt+${key}`;
}

function formatTime(ms: number): string {
  const d = new Date(ms);
  const today = new Date();
  const yesterday = new Date(today.getTime() - 86_400_000);
  const day =
    d.toDateString() === today.toDateString()
      ? "今天"
      : d.toDateString() === yesterday.toDateString()
        ? "昨天"
        : `${d.getMonth() + 1} 月 ${d.getDate()} 日`;
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${day} ${hh}:${mm}`;
}

function Toggle(props: { on: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={props.on}
      className="tgl"
      onClick={props.onClick}
      aria-label="启用开关"
    />
  );
}

export default function SettingsPage() {
  const [file, setFile] = useState<ServicesFile | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<ServiceConfig | null>(null);
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [hasKey, setHasKey] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [settings, setSettings] = useState({
    concurrency: 2,
    timeoutSecs: 15,
    inputTargetLang: "English",
    ankiUrl: "http://127.0.0.1:8765",
    ankiDeck: "随译",
    speechVoice: "",
    speechRate: 1,
  });
  const [anki, setAnki] = useState<AnkiStatus | null>(null);
  const [ankiTesting, setAnkiTesting] = useState(false);
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [pluginsPath, setPluginsPath] = useState("");
  const [page, setPage] = useState<Page>("services");
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<ConnectionTest | null>(null);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  /** 三个全局热键的注册状态，切到热键页时刷新 */
  const [hotkeys, setHotkeys] = useState<HotkeyStatus[]>([]);
  /** 正在朗读的那条记录；null 表示没有在朗读 */
  const [speakingId, setSpeakingId] = useState<number | null>(null);
  /** 换服务重译的临时结果，只留在界面上，不改动历史 */
  const [retrans, setRetrans] = useState<{ id: number; service: string; text: string } | null>(null);
  /** 正在录制哪个动作的热键；null 表示没有在录制 */
  const [recording, setRecording] = useState<string | null>(null);
  const [histQuery, setHistQuery] = useState("");
  const [histKind, setHistKind] = useState<HistoryKind | "">("");
  /** 按服务筛选；空串表示全部 */
  const [histService, setHistService] = useState("");
  /** 时间范围：近 7 天，或者全部 */
  const [histRange, setHistRange] = useState<"all" | "7d">("all");
  const [histId, setHistId] = useState<number | null>(null);
  /** 生词本：列表与待同步计数来自同一次后端调用，不会出现两边对不上 */
  const [book, setBook] = useState<WordbookView | null>(null);
  const [bookBusy, setBookBusy] = useState(false);
  /** 手动添加词条的输入 */
  const [newTerm, setNewTerm] = useState("");
  const [newMeaning, setNewMeaning] = useState("");
  const [adding, setAdding] = useState(false);
  /** 朗读会话的实时状态（进度、时长都来自系统）与系统可用音色 */
  const [speech, setSpeech] = useState<SpeechState>(IDLE_SPEECH);
  const [voices, setVoices] = useState<SpeechVoice[]>([]);
  const [preview, setPreview] = useState(DEFAULT_PREVIEW);
  const [pickingVoice, setPickingVoice] = useState(false);

  /** 服务与时间两个维度在本地过滤，不用再跑一次后端查询 */
  const visibleHistory = history.filter((h) => {
    if (histService && h.serviceName !== histService) return false;
    if (histRange === "7d" && Date.now() - h.createdAt > 7 * 86_400_000) return false;
    return true;
  });

  const selectedEntry =
    visibleHistory.find((h) => h.id === histId) ?? visibleHistory[0] ?? null;

  const sortServices = (list: ServiceConfig[]) => [...list].sort((a, b) => a.order - b.order);

  const refresh = useCallback(async () => {
    try {
      const f = await listServices();
      setFile(f);
      setSettings({
        concurrency: f.concurrency,
        timeoutSecs: f.timeoutSecs,
        inputTargetLang: f.inputTargetLang || "English",
        ankiUrl: f.ankiUrl || "http://127.0.0.1:8765",
        ankiDeck: f.ankiDeck || "随译",
        speechVoice: f.speechVoice || "",
        speechRate: f.speechRate || 1,
      });
      const sorted = sortServices(f.services);
      setSelectedId((cur) => cur ?? sorted[0]?.id ?? null);
      return sorted;
    } catch (e) {
      setError(String(e));
      return [];
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const loadHistory = useCallback(async () => {
    try {
      const rows = await listHistory({
        query: histQuery,
        kind: histKind || undefined,
        limit: 200,
      });
      setHistory(rows);
      setHistId((cur) => (rows.some((r) => r.id === cur) ? cur : (rows[0]?.id ?? null)));
    } catch (e) {
      setError(String(e));
    }
  }, [histQuery, histKind]);

  useEffect(() => {
    if (page === "history") void loadHistory();
  }, [page, loadHistory]);

  const loadHotkeys = useCallback(async () => {
    try {
      setHotkeys(await hotkeyStatus());
    } catch {
      // 读不到状态时不显示徽标，不影响其余设置项
    }
  }, []);

  useEffect(() => {
    if (page === "hotkeys") void loadHotkeys();
  }, [page, loadHotkeys]);

  /** 生词本页：进页面时同时刷新列表与 Anki 连接状态 */
  const loadBook = useCallback(async () => {
    try {
      setBook(await wordbookList());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    if (page === "wordbook") {
      void loadBook();
      void handleAnkiTest();
    }
  }, [page, loadBook]);

  /**
   * 朗读进行中按 300ms 轮询一次后端。进度是 MCI 的真实位置，
   * 播完了后端会自己把会话清空，这里跟着回到静止态。
   */
  useEffect(() => {
    if (!speech.active) return;
    const timer = window.setInterval(() => {
      void speechState()
        .then(setSpeech)
        .catch(() => setSpeech(IDLE_SPEECH));
    }, 300);
    return () => window.clearInterval(timer);
  }, [speech.active]);

  useEffect(() => {
    if (page !== "speech") return;
    void listSpeechVoices()
      .then(setVoices)
      .catch(() => setVoices([]));
    void speechState()
      .then(setSpeech)
      .catch(() => setSpeech(IDLE_SPEECH));
  }, [page]);

  // 录制中：抓下一次按键组合，Esc 取消。只按修饰键时继续等。
  useEffect(() => {
    if (!recording) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setRecording(null);
        return;
      }
      const mods: string[] = [];
      if (e.ctrlKey) mods.push("ctrl");
      if (e.altKey) mods.push("alt");
      if (e.shiftKey) mods.push("shift");
      if (e.metaKey) mods.push("super");
      const named: Record<string, string> = {
        " ": "space",
        ArrowUp: "up",
        ArrowDown: "down",
        ArrowLeft: "left",
        ArrowRight: "right",
        Enter: "enter",
        Tab: "tab",
        Backspace: "backspace",
        Delete: "delete",
      };
      const key = e.key.length === 1 ? e.key.toLowerCase() : named[e.key];
      if (!key) return; // 只按了修饰键
      if (mods.length === 0) {
        setError("至少要带一个修饰键，否则会和正常打字冲突");
        setRecording(null);
        return;
      }
      const accel = [...mods, key].join("+");
      const target = recording;
      setRecording(null);
      void setHotkey(target, accel)
        .then(setHotkeys)
        .catch((err) => setError(String(err)));
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [recording]);

  // 翻译页会把每个服务最近一次结果写进 lastResult，这里跟着刷新
  useSyncExternalStore(subscribe, getVersion);

  const loadPlugins = useCallback(async () => {
    try {
      setPlugins(await listPlugins());
      setPluginsPath(await pluginsDirPath());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    if (page === "plugins") void loadPlugins();
  }, [page, loadPlugins]);

  async function handleTogglePlugin(id: string, enabled: boolean) {
    try {
      setPlugins(await setPluginEnabled(id, enabled));
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleCreateSample() {
    try {
      setPlugins(await createSamplePlugin());
      flash("示例插件已生成，改改就能用");
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleClearHistory() {
    if (!window.confirm("清空全部翻译历史？此操作不可撤销。")) return;
    try {
      await clearHistory();
      setHistory([]);
      setHistId(null);
      flash("历史已清空");
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleDeleteEntry(id: number) {
    try {
      await deleteHistory(id);
      await loadHistory();
    } catch (e) {
      setError(String(e));
    }
  }

  /** 朗读这条记录的译文；再点一次停止 */
  async function handleSpeak(entry: HistoryEntry) {
    if (speakingId === entry.id) {
      await stopSpeaking().catch(() => {});
      setSpeakingId(null);
      return;
    }
    const text = entry.translated || entry.source;
    if (!text) return;
    try {
      await speakText(text);
      setSpeakingId(entry.id);
    } catch (e) {
      setError(String(e));
    }
  }

  /** 把这条记录加进 Anki 生词本：正面原文、背面译文 */
  async function handleAddToAnki(entry: HistoryEntry) {
    try {
      // 走生词本：先落本地，Anki 没开也留得住，之后能批量补发
      const r = await wordbookAdd(entry.source, entry.translated || entry.error || "", {
        source: entry.kind,
      });
      setBook(r);
      if (r.duplicate) flash(r.error ?? "这条已经在生词本里了");
      else if (r.synced) flash("已加入生词本并同步到 Anki");
      else flash("已加入生词本，等 Anki 可用时自动同步");
    } catch (e) {
      setError(String(e));
    }
  }

  /** 换个服务重译这条的原文，结果只显示在详情里，不写回历史 */
  async function handleRetranslate(entry: HistoryEntry, serviceId: string) {
    const svc = file?.services.find((s) => s.id === serviceId);
    if (!svc) return;
    try {
      const r = await invoke<{ text: string }>("translate_text", {
        serviceId,
        text: entry.source,
        from: "自动检测",
        to: "简体中文",
        kind: "manual",
      });
      setRetrans({ id: entry.id, service: svc.name, text: r.text });
    } catch (e) {
      setError(String(e));
    }
  }

  // 选中变化：加载草稿 + 查询密钥状态（不取明文）
  useEffect(() => {
    if (!file || !selectedId) {
      setDraft(null);
      return;
    }
    const s = file.services.find((x) => x.id === selectedId);
    if (!s) {
      setDraft(null);
      return;
    }
    setDraft({ ...s });
    setApiKeyInput("");
    setTestResult(null);
    void getApiKey(s.id, true)
      .then((v) => setHasKey(v !== null))
      .catch(() => setHasKey(false));
  }, [file, selectedId]);

  const services = file ? sortServices(file.services) : [];

  function flash(msg: string) {
    setNotice(msg);
    setError("");
    window.setTimeout(() => setNotice(""), 2500);
  }

  async function handleSave() {
    if (!draft) return;
    if (!draft.name.trim()) return setError("名称不能为空");
    if (!draft.baseUrl.trim()) return setError("Base URL 不能为空");
    try {
      const list = await saveService(draft);
      if (apiKeyInput.trim()) {
        await setApiKey(draft.id, apiKeyInput.trim());
        setApiKeyInput("");
      }
      setFile((f) => (f ? { ...f, services: list } : f));
      setSelectedId(draft.id || list[list.length - 1]?.id || null);
      flash("已保存");
    } catch (e) {
      setError(String(e));
    }
  }

  /** 探测服务连通性：确认 Key、网关与模型三件事 */
  async function handleTest() {
    if (!draft?.id) return;
    setTesting(true);
    setTestResult(null);
    try {
      setTestResult(await testConnection(draft.id));
    } catch (e) {
      setTestResult({ ok: false, elapsedMs: 0, models: [], error: String(e) });
    } finally {
      setTesting(false);
    }
  }

  async function handleDelete() {
    if (!draft || !draft.id) return;
    if (!window.confirm(`确认删除服务「${draft.name}」？其 API Key 也会一并清除。`)) return;
    try {
      const list = await deleteService(draft.id);
      setFile((f) => (f ? { ...f, services: list } : f));
      setSelectedId(sortServices(list)[0]?.id ?? null);
      flash("已删除");
    } catch (e) {
      setError(String(e));
    }
  }

  async function toggleEnabled(s: ServiceConfig) {
    try {
      // 插件服务的启停归插件页管，不能写进 services.json
      if (s.pluginId) {
        setPlugins(await setPluginEnabled(s.pluginId, !s.enabled));
        return;
      }
      const list = await saveService({ ...s, enabled: !s.enabled });
      setFile((f) => (f ? { ...f, services: list } : f));
    } catch (e) {
      setError(String(e));
    }
  }

  async function move(s: ServiceConfig, dir: -1 | 1) {
    const ids = services.map((x) => x.id);
    const i = ids.indexOf(s.id);
    const j = i + dir;
    if (j < 0 || j >= ids.length) return;
    [ids[i], ids[j]] = [ids[j], ids[i]];
    try {
      const list = await reorderServices(ids);
      setFile((f) => (f ? { ...f, services: list } : f));
    } catch (e) {
      setError(String(e));
    }
  }

  async function saveGlobal(next = settings) {
    try {
      await saveSettings(next);
      flash("全局设置已保存");
    } catch (e) {
      setError(String(e));
    }
  }

  /** 探测 Anki：未装 AnkiConnect 或 Anki 没开时给出可执行的提示 */
  async function handleAnkiTest() {
    setAnkiTesting(true);
    try {
      setAnki(await ankiStatus());
    } catch (e) {
      setAnki({ available: false, version: null, deckExists: false, error: String(e) });
    } finally {
      setAnkiTesting(false);
    }
  }

  /** 批量补发待同步的词条；一条都没成功时把原因说清楚 */
  async function handleBookSync() {
    setBookBusy(true);
    try {
      const r = await wordbookSync();
      setBook(r);
      if (r.stats.pending === 0) flash(`已全部同步，共 ${r.stats.total} 条`);
      else if (r.error) setError(`同步未完成：${r.error}`);
      else flash(`已同步，还有 ${r.stats.pending} 条待补发`);
      void handleAnkiTest();
    } catch (e) {
      setError(String(e));
    } finally {
      setBookBusy(false);
    }
  }

  async function handleBookAdd() {
    const term = newTerm.trim();
    const meaning = newMeaning.trim();
    if (!term || !meaning) {
      setError("词条和释义都要填");
      return;
    }
    try {
      const r = await wordbookAdd(term, meaning, { source: "manual" });
      setBook(r);
      setNewTerm("");
      setNewMeaning("");
      setAdding(false);
      if (r.duplicate) flash(r.error ?? "这个词条已经在生词本里了");
      else if (r.synced) flash("已加入并同步到 Anki");
      else flash("已加入生词本，等 Anki 可用时自动同步");
      void handleAnkiTest();
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleBookRemove(id: number) {
    try {
      setBook(await wordbookRemove(id));
      flash("已从生词本移除");
    } catch (e) {
      setError(String(e));
    }
  }

  /** 采用建议的替代热键；如果建议的组合也被占用就说清楚，不让用户以为改成功了 */
  async function useSuggestedHotkey(id: string, accel: string) {
    try {
      const next = await setHotkey(id, accel);
      setHotkeys(next);
      if (next.find((h) => h.id === id)?.registered) {
        flash(`已改为 ${accelKeys(accel).join(" + ")}`);
      } else {
        setError("建议的组合也被占用了，换一个再试");
      }
    } catch (e) {
      setError(String(e));
    }
  }

  /** 按当前音色与语速朗读一段文本 */
  async function startSpeech(text: string) {
    const body = text.trim();
    if (!body) {
      setError("先写一段试听文本");
      return;
    }
    try {
      setSpeech(
        await speakText(body, {
          rate: settings.speechRate,
          voice: settings.speechVoice,
        }),
      );
    } catch (e) {
      setError(String(e));
    }
  }

  /** 播放键：没在朗读就开一段，正在朗读就暂停，暂停中就续播 */
  async function handleSpeechToggle() {
    if (!speech.active) {
      await startSpeech(preview);
      return;
    }
    try {
      setSpeech(speech.playing ? await pauseSpeaking() : await resumeSpeaking());
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleSpeechStop() {
    try {
      setSpeech(await stopSpeaking());
    } catch {
      // 本来就没在播放，停止失败不需要打扰用户
    }
  }

  /** 改语速：先存设置；正在朗读就按新语速重来一段，否则改了听不出区别 */
  async function handleSpeechRate(rate: number) {
    const next = { ...settings, speechRate: rate };
    setSettings(next);
    void saveGlobal(next);
    if (speech.active) await startSpeech(speech.text || preview);
  }

  async function handleSpeechVoice(voiceId: string) {
    const next = { ...settings, speechVoice: voiceId };
    setSettings(next);
    void saveGlobal(next);
    setPickingVoice(false);
    if (speech.active) await startSpeech(speech.text || preview);
  }

  function clearKey() {
    if (!draft?.id) return;
    void deleteApiKey(draft.id)
      .then(() => {
        setHasKey(false);
        flash("已清除该服务密钥");
      })
      .catch((e) => setError(String(e)));
  }

  return (
    <div className="set-body">
      {/* ---------- 侧边栏 ---------- */}
      <aside className="side">
        {SIDEBAR_MAIN.map((item) => {
          const target = SIDEBAR_PAGES[item];
          return (
            <div
              key={item}
              className={`side-item${page === target ? " on" : ""}`}
              onClick={() => setPage(target)}
            >
              {item}
            </div>
          );
        })}
        {/* 全都落地之后这一组自然消失，不留一个空标题 */}
        {SIDEBAR_SOON.length > 0 && (
          <>
            <div className="side-group">即将推出</div>
            {SIDEBAR_SOON.map(({ name, milestone }) => (
              <div key={name} className="side-item soon" title={`${milestone} 里程碑开放`}>
                {name}
                <span className="lk"><Icon name="lock" size="sm" />{milestone}</span>
              </div>
            ))}
          </>
        )}
        {SIDEBAR_TAIL.map((item) => (
          <div key={item} className="side-item">{item}</div>
        ))}
        <div className="side-foot">
          随译 v{__APP_VERSION__}
          <br />
          译文由你配置的服务提供
        </div>
      </aside>

      {/* ---------- 主区 ---------- */}
      <div className="main">
        {/* 两栏包一层：卡片高度跟着内容走，内容不满时至少占满窗口高度 */}
        <div className="cols">
        {page === "general" && (
          <div className="panel" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <div className="card">
              <div className="card-head">
                <span className="hname">输入框转译</span>
                <span className="m">Alt + T</span>
              </div>
              <div className="f" style={{ maxWidth: 280 }}>
                <label>目标语言</label>
                <select
                  className="inp"
                  value={settings.inputTargetLang}
                  onChange={(e) => {
                    const next = { ...settings, inputTargetLang: e.target.value };
                    setSettings(next);
                    void saveGlobal(next);
                  }}
                >
                  {TARGET_LANGS.map((l) => (
                    <option key={l} value={l}>{l}</option>
                  ))}
                </select>
              </div>
              <div className="thint">
                按下 Alt+T 会读取当前输入框内容，翻译成该语言后原位写回。来源语言始终自动检测。
              </div>
            </div>

            <div className="card">
              <div className="card-head">
                <span className="hname">Anki 生词本</span>
                <span className="m">需要安装并启动 AnkiConnect 插件</span>
              </div>
              <div className="row2">
                <div className="f">
                  <label>AnkiConnect 地址</label>
                  <input
                    className="inp mono"
                    value={settings.ankiUrl}
                    onChange={(e) => setSettings((s) => ({ ...s, ankiUrl: e.target.value }))}
                    onBlur={() => void saveGlobal()}
                  />
                </div>
                <div className="f">
                  <label>牌组</label>
                  <input
                    className="inp"
                    value={settings.ankiDeck}
                    onChange={(e) => setSettings((s) => ({ ...s, ankiDeck: e.target.value }))}
                    onBlur={() => void saveGlobal()}
                  />
                </div>
              </div>
              <div className="testbox" style={{ marginBottom: 0 }}>
                <div className="trow">
                  <button className="btn mini" disabled={ankiTesting} onClick={() => void handleAnkiTest()}>
                    <Icon name="bolt" size="sm" />{ankiTesting ? "测试中" : "测试连接"}
                  </button>
                  {ankiTesting && <span className="spin" />}
                  {anki && anki.available && (
                    <>
                      <span className="chip ok"><Icon name="check" size="sm" />Anki 已连接</span>
                      {anki.version != null && <span className="chip mile mini">AnkiConnect v{anki.version}</span>}
                      <span className="t-cap">
                        {anki.deckExists ? `牌组「${settings.ankiDeck}」已存在` : "牌组将在首次添加时自动创建"}
                      </span>
                    </>
                  )}
                </div>
                {anki && !anki.available && (
                  <div className="terr">
                    <Icon name="alert" size="sm" />
                    <span>
                      连不上 Anki。请确认 Anki 正在运行，且已安装 AnkiConnect 插件（工具 → 插件 → 获取插件 → 代码 2055492159）。
                      {anki.error ? ` 详情：${anki.error}` : ""}
                    </span>
                  </div>
                )}
              </div>
            </div>
          </div>
        )}

        {page === "hotkeys" && (
          <div className="card panel">
            <div className="card-head">
              <span className="hname">全局热键</span>
              <span className="m">自定义录制后续开放</span>
            </div>

            {hotkeys.some((h) => !h.registered) && (
              <div className="hkwarn">
                <Icon name="alert" size="sm" />
                <span>
                  {hotkeys
                    .filter((h) => !h.registered)
                    .map((h) => h.label)
                    .join("、")}
                  的组合已被其他程序占用，按下去不会生效。关掉占用它的程序后点右侧重试。
                </span>
                <button
                  className="btn mini"
                  onClick={() => void retryHotkeys().then(setHotkeys)}
                >
                  <Icon name="refresh" size="sm" />重试注册
                </button>
              </div>
            )}

            {HOTKEYS.map((h) => {
              const st = hotkeys.find((x) => x.id === h.id);
              const keys = accelKeys(st?.accelerator ?? h.accel);
              return (
                <div className="hkrow" key={h.id}>
                  <span className="n">{h.name}</span>
                  <span className="m">{h.desc}</span>
                  {st && (
                    <span className={`chip mini ${st.registered ? "ok" : "warn"}`}>
                      {st.registered ? "已启用" : "已被占用"}
                    </span>
                  )}
                  <span className="key">
                    {keys.map((k, i) => (
                      <span className="kbd" key={i}>{k}</span>
                    ))}
                  </span>
                  {recording === h.id ? (
                    <span className="hk-rec">按下新组合，Esc 取消</span>
                  ) : (
                    <button className="btn mini" onClick={() => setRecording(h.id)}>录制</button>
                  )}
                  {st && !st.registered && (
                    <span className="hk-suggest">
                      <Icon name="alert" size="sm" />
                      已被其他程序占用，建议改用
                      {accelKeys(suggestionFor(st.accelerator || h.accel)).map((k, i) => (
                        <span className="kbd" key={i}>{k}</span>
                      ))}
                      <button
                        className="btn mini"
                        onClick={() =>
                          void useSuggestedHotkey(h.id, suggestionFor(st.accelerator || h.accel))
                        }
                      >
                        采用建议
                      </button>
                    </span>
                  )}
                </div>
              );
            })}

            <div className="hk-foot">
              <span className="thint">
                热键在应用启动时逐个注册，被占用的会在后台补注册；录制时至少要带一个修饰键。
              </span>
              {hotkeys.some((h) => h.custom) && (
                <button className="btn mini" onClick={() => void resetHotkeys().then(setHotkeys)}>
                  <Icon name="refresh" size="sm" />恢复默认热键
                </button>
              )}
            </div>
          </div>
        )}

        {page === "history" && (
          <div className="card panel wide">
            <div className="card-head">
              <span className="hname">历史记录</span>
              <span className="m">保留最近 2000 条</span>
              <span style={{ flex: 1 }} />
              <button className="btn mini" onClick={() => void loadHistory()}>
                <Icon name="refresh" size="sm" />刷新
              </button>
              <button
                className="btn danger mini"
                disabled={history.length === 0}
                onClick={() => void handleClearHistory()}
              >
                <Icon name="trash" size="sm" />清空
              </button>
            </div>

            <div className="hist-tools">
              <span className="search">
                <Icon name="search" size="sm" />
                <input
                  className="inp"
                  placeholder="搜索原文或译文…"
                  value={histQuery}
                  onChange={(e) => setHistQuery(e.target.value)}
                />
              </span>
              <span className={`chip${histKind === "" ? " acc" : ""}`} onClick={() => setHistKind("")}>
                全部
              </span>
              {(Object.keys(HISTORY_KIND_LABELS) as HistoryKind[]).map((k) => (
                <span
                  key={k}
                  className={`chip${histKind === k ? " acc" : ""}`}
                  onClick={() => setHistKind(k)}
                >
                  {HISTORY_KIND_LABELS[k]}
                </span>
              ))}
              <span className="hist-sep" />
              <span
                className={`chip${histService === "" ? " acc" : ""}`}
                onClick={() => setHistService("")}
              >
                全部服务
              </span>
              {Array.from(new Set(history.map((h) => h.serviceName).filter(Boolean))).map((name) => (
                <span
                  key={name}
                  className={`chip${histService === name ? " acc" : ""}`}
                  onClick={() => setHistService(name)}
                >
                  {name}
                </span>
              ))}
              <span className="hist-sep" />
              <span
                className={`chip${histRange === "all" ? " acc" : ""}`}
                onClick={() => setHistRange("all")}
              >
                全部时间
              </span>
              <span
                className={`chip${histRange === "7d" ? " acc" : ""}`}
                onClick={() => setHistRange("7d")}
              >
                近 7 天
              </span>
            </div>

            <div className="hist">
              <div className="hist-list">
                {visibleHistory.length === 0 && (
                  <div className="empty-hint">
                    {histQuery || histKind || histService || histRange === "7d"
                      ? "没有匹配的记录"
                      : "还没有翻译记录"}
                  </div>
                )}
                {visibleHistory.map((h) => (
                  <div
                    key={h.id}
                    className={`hrow${selectedEntry?.id === h.id ? " on" : ""}`}
                    onClick={() => setHistId(h.id)}
                  >
                    <div className="hl1">
                      <span className="t">{h.source}</span>
                      {h.serviceName && <span className="chip acc mini">{h.serviceName}</span>}
                    </div>
                {/* 第二行拆两段：摘要可截断，时间与耗时固定不截断 */}
                <div className={`hl2${h.ok ? "" : " err"}`}>
                  <span className="sum">{h.translated || h.error || "—"}</span>
                  <span className="meta">
                    <span>{HISTORY_KIND_LABELS[h.kind] ?? h.kind}</span>
                    <span>{formatTime(h.createdAt)}</span>
                    {h.ok && h.elapsedMs > 0 && <span>{(h.elapsedMs / 1000).toFixed(1)}s</span>}
                  </span>
                    </div>
                  </div>
                ))}
              </div>

              <div className="hist-detail">
                {selectedEntry ? (
                  <>
                    <div className="lab">原文</div>
                    <div className="card" style={{ padding: "10px 12px" }}>
                      <div className="t-body">{selectedEntry.source}</div>
                    </div>
                    <div className="lab">
                      译文{selectedEntry.serviceName ? `（${selectedEntry.serviceName}）` : ""}
                    </div>
                    <div className="card" style={{ padding: "10px 12px" }}>
                      <div className={selectedEntry.ok ? "t-body" : "t-body terr"}>
                        {selectedEntry.translated || selectedEntry.error}
                      </div>
                    </div>
                    <div className="hmeta">
                      <span>{formatTime(selectedEntry.createdAt)}</span>
                      {selectedEntry.ok && <span>{(selectedEntry.elapsedMs / 1000).toFixed(1)}s</span>}
                    </div>
                    <div className="hact">
                      <button
                        className="btn primary mini"
                        disabled={!selectedEntry.translated}
                        onClick={() => void navigator.clipboard.writeText(selectedEntry.translated)}
                      >
                        <Icon name="copy" size="sm" />复制译文
                      </button>
                      <button
                        className="btn mini"
                        disabled={!selectedEntry.translated}
                        onClick={() => void handleAddToAnki(selectedEntry)}
                      >
                        <Icon name="bookmark" size="sm" />生词本
                      </button>
                      <button
                        className="btn mini"
                        disabled={!(selectedEntry.translated || selectedEntry.source)}
                        onClick={() => void handleSpeak(selectedEntry)}
                      >
                        <Icon name={speakingId === selectedEntry.id ? "pause" : "speaker"} size="sm" />
                        {speakingId === selectedEntry.id ? "停止" : "朗读"}
                      </button>
                      <select
                        className="inp mini-sel"
                        value=""
                        onChange={(e) => {
                          if (e.target.value) void handleRetranslate(selectedEntry, e.target.value);
                        }}
                      >
                        <option value="">换服务重译…</option>
                        {(file?.services ?? [])
                          .filter((s) => s.enabled && s.kind === "translation")
                          .filter((s) => s.name !== selectedEntry.serviceName)
                          .map((s) => (
                            <option key={s.id} value={s.id}>{s.name}</option>
                          ))}
                      </select>
                      <button
                        className="btn mini"
                        onClick={() => void handleDeleteEntry(selectedEntry.id)}
                      >
                        <Icon name="trash" size="sm" />删除这条
                      </button>
                    </div>
                    {retrans && retrans.id === selectedEntry.id && (
                      <div className="retrans">
                        <div className="lab">换服务重译（{retrans.service}）</div>
                        <div className="t-body">{retrans.text}</div>
                      </div>
                    )}
                  </>
                ) : (
                  <div className="empty-hint">← 选择左侧记录查看详情</div>
                )}
              </div>
            </div>
          </div>
        )}

        {page === "wordbook" && (
          <div className="panel" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <div className="card">
              <div className="card-head">
                <span className="hname">生词本</span>
                {anki?.available ? (
                  <span className="chip ok mini">
                    Anki 已连接{anki.version ? ` · v${anki.version}` : ""}
                  </span>
                ) : ankiTesting ? (
                  <span className="chip mini">
                    <span className="spin" />
                    正在检测 Anki
                  </span>
                ) : (
                  <span className="chip warn mini">Anki 未连接</span>
                )}
                <span className="m">共 {book?.stats.total ?? 0} 条</span>
                <span style={{ flex: 1 }} />
                <button className="btn mini" disabled={bookBusy} onClick={() => void handleBookSync()}>
                  <Icon name="refresh" size="sm" />
                  同步
                </button>
              </div>

              {!book || book.entries.length === 0 ? (
                <div className="empty-hint">
                  还没有词条。翻译时点「生词本」，或在下面手动添加。
                </div>
              ) : (
                book.entries.map((w) => (
                  <div className="wrow" key={w.id}>
                    <span className={`dot${w.syncedAt ? "" : " warn"}`} />
                    <span className="wt">
                      <b>{w.term}</b>
                      {w.reading && <span className="rd">{w.reading}</span>}
                    </span>
                    <span className="wm" title={w.meaning}>
                      {w.meaning} · {formatTime(w.createdAt)}
                    </span>
                    {w.syncedAt ? (
                      <span className="chip ok mini">已同步</span>
                    ) : (
                      <span className="chip warn mini" title={w.lastError ?? undefined}>
                        待同步
                      </span>
                    )}
                    <button
                      className="wdel"
                      title="从生词本移除"
                      onClick={() => void handleBookRemove(w.id)}
                    >
                      <Icon name="trash" size="sm" />
                    </button>
                  </div>
                ))
              )}

              {adding && (
                <div className="wadd">
                  <input
                    className="inp"
                    placeholder="词条，如 retrieval"
                    value={newTerm}
                    onChange={(e) => setNewTerm(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") void handleBookAdd();
                    }}
                  />
                  <input
                    className="inp"
                    placeholder="释义，如 检索"
                    value={newMeaning}
                    onChange={(e) => setNewMeaning(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") void handleBookAdd();
                    }}
                  />
                  <button className="btn primary mini" onClick={() => void handleBookAdd()}>
                    加入
                  </button>
                  <button className="btn mini" onClick={() => setAdding(false)}>
                    取消
                  </button>
                </div>
              )}

              <div className="wfoot">
                {book && book.stats.pending > 0 ? (
                  <span className="chip warn mini">{book.stats.pending} 条待同步</span>
                ) : (
                  <span className="chip ok mini">全部已同步</span>
                )}
                <span style={{ flex: 1 }} />
                <button className="btn mini" onClick={() => setAdding((v) => !v)}>
                  <Icon name="plus" size="sm" />
                  手动添加词条
                </button>
              </div>
            </div>

            <div className="thint">
              Anki 未启动时自动排队，下次连接后批量补发。词条先存在本机，Anki 没开也不会丢。
            </div>
          </div>
        )}

        {page === "speech" && (
          <div className="panel mid" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <div className="card">
              <div className="card-head">
                <span className="hname">语音合成</span>
                <span className="chip mile mini">Windows 本地语音</span>
                <span className="m">离线，不消耗翻译额度</span>
                <span style={{ flex: 1 }} />
                <button className="btn mini" onClick={() => void startSpeech(preview)}>
                  <Icon name="speaker" size="sm" />
                  试听
                </button>
              </div>

              <SpeechBar
                state={speech}
                canPlay={preview.trim().length > 0}
                voiceLabel={
                  settings.speechVoice
                    ? (() => {
                        const v = voices.find((x) => x.id === settings.speechVoice);
                        return v ? `${v.name} · ${v.language}` : "已选音色不可用";
                      })()
                    : "系统默认"
                }
                onToggle={() => void handleSpeechToggle()}
                onStop={() => void handleSpeechStop()}
                onRate={(r) => void handleSpeechRate(r)}
                onPickVoice={() => setPickingVoice((v) => !v)}
              />

              {pickingVoice && (
                <div className="voicelist">
                  <div
                    className={`vrow${settings.speechVoice ? "" : " on"}`}
                    onClick={() => void handleSpeechVoice("")}
                  >
                    系统默认音色
                    <span className="m">跟随 Windows</span>
                  </div>
                  {voices.map((v) => (
                    <div
                      key={v.id}
                      className={`vrow${settings.speechVoice === v.id ? " on" : ""}`}
                      onClick={() => void handleSpeechVoice(v.id)}
                    >
                      {v.name}
                      <span className="m">{v.language}</span>
                    </div>
                  ))}
                  {voices.length === 0 && (
                    <div className="vrow" style={{ cursor: "default" }}>
                      系统里没有读到可用语音
                      <span className="m">在 Windows 设置里添加语音包</span>
                    </div>
                  )}
                </div>
              )}

              <div className="f" style={{ marginTop: 12 }}>
                <label>试听文本</label>
                <textarea
                  className="inp"
                  rows={2}
                  value={preview}
                  onChange={(e) => setPreview(e.target.value)}
                  placeholder="写一段用来试听音色与语速的文本"
                />
              </div>
            </div>

            <div className="thint">
              朗读用 Windows 自带语音，离线、不经过翻译链路、不消耗翻译额度。
              音色与语速存在本机，划词弹窗和历史记录里的朗读也按这一套走。
            </div>
          </div>
        )}

        {page === "plugins" && (
          <div className="card panel wide">
            <div className="card-head">
              <span className="hname">插件</span>
              <span className="m">每个子目录一个插件，入口是 manifest.json</span>
              <span style={{ flex: 1 }} />
              <button className="btn mini" onClick={() => void loadPlugins()}>
                <Icon name="refresh" size="sm" />刷新
              </button>
              <button className="btn primary mini" onClick={() => void handleCreateSample()}>
                <Icon name="plus" size="sm" />生成示例插件
              </button>
            </div>

            {pluginsPath && (
              <div className="plpath">
                <Icon name="info" size="sm" />
                <span>插件目录：<code>{pluginsPath}</code></span>
              </div>
            )}

            {plugins.length === 0 && (
              <div className="empty-hint">
                还没有插件。点「生成示例插件」会创建一个可用的翻译插件，改改就能变成自己的。
              </div>
            )}

            {plugins.map((p) => (
              <div className="plrow" key={p.dir}>
                <Toggle on={p.enabled} onClick={() => void handleTogglePlugin(p.id, !p.enabled)} />
                <span className="nm">
                  <b>{p.name}</b>
                  <span>
                    {p.ok
                      ? [
                          p.kind ? PLUGIN_KIND_LABELS[p.kind] : "",
                          p.permissions.length ? `权限：${p.permissions.join(" / ")}` : "无特殊权限",
                        ]
                          .filter(Boolean)
                          .join("，")
                      : p.error}
                  </span>
                </span>
                <span className={`chip ${p.ok ? "ok" : "err"} mini`}>
                  {p.ok ? "校验通过" : "有问题"}
                </span>
              </div>
            ))}

            <div className="thint">
              翻译类插件通过校验并启用后，会自动出现在服务列表里参与多服务对比。
              插件目前没有网络能力，只有申请了 clipboard 权限才能读写剪贴板。
            </div>
          </div>
        )}

        {page === "services" && (
          <>
        {/* 服务列表 */}
        <div className="card list-col">
          <div className="card-head">
            <b>翻译服务</b>
            <span className="m">上下调整顺序</span>
            <span style={{ flex: 1 }} />
            <button
              className="btn primary mini"
              onClick={() => {
                setDraft({ ...EMPTY_SERVICE });
                setSelectedId(null);
                setApiKeyInput("");
                setHasKey(false);
              }}
            >
              <Icon name="plus" size="sm" />添加服务
            </button>
          </div>

          {services.map((s) => (
            // 整行可点即选中编辑，不必每行再挂一个「编辑」按钮
            <div
              key={s.id}
              className={`svc${draft?.id === s.id ? " on" : ""}`}
              onClick={() => {
                setDraft({ ...s });
                setSelectedId(s.id);
              }}
            >
              <span className="arrows">
                {!s.pluginId && (
                  <>
                    <button className="mini-as-link" onClick={() => void move(s, -1)} title="上移"><Icon name="chev-up" size="sm" /></button>
                    <button className="mini-as-link" onClick={() => void move(s, 1)} title="下移"><Icon name="chev-down" size="sm" /></button>
                  </>
                )}
              </span>
              <Toggle on={s.enabled} onClick={() => void toggleEnabled(s)} />
              <span className="svc-name">
                <b>{s.name || "未命名服务"}</b>
                  <span className="meta">
                    <span className="model" title={serviceMeta(s)}>{serviceMeta(s)}</span>
                    {(() => {
                      const last = lastOf(s.id);
                      if (last && !last.ok) {
                        const code = statusCodeOf(last.error);
                        return (
                          <span className="chip err mini">上次失败{code ? ` ${code}` : ""}</span>
                        );
                      }
                      return s.enabled ? <span className="chip ok mini">已启用</span> : null;
                    })()}
                  </span>
              </span>
              {s.pluginId && <span className="chip acc mini">插件</span>}
            </div>
          ))}
          {services.length === 0 && (
            <div className="empty-hint">还没有服务，点右上角的「添加服务」开始配置。</div>
          )}

          <div className="fallback">
            <div className="fallback-head">性能与回退</div>
            <div className="row2" style={{ alignItems: "center" }}>
              <label className="g-label">
                并发数
                <input
                  className="inp"
                  type="number"
                  min={1}
                  max={8}
                  value={settings.concurrency}
                  onChange={(e) => setSettings((s) => ({ ...s, concurrency: Number(e.target.value) }))}
                  onBlur={() => void saveGlobal()}
                />
              </label>
              <label className="g-label">
                单服超时（秒）
                <input
                  className="inp"
                  type="number"
                  min={3}
                  max={120}
                  value={settings.timeoutSecs}
                  onChange={(e) => setSettings((s) => ({ ...s, timeoutSecs: Number(e.target.value) }))}
                  onBlur={() => void saveGlobal()}
                />
              </label>
            </div>
            失败按启用顺序回退到下一条，并发上限 8。单服务模式在通用设置中切换。
          </div>
        </div>

        {/* 编辑表单 */}
        <div className="card form-col">
          {!draft ? (
            <div className="empty-hint">← 选择左侧服务进行编辑，或添加新服务</div>
          ) : (
            <>
              <div className="card-head">
                <b>{draft.id ? `编辑服务：${draft.name}` : "添加服务"}</b>
                <span className="chip acc">{PROTOCOL_LABELS[draft.protocol]}</span>
                <span style={{ flex: 1 }} />
                {draft.id && (
                  <button className="btn danger mini" onClick={() => void handleDelete()}>
                    删除
                  </button>
                )}
              </div>

              <div className="row2">
                <div className="f">
                  <label>名称</label>
                  <input className="inp" value={draft.name}
                    onChange={(e) => setDraft({ ...draft, name: e.target.value })} placeholder="如：DeepSeek" />
                </div>
                <div className="f">
                  <label>协议</label>
                  <select className="inp" value={draft.protocol}
                    onChange={(e) => setDraft({ ...draft, protocol: e.target.value as Protocol })}>
                    {/* 只写协议名：端点写在下面的 Base URL 里，写全了在半栏宽度会被截断 */}
                    <option value="open_ai_compatible">OpenAI 兼容</option>
                    <option value="anthropic" disabled>Anthropic（开发中）</option>
                    <option value="gemini" disabled>Gemini（开发中）</option>
                  </select>
                </div>
              </div>

              <div className="f">
                <label>Base URL</label>
                <input className="inp mono" value={draft.baseUrl}
                  onChange={(e) => setDraft({ ...draft, baseUrl: e.target.value })}
                  placeholder="https://api.deepseek.com/v1" />
              </div>

              <div className="f">
                <label>
                  API Key（仅存于系统凭据管理器）
                  <span className="key-state">
                    {hasKey
                      ? <span className="chip ok"><Icon name="check" size="sm" />已保存</span>
                      : <span className="chip">未设置</span>}
                  </span>
                </label>
                <div className="keyrow">
                  <input className="inp mono" type="password" value={apiKeyInput}
                    onChange={(e) => setApiKeyInput(e.target.value)}
                    placeholder={hasKey ? "已保存，留空则不修改；输入新值则覆盖" : "sk-..."} />
                  {hasKey && (
                    <button className="btn mini" onClick={clearKey}>清除</button>
                  )}
                </div>
              </div>

              {/* 测试连接：Key 是否有效、网关是否可达、模型是否可见 */}
              <div className="testbox">
                <div className="trow">
                  <button
                    className="btn mini"
                    disabled={testing || !draft.id || !draft.baseUrl.trim()}
                    onClick={() => void handleTest()}
                  >
                    <Icon name="bolt" size="sm" />{testing ? "测试中" : "测试连接"}
                  </button>
                  {testing && <span className="spin" />}
                  {testResult?.ok && (
                    <>
                      <span className="chip ok"><Icon name="check" size="sm" />连接正常</span>
                      <span className="chip mile">{testResult.elapsedMs}ms</span>
                      <span className="t-cap">
                        {testResult.models.length === 0
                          ? "服务未返回模型列表"
                          : testResult.models.includes(draft.model)
                            ? `模型 ${draft.model} 可用`
                            : `返回 ${testResult.models.length} 个模型，未包含 ${draft.model}`}
                      </span>
                    </>
                  )}
                </div>
                {testResult && !testResult.ok && testResult.error && (
                  <div className="terr"><Icon name="alert" size="sm" />{testResult.error}</div>
                )}
                {!draft.id && <div className="thint">先保存服务，再测试连接</div>}
              </div>

              <div className="row2">
                <div className="f">
                  <label>模型（可手填）</label>
                  <input className="inp mono" value={draft.model}
                    onChange={(e) => setDraft({ ...draft, model: e.target.value })}
                    placeholder="deepseek-chat" />
                </div>
                <div className="f" style={{ flex: "0 0 44%" }}>
                  <label>结果类型</label>
                  <div className="seg">
                    {(["text", "dictionary"] as ResultType[]).map((t) => (
                      <span key={t} className={draft.resultType === t ? "on" : ""}
                        onClick={() => setDraft({ ...draft, resultType: t })}>
                        {t === "text" ? "纯文本" : "词典结构"}
                      </span>
                    ))}
                  </div>
                </div>
              </div>

              <div className="f">
                <label>Prompt 模板（{"{{from}} {{to}} {{text}}"} 自动注入）</label>
                <textarea className="inp mono" rows={3} value={draft.promptTemplate ?? ""}
                  onChange={(e) => setDraft({ ...draft, promptTemplate: e.target.value })}
                  placeholder={DEFAULT_PROMPT} />
              </div>

              <div className="row2">
                <div className="f">
                  <label>Temperature：{draft.temperature ?? 0.3}</label>
                  <input type="range" min={0} max={1} step={0.1} style={{ width: "100%" }}
                    value={draft.temperature ?? 0.3}
                    onChange={(e) => setDraft({ ...draft, temperature: Number(e.target.value) })} />
                </div>
                <div className="f">
                  <label>流式输出 (SSE)</label>
                  <Toggle on={draft.stream} onClick={() => setDraft({ ...draft, stream: !draft.stream })} />
                </div>
              </div>

              <div className="form-foot">
                <button className="btn primary" onClick={() => void handleSave()}>保存修改</button>
                <span className="sec-note"><Icon name="lock" size="sm" />密钥仅存于 Windows 凭据管理器</span>
              </div>
            </>
          )}
        </div>
          </>
        )}
        </div>
      </div>

      {/* ---------- 全局提示条 ---------- */}
      {(error || notice) && (
        <div className={`toast ${error ? "err" : "ok"}`}>{error || notice}</div>
      )}
    </div>
  );
}
