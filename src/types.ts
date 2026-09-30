// 与 src-tauri/src/config.rs 的 serde camelCase 输出一一对应
export type ServiceKind = "translation" | "ocr" | "speech";
export type Protocol = "openai_compatible" | "anthropic" | "gemini";
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
  protocol: "openai_compatible",
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
  openai_compatible: "OpenAI 兼容",
  anthropic: "Anthropic",
  gemini: "Gemini",
};

/** 列表行的第二行摘要：协议 · 模型 */
export function serviceMeta(s: ServiceConfig): string {
  const proto = PROTOCOL_LABELS[s.protocol];
  return s.model ? `${proto} · ${s.model}` : proto;
}
