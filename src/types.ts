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
  /** 这家服务是否需要 API Key。本地服务（Ollama）设 false，请求就不带 Authorization 头 */
  requiresKey: boolean;
  order: number;
  /** 由插件提供的服务才有；这类服务不落盘，也不能在服务列表里编辑 */
  pluginId?: string | null;
}

export interface ServicesFile {
  version: number;
  concurrency: number;
  timeoutSecs: number;
  inputTargetLang: string;
  ankiUrl: string;
  ankiDeck: string;
  speechVoice: string;
  speechRate: number;
  services: ServiceConfig[];
}

export interface GlobalSettings {
  concurrency: number;
  timeoutSecs: number;
  /** 输入框转译（Alt+T）的目标语言 */
  inputTargetLang: string;
  /** AnkiConnect 地址与目标牌组 */
  ankiUrl: string;
  ankiDeck: string;
  /** 朗读用的系统音色 id，空串表示系统默认 */
  speechVoice: string;
  /** 语速倍数 */
  speechRate: number;
}

/** 系统里的一个本地语音 */
export interface SpeechVoice {
  id: string;
  name: string;
  /** BCP-47 语言标签，例如 zh-CN */
  language: string;
  gender: string;
}

/** 一次朗读会话的实时状态，进度与时长都来自系统 */
export interface SpeechState {
  active: boolean;
  playing: boolean;
  paused: boolean;
  positionMs: number;
  durationMs: number;
  rate: number;
  voice: string;
  text: string;
}

export interface AnkiStatus {
  available: boolean;
  version: number | null;
  deckExists: boolean;
  error: string | null;
}

export interface AnkiAddResult {
  added: boolean;
  /** 已存在同名词条，不算失败 */
  duplicate: boolean;
  noteId: number | null;
  error: string | null;
}

/** 生词本里的一条词。syncedAt 为空表示还在待同步队列里 */
export interface WordEntry {
  id: number;
  createdAt: number;
  /** 词条本身，Anki 卡片的正面 */
  term: string;
  /** 音标或读音，可为空串 */
  reading: string;
  /** 释义或译文，Anki 卡片的背面 */
  meaning: string;
  source: string;
  syncedAt?: number;
  noteId?: number;
  /** 最近一次同步失败的原因 */
  lastError?: string;
}

export interface WordbookStats {
  total: number;
  pending: number;
}

/** 生词本的完整视图：一次调用同时拿到列表、计数与本次同步结果 */
export interface WordbookView {
  entries: WordEntry[];
  stats: WordbookStats;
  /** 本次操作有词条被送进 Anki */
  synced: boolean;
  /** 词条之前就在 Anki 里 */
  duplicate: boolean;
  error: string | null;
}

/** 词典结构化结果（服务结果类型为「词典结构」时返回） */
export interface Sense {
  pos: string;
  def: string;
  example?: string;
}

export interface DictionaryResult {
  word: string;
  phonetic?: string;
  senses: Sense[];
}

/** translate_text 的返回值 */
export interface TranslateResult {
  serviceId: string;
  text: string;
  elapsedMs: number;
  /** 解析成功时存在；解析失败只有 text，按纯文本展示 */
  dictionary?: DictionaryResult;
}

/** 翻译来源，用于历史归类 */
export type HistoryKind = "selection" | "screenshot" | "manual" | "input";

export interface HistoryEntry {
  id: number;
  createdAt: number;
  kind: HistoryKind;
  source: string;
  translated: string;
  serviceName: string;
  elapsedMs: number;
  ok: boolean;
  error?: string;
}

export const HISTORY_KIND_LABELS: Record<HistoryKind, string> = {
  selection: "划词",
  screenshot: "截图",
  manual: "手输",
  input: "输入框",
};

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
  requiresKey: true,
  order: 0,
};

export const PROTOCOL_LABELS: Record<Protocol, string> = {
  open_ai_compatible: "OpenAI 兼容",
  anthropic: "Anthropic",
  gemini: "Gemini",
};

/**
 * 列表行的第二行摘要。
 * OpenAI 兼容是默认协议，每行都重复一遍会把模型名挤到截断，
 * 所以只有非默认协议才带前缀。完整信息仍在右侧表单里。
 */
export function serviceMeta(s: ServiceConfig): string {
  // 插件服务在列表行里已经有「插件」标签，这里只报模型
  if (s.pluginId) return s.model || "本地脚本";
  const proto = PROTOCOL_LABELS[s.protocol];
  if (s.protocol === "open_ai_compatible") return s.model || proto;
  return s.model ? `${proto}：${s.model}` : proto;
}

export type PluginKind = "translation" | "ocr" | "speech" | "action";

export interface PluginInfo {
  id: string;
  name: string;
  version: string;
  description: string;
  author: string;
  kind: PluginKind | null;
  permissions: string[];
  dir: string;
  main: string | null;
  enabled: boolean;
  ok: boolean;
  error: string | null;
}

export const PLUGIN_KIND_LABELS: Record<PluginKind, string> = {
  translation: "翻译",
  ocr: "OCR",
  speech: "语音",
  action: "动作",
};

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
