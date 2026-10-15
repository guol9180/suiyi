// 历史记录（v1.0 起为主窗口一级页签）：日期分组列表 + 详情双栏。
// 数据自包含：挂载时拉取服务与历史，不依赖设置页；搜索词走后端，服务与时间在本地过滤。
import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  clearHistory,
  deleteHistory,
  listHistory,
  listServices,
  speakText,
  stopSpeaking,
  wordbookAdd,
} from "../api";
import {
  enabledTranslations,
  HISTORY_KIND_LABELS,
  type HistoryEntry,
  type HistoryKind,
  type ServiceConfig,
} from "../types";
import { dayLabel, formatSecs, formatTime } from "../format";
import { Icon } from "../components/Icon";
import "./History.css";

type Range = "all" | "7d";
interface Retrans {
  id: number;
  service: string;
  text: string;
}
interface DayGroup {
  label: string;
  items: HistoryEntry[];
}

/** 按日期分组；后端按时间倒序返回，相邻同标签归并即可，不打乱原顺序 */
function groupByDay(rows: HistoryEntry[]): DayGroup[] {
  const groups: DayGroup[] = [];
  for (const h of rows) {
    const label = dayLabel(h.createdAt);
    const last = groups[groups.length - 1];
    if (last && last.label === label) last.items.push(h);
    else groups.push({ label, items: [h] });
  }
  return groups;
}

export default function HistoryPage(props: { active: boolean }) {
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [services, setServices] = useState<ServiceConfig[]>([]);
  const [histQuery, setHistQuery] = useState("");
  const [histKind, setHistKind] = useState<HistoryKind | "">("");
  const [histService, setHistService] = useState("");
  const [histRange, setHistRange] = useState<Range>("all");
  const [histId, setHistId] = useState<number | null>(null);
  /** 正在朗读的那条记录；null 表示没有在朗读 */
  const [speakingId, setSpeakingId] = useState<number | null>(null);
  /** 换服务重译的临时结果，只留在界面上，不改动历史 */
  const [retrans, setRetrans] = useState<Retrans | null>(null);
  const [toast, setToast] = useState<{ kind: "ok" | "err"; text: string } | null>(null);

  function flash(kind: "ok" | "err", text: string) {
    setToast({ kind, text });
    window.setTimeout(() => setToast((cur) => (cur?.text === text ? null : cur)), 2600);
  }

  const loadHistory = useCallback(async () => {
    try {
      const rows = await listHistory({ query: histQuery, kind: histKind || undefined, limit: 200 });
      setHistory(rows);
      setHistId((cur) => (rows.some((r) => r.id === cur) ? cur : (rows[0]?.id ?? null)));
    } catch (e) {
      flash("err", String(e));
    }
  }, [histQuery, histKind]);

  // 页签是 keep-mounted 的：只在挂载时拉一次的话，划词/截图的新记录永远进不来，
  // 用户只能手动刷新。每次切到本页签时重新拉取。
  useEffect(() => {
    if (props.active) void loadHistory();
  }, [props.active, loadHistory]);

  useEffect(() => {
    void listServices()
      .then((f) => setServices(enabledTranslations(f.services)))
      .catch(() => setServices([]));
  }, []);

  const visibleHistory = useMemo(
    () =>
      history.filter((h) => {
        if (histService && h.serviceName !== histService) return false;
        if (histRange === "7d" && Date.now() - h.createdAt > 7 * 86_400_000) return false;
        return true;
      }),
    [history, histService, histRange],
  );

  const selectedEntry = visibleHistory.find((h) => h.id === histId) ?? visibleHistory[0] ?? null;
  const groups = useMemo(() => groupByDay(visibleHistory), [visibleHistory]);

  async function handleClearHistory() {
    if (!window.confirm("清空全部翻译历史？此操作不可撤销。")) return;
    try {
      await clearHistory();
      setHistory([]);
      setHistId(null);
      flash("ok", "历史已清空");
    } catch (e) {
      flash("err", String(e));
    }
  }

  async function handleDeleteEntry(id: number) {
    try {
      await deleteHistory(id);
      await loadHistory();
    } catch (e) {
      flash("err", String(e));
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
      flash("err", String(e));
    }
  }

  /** 把这条记录加进 Anki 生词本：正面原文、背面译文 */
  async function handleAddToAnki(entry: HistoryEntry) {
    try {
      // 走生词本：先落本地，Anki 没开也留得住，之后能批量补发
      const r = await wordbookAdd(entry.source, entry.translated || entry.error || "", {
        source: entry.kind,
      });
      if (r.duplicate) flash("err", r.error ?? "这条已经在生词本里了");
      else if (r.synced) flash("ok", "已加入生词本并同步到 Anki");
      else flash("ok", "已加入生词本，等 Anki 可用时自动同步");
    } catch (e) {
      flash("err", String(e));
    }
  }

  /** 换个服务重译这条的原文，结果只显示在详情里，不写回历史 */
  async function handleRetranslate(entry: HistoryEntry, serviceId: string) {
    const svc = services.find((s) => s.id === serviceId);
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
      flash("err", String(e));
    }
  }

  const filtered = histQuery || histKind || histService || histRange === "7d";

  return (
    <div className="history-page">
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
            <span key={k} className={`chip${histKind === k ? " acc" : ""}`} onClick={() => setHistKind(k)}>
              {HISTORY_KIND_LABELS[k]}
            </span>
          ))}
          <span className="hist-sep" />
          <span className={`chip${histService === "" ? " acc" : ""}`} onClick={() => setHistService("")}>
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
          <span className={`chip${histRange === "all" ? " acc" : ""}`} onClick={() => setHistRange("all")}>
            全部时间
          </span>
          <span className={`chip${histRange === "7d" ? " acc" : ""}`} onClick={() => setHistRange("7d")}>
            近 7 天
          </span>
        </div>

        <div className="hist">
          <div className="hist-list">
            {visibleHistory.length === 0 && (
              <div className="empty-hint">{filtered ? "没有匹配的记录" : "还没有翻译记录"}</div>
            )}
            {groups.map((g) => (
              <div key={g.label}>
                <div className="dg">{g.label}</div>
                {g.items.map((h) => (
                  <div
                    key={h.id}
                    className={`hrow${selectedEntry?.id === h.id ? " on" : ""}`}
                    onClick={() => setHistId(h.id)}
                  >
                    <div className="hl1">
                      <span className={`dot${h.ok ? "" : " err"}`} />
                      <span className="t">{h.source}</span>
                      {h.serviceName && <span className="chip acc mini">{h.serviceName}</span>}
                    </div>
                    {/* 第二行拆两段：摘要可截断，时间与耗时固定不截断 */}
                    <div className={`hl2${h.ok ? "" : " err"}`}>
                      <span className="sum">{h.translated || h.error || "—"}</span>
                      <span className="meta">
                        <span>{HISTORY_KIND_LABELS[h.kind] ?? h.kind}</span>
                        <span>{formatTime(h.createdAt)}</span>
                        {h.ok && h.elapsedMs > 0 && <span>{formatSecs(h.elapsedMs)}</span>}
                      </span>
                    </div>
                  </div>
                ))}
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
                  <span>{HISTORY_KIND_LABELS[selectedEntry.kind] ?? selectedEntry.kind}</span>
                  <span>{formatTime(selectedEntry.createdAt)}</span>
                  {selectedEntry.ok && <span>{formatSecs(selectedEntry.elapsedMs)}</span>}
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
                    {enabledTranslations(services)
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

      {toast && <div className={`toast float ${toast.kind}`}>{toast.text}</div>}
    </div>
  );
}
