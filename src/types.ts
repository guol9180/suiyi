// 与 src-tauri/src/config.rs 的 serde camelCase 输出一一对应
export type ServiceKind = "translation" | "ocr" | "speech";
// 注意：Rust 端 Protocol::OpenAiCompatible 的 snake_case 是 open_ai_compatible
export type Protocol = "open_ai_compatible" | "anthropic" | "gemini";
export type ResultType = "text" | "dictionary";

export interface ServiceConfig {
  id: string;
  name: string;
  kind: ServiceKind;
  protocol: Protocol;
  enabled: boolean;
  baseUrl: string;
  model: string;
  promptTemplate: string | null;
  temperature: number | null;
  stream: boolean;
  resultType: ResultType;
  order: number;
}

export interface ServicesFile {
  version: number;
  concurrency: number;
  timeoutSecs: number;
  services: ServiceConfig[];
}

export interface GlobalSettings {
  concurrency: number;
  timeoutSecs: number;
}

export const DEFAULT_PROMPT =
  "你是专业翻译引擎。将{{from}}翻译为{{to}}，只输出译文：\n{{text}}";

export const EMPTY_SERVICE: ServiceConfig = {
  id: "",
  name: "",
  kind: "translation",
  protocol: "open_ai_compatible",
  enabled: false,
  baseUrl: "",
  model: "",
  promptTemplate: null,
  temperature: 0.3,
  stream: true,
  resultType: "text",
  order: 0,
};

export const PROTOCOL_LABELS: Record<Protocol, string> = {
  open_ai_compatible: "OpenAI 兼容",
  anthropic: "Anthropic",
  gemini: "Gemini",
};

/** 列表行的第二行摘要：协议 · 模型 */
export function serviceMeta(s: ServiceConfig): string {
  const proto = PROTOCOL_LABELS[s.protocol];
  return s.model ? `${proto} · ${s.model}` : proto;
}

/**
 * 语言方向选项。目标语言不含「自动检测」，其余与来源一致，
 * 这样互换方向后两边的取值永远合法。
 */
export const SOURCE_LANGS = ["自动检测", "中文", "简体中文", "English", "日本語"];
export const TARGET_LANGS = ["中文", "简体中文", "English", "日本語"];

/** 互换语言方向。来源是「自动检测」时无法反过来，目标语退到 English。 */
export function swapLanguages(from: string, to: string): { from: string; to: string } {
  return { from: to, to: from === "自动检测" ? "English" : from };
}
