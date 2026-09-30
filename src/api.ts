// Tauri 命令封装：前端所有后端调用只经过这里
import { invoke } from "@tauri-apps/api/core";
import type { GlobalSettings, ServiceConfig, ServicesFile } from "./types";
import type { HistoryEntry, HistoryKind } from "./types";
import type { AnkiAddResult, AnkiStatus } from "./types";
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
}

/** 三个全局热键当前的注册状态 */
export function hotkeyStatus(): Promise<HotkeyStatus[]> {
  return invoke("hotkey_status");
}

/** 重新尝试注册尚未成功的热键 */
export function retryHotkeys(): Promise<HotkeyStatus[]> {
  return invoke("retry_hotkeys");
}

/** 探测服务连通性：Key 是否有效、网关是否可达、模型是否可见 */
export function testConnection(serviceId: string): Promise<ConnectionTest> {
  return invoke("test_connection", { serviceId });
}

/** 把译文替换回取词时所在的那个窗口 */
export function replaceSelection(text: string): Promise<void> {
  return invoke("replace_selection", { text });
}

/** 用 Windows 本地语音朗读文本（离线，不消耗翻译额度） */
export function speakText(text: string): Promise<void> {
  return invoke("speak_text", { text });
}

/** 停止当前朗读 */
export function stopSpeaking(): Promise<void> {
  return invoke("stop_speaking");
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

/** 把词条加入 Anki 生词本 */
export function ankiAdd(front: string, back: string): Promise<AnkiAddResult> {
  return invoke("anki_add", { front, back });
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
