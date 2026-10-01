// 截图识别结果面板（设计稿第 3 节）。
// 两种模式互斥：结果面板看原文与译文；原图覆盖把译文画回原来的位置。
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listServices } from "../api";
import { Icon } from "../components/Icon";
import type { ServiceConfig, TranslateResult } from "../types";
import "./OcrResult.css";

interface OcrLine {
  text: string;
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface OcrResult {
  cropUrl: string;
  cropW: number;
  cropH: number;
  text: string;
  lines: OcrLine[];
  /** windows 或 plugin:插件名 */
  engine: string;
  /** 识别语言（BCP-47），插件可能为空 */
  lang: string;
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
  const [mode, setMode] = useState<"panel" | "overlay">("panel");
  const [services, setServices] = useState<ServiceConfig[]>([]);
  const [pieces, setPieces] = useState<Piece[]>([]);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
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

  // 挂载时先取一次最近结果：事件可能比页面先到，只靠监听会丢
  useEffect(() => {
    void invoke<OcrResult | null>("ocr_last").then((r) => {
      if (r) setResult(r);
    });
    const un = listen<OcrResult>("ocr-set-source", (e) => {
      setResult(e.payload);
      setNotice("");
    });
    return () => {
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
      if (e.key === "Escape") void getCurrentWindow().hide();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const first = pieces.find((p) => !p.error && p.text);
  const engineLabel =
    result?.engine === "windows"
      ? "Windows.Media.OCR"
      : result?.engine?.startsWith("plugin:")
        ? `OCR 插件 ${result.engine.slice("plugin:".length)}`
        : "—";
  /** 只认识 en-US / zh-CN 这两种常见值，其余原样显示 */
  const langLabel =
    result?.lang === "en-US" ? "英文" : result?.lang === "zh-CN" ? "中文" : (result?.lang ?? "");

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

  /**
   * 覆盖模式的定位。
   *
   * 两种情况会退化成「整块压在选区上」而不是逐行贴合：
   * 1. 插件 OCR 只给文字、没有行矩形 —— 不该假装知道每行在哪；
   * 2. 译文行数与原文行数对不上 —— 模型没按行返回，硬按行贴只会错位。
   * 这两种都比「贴得漂漂亮亮但位置是错的」诚实。
   */
  const rectLines = (result?.lines ?? []).filter((l) => l.w > 0 && l.h > 0);
  const transLines = (first?.text ?? "")
    .split("\n")
    .map((t) => t.trim())
    .filter(Boolean);
  const perLine = rectLines.length > 0 && transLines.length === rectLines.length;
  const fallbackTop = rectLines.length > 0 ? rectLines[0].y / (result?.cropH || 1) : 0.06;

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
          <button className="ocr-x" title="关闭" onClick={() => void getCurrentWindow().hide()}>
            <Icon name="close" size="sm" />
          </button>
        </div>

        <div className="ocr-seg">
          <span className={mode === "panel" ? "on" : ""} onClick={() => setMode("panel")}>
            结果面板
          </span>
          <span className={mode === "overlay" ? "on" : ""} onClick={() => setMode("overlay")}>
            原图覆盖
          </span>
        </div>

        {notice && <div className="ocr-notice">{notice}</div>}

        {/* 隐私条：截图去了哪儿必须写清楚，只说真话 */}
        <div className="ocr-privacy">
          <Icon name="info" size="sm" />
          <span>识别在本机完成，截图不上传；只有识别出的文字会发给你配置的服务。</span>
        </div>

        {mode === "panel" ? (
          <div className="ocr-body">
            {!result ? (
              <div className="emptybox">正在读取识别结果…</div>
            ) : !result.text.trim() ? (
              <div className="emptybox">
                未识别到文字
                <br />
                选区可能不含文本，或者换一段更清楚的区域
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
        ) : (
          <div className="ocr-stage">
            {result?.cropUrl ? (
              <div className="ocr-shot">
                <img className="ocr-shot-img" src={result.cropUrl} alt="选区" draggable={false} />
                {first &&
                  (perLine
                    ? rectLines.map((l, i) => (
                        <span
                          className="otrans"
                          key={i}
                          title={`${l.text} → ${transLines[i]}`}
                          style={{
                            left: `${(l.x / result.cropW) * 100}%`,
                            top: `${(l.y / result.cropH) * 100}%`,
                            minWidth: `${(l.w / result.cropW) * 100}%`,
                            minHeight: `${(l.h / result.cropH) * 100}%`,
                          }}
                        >
                          {transLines[i]}
                        </span>
                      ))
                    : (
                        <span
                          className="otrans block"
                          style={{ top: `${fallbackTop * 100}%` }}
                          title={first.text}
                        >
                          {first.text}
                        </span>
                      ))}
                {!first && <span className="otrans center">还没有译文</span>}
              </div>
            ) : (
              <div className="emptybox">没有可覆盖的原图</div>
            )}
          </div>
        )}

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
