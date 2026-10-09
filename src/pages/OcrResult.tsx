// 截图识别结果面板（设计稿第 3 节）。
// v0.9.1 起只有一种形态：原文 + 各服务译文。原来那个「原图覆盖」入口已移除。
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listServices } from "../api";
import { Icon } from "../components/Icon";
import type { ServiceConfig, TranslateResult } from "../types";
import "./OcrResult.css";

/** 识别结果。只带文本与引擎信息：选区图与行矩形不再往界面传（见 screenshot.rs 的说明） */
export interface OcrResult {
  text: string;
  /** paddle / windows / plugin:插件名 */
  engine: string;
  /** 识别语言（BCP-47），插件可能为空 */
  lang: string;
  /** 识别失败的原因；为空表示流程跑完了（哪怕没认出一个字） */
  error?: string | null;
}

/** 一个服务的译文。失败也留着，卡片上要说清是哪个服务没成 */
interface Piece {
  serviceId: string;
  name: string;
  text: string;
  elapsedMs: number;
  error?: string;
}

const TARGET = "简体中文";

export default function OcrResultPage() {
  const [result, setResult] = useState<OcrResult | null>(null);
  const [services, setServices] = useState<ServiceConfig[]>([]);
  const [pieces, setPieces] = useState<Piece[]>([]);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  /** 3 秒还没等到识别结果：面板不再显示「正在读取…」，给明确的重新框选/关闭 */
  const [waitedLong, setWaitedLong] = useState(false);
  /** 每次新的识别结果进来就换一个批次号，过期的翻译结果直接丢掉 */
  const batchRef = useRef(0);

  const runTranslate = useCallback(
    async (text: string, list: ServiceConfig[]) => {
      const batch = ++batchRef.current;
      if (!text.trim() || list.length === 0) {
        setPieces([]);
        setBusy(false);
        return;
      }
      setBusy(true);
      setPieces(list.map((s) => ({ serviceId: s.id, name: s.name, text: "", elapsedMs: 0 })));
      const settled = await Promise.allSettled(
        list.map((s) =>
          invoke<TranslateResult>("translate_text", {
            serviceId: s.id,
            text,
            from: "自动检测",
            to: TARGET,
            kind: "screenshot",
            // 结果要按行盖回原文位置，让模型保持换行
            preserveLines: true,
          }),
        ),
      );
      if (batch !== batchRef.current) return; // 已经有新的识别结果了
      setPieces(
        settled.map((r, i) => {
          const base = { serviceId: list[i].id, name: list[i].name };
          if (r.status === "fulfilled") {
            return { ...base, text: r.value.text, elapsedMs: r.value.elapsedMs };
          }
          return { ...base, text: "", elapsedMs: 0, error: String(r.reason) };
        }),
      );
      setBusy(false);
    },
    [],
  );

  /**
   * 关闭面板。走 Rust 的 ocr_close 命令而不是 getCurrentWindow().hide()：
   * 命令是我们自己注册的，不受 capability 权限集影响；`hide()` 依赖 core:window 权限，
   * 一旦那个窗口漏在权限集外就会「点关闭没反应、Esc 也没用」——之前就是这样卡住的。
   */
  const closePanel = useCallback(async () => {
    try {
      await invoke("ocr_close");
    } catch {
      await getCurrentWindow().hide().catch(() => {});
    }
  }, []);

  // 挂载时先取一次最近结果：事件可能比页面先到，只靠监听会丢
  useEffect(() => {
    void invoke<OcrResult | null>("ocr_last")
      .then((r) => {
        if (r) setResult(r);
      })
      .catch(() => setNotice("读取识别结果失败，可以重新框选"));
    // 3 秒还没结果就别一直显示「正在读取」：给一条明确的出路
    const timer = window.setTimeout(() => {
      setResult((prev) => prev ?? null);
      setWaitedLong(true);
    }, 3000);
    const un = listen<OcrResult>("ocr-set-source", (e) => {
      setResult(e.payload);
      setNotice("");
    });
    return () => {
      window.clearTimeout(timer);
      void un.then((f) => f());
    };
  }, []);

  useEffect(() => {
    void listServices()
      .then((f) =>
        setServices(
          f.services
            .filter((s) => s.enabled && s.kind === "translation")
            .sort((a, b) => a.order - b.order),
        ),
      )
      .catch(() => setServices([]));
  }, []);

  // 有识别结果就自动翻译一次
  useEffect(() => {
    if (!result) return;
    void runTranslate(result.text, services);
  }, [result, services, runTranslate]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") void closePanel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [closePanel]);

  const first = pieces.find((p) => !p.error && p.text);
  const engineLabel =
    result?.engine === "windows"
      ? "Windows.Media.OCR"
      : result?.engine === "paddle"
        ? "PaddleOCR（本地）"
        : result?.engine?.startsWith("plugin:")
          ? `OCR 插件 ${result.engine.slice("plugin:".length)}`
          : "—";
  /** 只认识 en-US / zh-CN 这两种常见值，其余原样显示；PaddleOCR 中英混排时报空串 */
  const langLabel =
    result?.engine === "paddle"
      ? ""
      : result?.lang === "en-US"
        ? "英文"
        : result?.lang === "zh-CN"
          ? "中文"
          : (result?.lang ?? "");

  async function copyTranslation() {
    const text = pieces
      .filter((p) => !p.error && p.text)
      .map((p) => p.text)
      .join("\n\n");
    if (!text) {
      setNotice("还没有译文可复制");
      return;
    }
    await navigator.clipboard.writeText(text);
    setNotice("译文已复制");
  }

  /** 存成 txt：原文 + 每个服务的译文，文件名带时间戳 */
  async function save() {
    if (!result) return;
    const stamp = new Date()
      .toISOString()
      .slice(0, 19)
      .replace(/[:T]/g, "-");
    const body = [
      `原文（${engineLabel}）`,
      result.text,
      "",
      ...pieces
        .filter((p) => !p.error && p.text)
        .map((p) => `译文（${p.name}）\n${p.text}\n`),
    ].join("\n");
    try {
      const path = await invoke<string>("save_text_file", {
        name: `suiyi-ocr-${stamp}.txt`,
        content: body,
      });
      setNotice(`已保存到 ${path}`);
    } catch (e) {
      setNotice(`保存失败：${e}`);
    }
  }

  /** 重新框选：关掉面板，再走一次 Alt+S 的同一条链路 */
  async function retry() {
    await invoke("ocr_close").catch(() => {});
    await invoke("start_screenshot").catch((e) => setNotice(String(e)));
  }

  return (
    <div className="ocr-root">
      <div className="ocr-card">
        <div className="ocr-head" data-tauri-drag-region>
          <span className="grip" data-tauri-drag-region>
            <Icon name="grip" size="sm" />
          </span>
          <span className="ocr-title" data-tauri-drag-region>
            截图识别
          </span>
          {busy && <span className="chip mile mini">翻译中</span>}
          {!busy && first && <span className="chip ok mini">{(first.elapsedMs / 1000).toFixed(1)}s</span>}
          <span style={{ flex: 1 }} />
          <button className="ocr-x" title="关闭" onClick={() => void closePanel()}>
            <Icon name="close" size="sm" />
          </button>
        </div>

        {notice && <div className="ocr-notice">{notice}</div>}

        {/* 隐私条：截图去了哪儿必须写清楚，只说真话 */}
        <div className="ocr-privacy">
          <Icon name="info" size="sm" />
          <span>识别在本机完成，截图不上传；只有识别出的文字会发给你配置的服务。</span>
        </div>

        <div className="ocr-body">
          {!result ? (
            waitedLong ? (
              <div className="emptybox">
                没拿到识别结果
                <br />
                可能上次框选已经结束，重新框一次即可
                <div className="ocr-fallback">
                  <button className="btn mini" onClick={() => void retry()}>
                    <Icon name="frame" size="sm" />重新框选
                  </button>
                  <button className="btn mini" onClick={() => void closePanel()}>关闭</button>
                </div>
              </div>
            ) : (
              <div className="emptybox">正在读取识别结果…</div>
            )
          ) : result.error ? (
            /* 识别本身失败：原因写清楚，并且给一条明确的出路 —— 之前是静默什么都不弹 */
            <div className="emptybox">
              识别失败
              <br />
              {result.error}
              <div className="ocr-fallback">
                <button className="btn mini" onClick={() => void retry()}>
                  <Icon name="frame" size="sm" />重新框选
                </button>
                <button className="btn mini" onClick={() => void closePanel()}>
                  关闭
                </button>
              </div>
            </div>
          ) : !result.text.trim() ? (
            <div className="emptybox">
              未识别到文字
              <br />
              选区可能太窄，或者不含文本；多框一行上下文再试一次更稳
              <div className="ocr-fallback">
                <button className="btn mini" onClick={() => void retry()}>
                  <Icon name="frame" size="sm" />重新框选
                </button>
              </div>
            </div>
          ) : (
            <>
              <div className="ocr-lab">
                原文
                {langLabel && <span className="chip mini" style={{ marginLeft: 6 }}>{langLabel}</span>}
              </div>
              <div className="ocr-line">{result.text}</div>
              <div className="ocr-lab" style={{ marginTop: 12 }}>
                译文 → {TARGET}
              </div>
              {pieces.length === 0 && <div className="emptybox">还没有配置可用的翻译服务</div>}
              {pieces.map((p) => (
                <div className="ocr-trans" key={p.serviceId}>
                  <span className="ocr-who">{p.name}</span>
                  {p.error ? (
                    <span className="ocr-err">{p.error}</span>
                  ) : (
                    <span className="tt">{p.text || "…"}</span>
                  )}
                </div>
              ))}
            </>
          )}
        </div>

        <div className="ocr-actions">
          <button className="btn primary mini" onClick={() => void copyTranslation()}>
            <Icon name="copy" size="sm" />
            复制译文
          </button>
          <button className="btn mini" onClick={() => void save()} disabled={!result?.text.trim()}>
            <Icon name="save" size="sm" />
            保存
          </button>
          <button className="btn mini" onClick={() => void retry()}>
            <Icon name="frame" size="sm" />
            重新框选
          </button>
        </div>

        <div className="ocr-engine">
          引擎 {engineLabel} · 本地离线识别
          <span style={{ flex: 1 }} />
          <span className="chip ok mini">本机完成</span>
        </div>
      </div>
    </div>
  );
}
