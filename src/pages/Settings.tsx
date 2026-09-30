// S0.6 设置页：服务配置（对应设计稿④）
import { useCallback, useEffect, useState } from "react";
import "./Settings.css";
import {
  ankiStatus,
  createSamplePlugin,
  clearHistory,
  deleteHistory,
  deleteApiKey,
  deleteService,
  getApiKey,
  listHistory,
  listPlugins,
  listServices,
  pluginsDirPath,
  reorderServices,
  saveService,
  saveSettings,
  setApiKey,
  setPluginEnabled,
  testConnection,
  type ConnectionTest,
} from "../api";
import { Icon } from "../components/Icon";
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
} from "../types";

const SIDEBAR_MAIN = ["通用", "热键", "服务配置", "历史记录", "插件"];

type Page = "general" | "hotkeys" | "services" | "history" | "plugins";
const SIDEBAR_PAGES: Record<string, Page> = {
  通用: "general",
  热键: "hotkeys",
  服务配置: "services",
  历史记录: "history",
  插件: "plugins",
};

/** 已注册的全局热键，与 src-tauri/src/lib.rs 的 with_shortcuts 一一对应 */
const HOTKEYS: { name: string; key: string; desc: string }[] = [
  { name: "划词翻译", key: "D", desc: "取选中文字并在光标处弹出翻译窗" },
  { name: "截图识别", key: "S", desc: "冻结鼠标所在显示器，框选后离线识别" },
  { name: "输入框转译", key: "T", desc: "翻译当前输入框内容并原位写回" },
];
/** 未实现的入口收进「即将推出」分组并带里程碑锁标，不再平铺成一排空壳 */
const SIDEBAR_SOON: { name: string; milestone: string }[] = [
  { name: "生词本", milestone: "M3" },
  { name: "语音合成", milestone: "M4" },
];
const SIDEBAR_TAIL = ["关于"];

/** 历史时间显示成「今天 14:22」这种更好读的形式 */
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
  });
  const [anki, setAnki] = useState<AnkiStatus | null>(null);
  const [ankiTesting, setAnkiTesting] = useState(false);
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [pluginsPath, setPluginsPath] = useState("");
  const [page, setPage] = useState<Page>("services");
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<ConnectionTest | null>(null);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [histQuery, setHistQuery] = useState("");
  const [histKind, setHistKind] = useState<HistoryKind | "">("");
  const [histId, setHistId] = useState<number | null>(null);

  const selectedEntry =
    history.find((h) => h.id === histId) ?? history[0] ?? null;

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
        <div className="side-group">即将推出</div>
        {SIDEBAR_SOON.map(({ name, milestone }) => (
          <div key={name} className="side-item soon" title={`${milestone} 里程碑开放`}>
            {name}
            <span className="lk"><Icon name="lock" size="sm" />{milestone}</span>
          </div>
        ))}
        {SIDEBAR_TAIL.map((item) => (
          <div key={item} className="side-item">{item}</div>
        ))}
        <div className="side-foot">
          随译 v0.1.0 (dev)
          <br />
          Tauri 2 · Rust + WebView
        </div>
      </aside>

      {/* ---------- 主区 ---------- */}
      <div className="main">
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
                <span className="hname">生词本 · Anki</span>
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
            {HOTKEYS.map((h) => (
              <div className="hkrow" key={h.name}>
                <span className="n">{h.name}</span>
                <span className="m">{h.desc}</span>
                <span className="key"><span className="kbd">Alt</span><span className="kbd">{h.key}</span></span>
              </div>
            ))}
            <div className="thint">
              热键在应用启动时注册；被其他程序占用时会后台重试。想排查可查看
              %APPDATA%/com.suiyi.dev/debug.log。
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
            </div>

            <div className="hist">
              <div className="hist-list">
                {history.length === 0 && (
                  <div className="empty-hint">
                    {histQuery || histKind ? "没有匹配的记录" : "还没有翻译记录"}
                  </div>
                )}
                {history.map((h) => (
                  <div
                    key={h.id}
                    className={`hrow${selectedEntry?.id === h.id ? " on" : ""}`}
                    onClick={() => setHistId(h.id)}
                  >
                    <div className="hl1">
                      <span className={`dot${h.ok ? "" : " err"}`} />
                      <span className="t">{h.source}</span>
                      <span className="chip mile mini">{HISTORY_KIND_LABELS[h.kind] ?? h.kind}</span>
                      {h.serviceName && <span className="chip acc mini">{h.serviceName}</span>}
                    </div>
                    <div className="hl2">
                      {h.translated || h.error || "—"} · {formatTime(h.createdAt)}
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
                      译文{selectedEntry.serviceName ? ` · ${selectedEntry.serviceName}` : ""}
                    </div>
                    <div className="card" style={{ padding: "10px 12px" }}>
                      <div className={selectedEntry.ok ? "t-body" : "t-body terr"}>
                        {selectedEntry.translated || selectedEntry.error}
                      </div>
                    </div>
                    <div className="hmeta">
                      {formatTime(selectedEntry.createdAt)}
                      {selectedEntry.ok && ` · ${(selectedEntry.elapsedMs / 1000).toFixed(1)}s`}
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
                        onClick={() => void handleDeleteEntry(selectedEntry.id)}
                      >
                        <Icon name="trash" size="sm" />删除这条
                      </button>
                    </div>
                  </>
                ) : (
                  <div className="empty-hint">← 选择左侧记录查看详情</div>
                )}
              </div>
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
                          p.version || "未标版本",
                          p.kind ? PLUGIN_KIND_LABELS[p.kind] : "",
                          p.permissions.length ? `权限：${p.permissions.join(" / ")}` : "无特殊权限",
                        ]
                          .filter(Boolean)
                          .join(" · ")
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
            <div key={s.id} className={`svc${draft?.id === s.id ? " on" : ""}`}>
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
                <span>{serviceMeta(s)}</span>
              </span>
              {s.pluginId ? (
                <span className="chip acc mini">插件</span>
              ) : (
                <button
                  className="btn mini"
                  onClick={() => {
                    setDraft({ ...s });
                    setSelectedId(s.id);
                  }}
                >
                  编辑
                </button>
              )}
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
                <b>{draft.id ? `编辑服务 · ${draft.name}` : "添加服务"}</b>
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
                    <option value="open_ai_compatible">OpenAI 兼容 (/v1/chat/completions)</option>
                    <option value="anthropic" disabled>Anthropic（S0.7 开放）</option>
                    <option value="gemini" disabled>Gemini（S0.7 开放）</option>
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
                  API Key（仅存于系统凭据管理器，不入配置文件）
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
                <div className="f" style={{ flex: "0 0 50%" }}>
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
                <label>Prompt 模板 · 变量 {"{{from}} {{to}} {{text}}"} 自动注入</label>
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

      {/* ---------- 全局提示条 ---------- */}
      {(error || notice) && (
        <div className={`toast ${error ? "err" : "ok"}`}>{error || notice}</div>
      )}
    </div>
  );
}
