// Tauri 命令封装：前端所有后端调用只经过这里
import { invoke } from "@tauri-apps/api/core";
import type { GlobalSettings, ServiceConfig, ServicesFile } from "./types";

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
