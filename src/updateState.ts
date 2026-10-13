/**
 * 更新流程的窗口级状态。
 *
 * 为什么不放在关于页的组件 state 里：下载跑在 Rust 的任务里，界面只是它的视图。
 * 关于页一卸载（切到通用、切到翻译页）组件状态就没了，回到关于页按钮复原，
 * 用户再点一次就是两个下载同时写同一个文件 —— 各自的已收字节交替上报，
 * 进度条看着左右横跳，最后还可能留下半截安装包。
 *
 * 所以状态放模块级，关于页和顶部提示条都从它读；下载完成要不要立刻拉安装向导，
 * 也按「当时看不看得见关于页」来定，绝不在用户干别的时突然退出应用。
 */
import { listen } from "@tauri-apps/api/event";
import { useSyncExternalStore } from "react";
import {
  cancelUpdate,
  checkUpdate,
  downloadUpdate,
  installUpdate,
  type UpdateInfo,
  type UpdateProgress,
} from "./api";

export type UpdatePhase =
  | "idle"
  | "checking"
  | "available"
  | "downloading"
  | "retrying"
  | "ready"
  | "installing"
  | "error";

/** 失败发生在哪一步：只有下载/安装失败才值得全局提示，查版本失败交给关于页 */
export type UpdateErrorFrom = "" | "check" | "download" | "install";

export interface UpdateSnapshot {
  phase: UpdatePhase;
  info: UpdateInfo | null;
  /** 上次查到结果的时间戳，0 表示这次运行还没查过 */
  checkedAt: number;
  /** 当前这一次尝试已收到的字节 */
  downloaded: number;
  total: number;
  /** 高水位：重试不回退时界面按它画，同一次下载内只增不减 */
  highWater: number;
  attempt: number;
  candidate: string;
  /** 下载好的安装包路径 */
  path: string;
  error: string;
  errorFrom: UpdateErrorFrom;
  /** 0~100；total 未知时为 -1 */
  percent: number;
}

const EMPTY: UpdateSnapshot = {
  phase: "idle",
  info: null,
  checkedAt: 0,
  downloaded: 0,
  total: 0,
  highWater: 0,
  attempt: 0,
  candidate: "",
  path: "",
  error: "",
  errorFrom: "",
  percent: -1,
};

let state: UpdateSnapshot = EMPTY;
const listeners = new Set<() => void>();
/** 关于页是否正开着：决定下载完成后是直接拉安装向导还是先停下等用户点 */
let aboutVisible = false;
/** 用户这一次下载是不是自己点的取消 */
let cancelRequested = false;
/** 下载轮次。取消或重新开始时递增，旧一轮的迟到结果直接丢掉 */
let session = 0;
let progressReady: Promise<void> | null = null;

function commit(next: UpdateSnapshot): void {
  state = next;
  listeners.forEach((fn) => fn());
}

function percentOf(highWater: number, total: number): number {
  if (total <= 0) return -1;
  return Math.max(0, Math.min(100, Math.round((highWater / total) * 100)));
}

/** 界面真正在乎的字段：其余（例如同一百分比内的字节数）变了不通知，省掉无谓重渲染 */
function visibleKey(s: UpdateSnapshot): string {
  return [s.phase, s.percent, s.attempt, s.error, s.path, s.info?.latest ?? ""].join("|");
}

export function updateSnapshot(): UpdateSnapshot {
  return state;
}

export function subscribeUpdate(fn: () => void): () => void {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}

/** 组件读更新状态的唯一入口：页面里不要再自己存一份进度 */
export function useUpdateSnapshot(): UpdateSnapshot {
  return useSyncExternalStore(subscribeUpdate, updateSnapshot, updateSnapshot);
}

/** 关于页挂载/卸载时登记：只影响「下载完是否自动打开安装向导」 */
export function setAboutVisible(visible: boolean): void {
  aboutVisible = visible;
}

function applyProgress(p: UpdateProgress): void {
  // 迟到的进度不该去搅乱已经结束或还没开始的阶段
  if (state.phase !== "downloading" && state.phase !== "retrying") return;
  // 重试事件只报「换线路了」，不带字节数：这时候把进度留着，
  // 界面才不会从 46% 被打回 0% 再爬一遍
  if (p.state === "retrying") {
    const next: UpdateSnapshot = {
      ...state,
      phase: "retrying",
      attempt: p.attempt || state.attempt,
      candidate: p.candidate || state.candidate,
    };
    if (visibleKey(next) === visibleKey(state)) return;
    commit(next);
    return;
  }
  // 换了线路/重试之后总量可能重新报，跨总量比较高水位没有意义
  const sameTotal = p.total > 0 && p.total === state.total;
  const total = p.total > 0 ? p.total : state.total;
  const highWater = sameTotal ? Math.max(state.highWater, p.downloaded) : p.downloaded;
  const next: UpdateSnapshot = {
    ...state,
    // 重试已经在上面单独处理过，走到这里一定是在下载
    phase: "downloading",
    downloaded: p.downloaded,
    total,
    highWater,
    attempt: p.attempt || state.attempt,
    candidate: p.candidate || state.candidate,
    percent: percentOf(highWater, total),
  };
  if (visibleKey(next) === visibleKey(state)) return;
  commit(next);
}

/** 只注册一份监听，挂在窗口上；关于页反复挂载卸载都不影响它 */
function ensureProgressListener(): Promise<void> {
  if (!progressReady) {
    progressReady = listen<UpdateProgress>("update-progress", (e) => applyProgress(e.payload)).then(
      () => undefined,
    );
  }
  return progressReady;
}

/**
 * 查一次最新版本。
 * `silent` 用于启动时的自动检查：查不到就什么也不说，别在用户没问的时候报错。
 */
export async function checkForUpdate(silent = false): Promise<void> {
  if (state.phase === "checking") return;
  // 正在下载或已经下好时不去打扰：这时查版本没有意义，也不该把阶段改回去
  if (state.phase === "downloading" || state.phase === "retrying" || state.phase === "installing") return;
  if (state.phase !== "ready") commit({ ...state, phase: "checking", error: "", errorFrom: "" });
  try {
    const info = await checkUpdate();
    // 安装包已经下好了：只更新版本信息，别把 ready 降级成 available 把路径丢了
    if (state.phase === "ready") {
      commit({ ...state, info, checkedAt: Date.now() });
      return;
    }
    commit({
      ...state,
      phase: info.hasUpdate ? "available" : "idle",
      info,
      checkedAt: Date.now(),
      error: "",
      errorFrom: "",
    });
  } catch (e) {
    // 安装包已经下好，或者启动时的自动检查连不上：都别把已有状态改成故障
    if (silent || state.phase === "ready") {
      commit({
        ...state,
        phase: state.phase === "ready" ? "ready" : state.info?.hasUpdate ? "available" : "idle",
        error: "",
        errorFrom: "",
      });
      return;
    }
    commit({
      ...state,
      phase: "error",
      error: String(e),
      errorFrom: "check",
      checkedAt: Date.now(),
    });
  }
}

/** 下载并安装。下载在 Rust 里跑，页面切走也不受影响 */
export async function startUpdateDownload(): Promise<void> {
  const info = state.info;
  if (!info?.hasUpdate) return;
  if (state.phase === "downloading" || state.phase === "retrying" || state.phase === "installing") return;
  cancelRequested = false;
  const my = ++session;
  void ensureProgressListener();
  commit({
    ...state,
    phase: "downloading",
    downloaded: 0,
    total: 0,
    highWater: 0,
    attempt: 1,
    candidate: "",
    path: "",
    error: "",
    errorFrom: "",
    percent: -1,
  });
  try {
    const path = await downloadWithBusyRetry(info.assetUrl, () => my !== session);
    if (my !== session) return;
    commit({
      ...state,
      phase: "ready",
      path,
      downloaded: state.total,
      highWater: state.total,
      percent: state.total > 0 ? 100 : -1,
    });
    if (aboutVisible) {
      // 用户就盯着这一页，按钮上写的就是「下载并安装」，直接把安装向导拉起来
      await installDownloadedUpdate();
    }
  } catch (e) {
    if (my !== session) return;
    if (cancelRequested) {
      commit({ ...state, phase: "available", downloaded: 0, total: 0, highWater: 0, attempt: 0, percent: -1 });
      return;
    }
    commit({ ...state, phase: "error", error: String(e), errorFrom: "download" });
  }
}

/**
 * 取消后马上又点下载时，Rust 那边上一轮可能还没退出（单飞标志还在），
 * 会立刻回一句「已经在下载中」。这不是故障，等它收尾就好，所以这里退避重试几次。
 */
async function downloadWithBusyRetry(assetUrl: string, stale: () => boolean): Promise<string> {
  for (let i = 0; i < 5; i += 1) {
    // 用户已经取消或又开了一轮：别再往后重试，直接把这一轮作废
    if (stale()) throw new Error("已取消下载");
    try {
      return await downloadUpdate(assetUrl);
    } catch (e) {
      const msg = String(e);
      if (stale()) throw e;
      if (!msg.includes("已经在下载中") || i === 4) throw e;
      await new Promise((r) => setTimeout(r, 400));
    }
  }
  // 循环里每次都 return 或 throw，走到这里只是因为 TS 看不出覆盖完整
  throw new Error("下载没有开始");
}

/** 取消下载：立刻把界面恢复到「发现新版本」，Rust 侧收到标志后收尾 */
export async function cancelUpdateDownload(): Promise<void> {
  if (state.phase !== "downloading" && state.phase !== "retrying") return;
  cancelRequested = true;
  session += 1;
  // 先改界面：网络收尾可能还要一会儿，别让用户觉得按钮没反应
  commit({
    ...state,
    phase: "available",
    downloaded: 0,
    total: 0,
    highWater: 0,
    attempt: 0,
    candidate: "",
    percent: -1,
    error: "",
    errorFrom: "",
  });
  try {
    await cancelUpdate();
  } catch {
    // 取消失败不影响界面：真要没停下，用户还能再点一次下载（Rust 那边有单飞兜底）
  }
}

/** 拉起安装向导：随译会退出，装完由向导把它带回来 */
export async function installDownloadedUpdate(): Promise<void> {
  if (!state.path) return;
  commit({ ...state, phase: "installing", error: "", errorFrom: "" });
  try {
    await installUpdate(state.path);
  } catch (e) {
    commit({ ...state, phase: "error", error: String(e), errorFrom: "install" });
  }
}

/** 关掉失败提示，回到「发现新版本」 */
export function dismissUpdateError(): void {
  if (state.phase !== "error") return;
  commit({
    ...state,
    phase: state.info?.hasUpdate ? "available" : "idle",
    error: "",
    errorFrom: "",
  });
}
