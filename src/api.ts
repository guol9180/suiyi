// Tauri 命令封装：前端所有后端调用只经过这里
import { invoke } from "@tauri-apps/api/core";
import type { GlobalSettings, ServiceConfig, ServicesFile } from "./types";
import type { HistoryEntry, HistoryKind } from "./types";
import type { AnkiStatus, SpeechState, SpeechVoice, WordbookView } from "./types";
import type { PluginInfo } from "./types";

export function listServices(): Promise<ServicesFile> {
  return invoke("list_services");
}

export function saveService(service: ServiceConfig): Promise<ServiceConfig[]> {
  return invoke("save_service", { service });
}

export function deleteService(serviceId: string): Promise<ServiceConfig[]> {
  return invoke("delete_service", { serviceId });
}

export function reorderServices(ids: string[]): Promise<ServiceConfig[]> {
  return invoke("reorder_services", { ids });
}

export function setApiKey(serviceId: string, apiKey: string): Promise<void> {
  return invoke("set_api_key", { serviceId, apiKey });
}

/** onlyCheck=true 时后端不回传明文，仅返回是否已设置 */
export function getApiKey(serviceId: string, onlyCheck = true): Promise<string | null> {
  return invoke("get_api_key", { serviceId, onlyCheck });
}

export function deleteApiKey(serviceId: string): Promise<void> {
  return invoke("delete_api_key", { serviceId });
}

/** 读一次剪贴板文本，供设置页的「从剪贴板粘贴」使用 */
export function readClipboardText(): Promise<string> {
  return invoke("read_clipboard_text");
}

export function getSettings(): Promise<GlobalSettings> {
  return invoke("get_settings");
}

export function saveSettings(settings: GlobalSettings): Promise<void> {
  return invoke("save_settings", { settings });
}

export interface ConnectionTest {
  ok: boolean;
  elapsedMs: number;
  models: string[];
  error: string | null;
}

/** 一个全局热键的注册状态 */
export interface HotkeyStatus {
  id: string;
  label: string;
  accelerator: string;
  registered: boolean;
  /** 注册失败的原因，成功时为 null */
  error: string | null;
  /** 是否已被用户改过 */
  custom: boolean;
}

/** 三个全局热键当前的注册状态 */
export function hotkeyStatus(): Promise<HotkeyStatus[]> {
  return invoke("hotkey_status");
}

/** 重新尝试注册尚未成功的热键 */
export function retryHotkeys(): Promise<HotkeyStatus[]> {
  return invoke("retry_hotkeys");
}

/** 改一个全局热键；组合注册不上时后端会回滚配置并返回原状态 */
export function setHotkey(id: string, accelerator: string): Promise<HotkeyStatus[]> {
  return invoke("set_hotkey", { id, accelerator });
}

/** 全部恢复出厂热键 */
export function resetHotkeys(): Promise<HotkeyStatus[]> {
  return invoke("reset_hotkeys");
}

/** 探测服务连通性：Key 是否有效、网关是否可达、模型是否可见 */
export function testConnection(serviceId: string): Promise<ConnectionTest> {
  return invoke("test_connection", { serviceId });
}

/**
 * 只取模型列表：设置页选中服务后就自动拉一次，用户不用手填模型名。
 * 失败时抛出的原因可直接展示（401/404 这类由前端 errorText 翻成人话）。
 */
export function listModels(serviceId: string): Promise<string[]> {
  return invoke("list_models", { serviceId });
}

/** 关于页要展示的三处路径 */
export interface AppPaths {
  configDir: string;
  logFile: string;
  pluginsDir: string;
}

export function appPaths(): Promise<AppPaths> {
  return invoke("app_paths");
}

/** 读日志尾部，供「复制诊断信息」用；读不到返回空串 */
export function tailLog(lines = 40): Promise<string> {
  return invoke("tail_log", { lines });
}

/**
 * 真正退出应用。点窗口的 × 只是收起主窗口（全局热键要继续可用），
 * 所以退出必须有一个显式入口。
 */
export function quitApp(): Promise<void> {
  return invoke("quit_app");
}

/** 把译文替换回取词时所在的那个窗口 */
export function replaceSelection(text: string): Promise<void> {
  return invoke("replace_selection", { text });
}

/**
 * 用 Windows 本地语音朗读文本（离线，不消耗翻译额度）。
 * 不传 rate/voice 时后端按设置里的音色与语速来，各处朗读保持一致。
 */
export function speakText(
  text: string,
  opts: { rate?: number; voice?: string } = {},
): Promise<SpeechState> {
  return invoke("speak_text", {
    text,
    rate: opts.rate ?? null,
    voice: opts.voice ?? null,
  });
}

/** 当前朗读进度。进度与时长都来自系统，不是估算出来的 */
export function speechState(): Promise<SpeechState> {
  return invoke("speech_state");
}

export function pauseSpeaking(): Promise<SpeechState> {
  return invoke("pause_speaking");
}

export function resumeSpeaking(): Promise<SpeechState> {
  return invoke("resume_speaking");
}

/** 停止当前朗读 */
export function stopSpeaking(): Promise<SpeechState> {
  return invoke("stop_speaking");
}

/** 系统里可用的本地语音 */
export function listSpeechVoices(): Promise<SpeechVoice[]> {
  return invoke("list_speech_voices");
}

export function listHistory(opts: {
  query?: string;
  kind?: HistoryKind;
  limit?: number;
  offset?: number;
} = {}): Promise<HistoryEntry[]> {
  return invoke("list_history", {
    query: opts.query ?? null,
    kind: opts.kind ?? null,
    limit: opts.limit ?? 100,
    offset: opts.offset ?? 0,
  });
}

export function deleteHistory(id: number): Promise<void> {
  return invoke("delete_history", { id });
}

export function clearHistory(): Promise<void> {
  return invoke("clear_history");
}

/** 探测 Anki 连接状态 */
export function ankiStatus(): Promise<AnkiStatus> {
  return invoke("anki_status");
}

/**
 * 生词本。加词是先落本地再推 Anki，所以 Anki 没开也不会丢词，
 * 每个调用都返回完整视图，前端不用再补一次查询。
 */
export function wordbookList(): Promise<WordbookView> {
  return invoke("wordbook_list");
}

export function wordbookAdd(
  term: string,
  meaning: string,
  opts: { reading?: string; source?: string } = {},
): Promise<WordbookView> {
  return invoke("wordbook_add", {
    term,
    meaning,
    reading: opts.reading ?? null,
    source: opts.source ?? null,
  });
}

/** 批量补发待同步的词条 */
export function wordbookSync(): Promise<WordbookView> {
  return invoke("wordbook_sync");
}

export function wordbookRemove(id: number): Promise<WordbookView> {
  return invoke("wordbook_remove", { id });
}

export function listPlugins(): Promise<PluginInfo[]> {
  return invoke("list_plugins");
}

export function setPluginEnabled(id: string, enabled: boolean): Promise<PluginInfo[]> {
  return invoke("set_plugin_enabled", { id, enabled });
}

export function createSamplePlugin(): Promise<PluginInfo[]> {
  return invoke("create_sample_plugin");
}

export function pluginsDirPath(): Promise<string> {
  return invoke("plugins_dir_path");
}

/** 运行动作插件，返回要展示给用户的提示文本 */
export function runActionPlugin(id: string, source: string, translated: string): Promise<string> {
  return invoke("run_action_plugin", { id, source, translated });
}
