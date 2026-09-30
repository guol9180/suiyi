/**
 * 每个服务最近一次翻译的结果，只放在内存里，重启即清空。
 *
 * 翻译页跑完一轮后写进来，设置页的服务列表读它显示「上次失败」。
 * 两个页面在同一个窗口里，所以一个模块级的小 store 就够了，
 * 不需要再引一个状态库。
 */

export interface LastResult {
  ok: boolean;
  /** 失败时的原始错误文本，成功时为空 */
  error?: string;
  at: number;
}

const results = new Map<string, LastResult>();
const listeners = new Set<() => void>();
let version = 0;

export function recordResult(serviceId: string, result: LastResult): void {
  results.set(serviceId, result);
  version += 1;
  listeners.forEach((fn) => fn());
}

export function subscribe(fn: () => void): () => void {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}

/** useSyncExternalStore 的快照：只关心"变过没有"，所以直接给版本号 */
export function getVersion(): number {
  return version;
}

export function lastOf(serviceId: string): LastResult | undefined {
  return results.get(serviceId);
}

/** 从错误文本里挑出 HTTP 状态码，挑不到就返回空 */
export function statusCodeOf(error: string | undefined): string {
  if (!error) return "";
  const m = error.match(/\b(4\d{2}|5\d{2})\b/);
  return m ? m[1] : "";
}
