/**
 * 光标旁的轻提示窗口。
 *
 * 输入框转译是「原位替换、不弹窗」的流程：替换完必须给一句反馈，否则用户不知道
 * 发生了什么。这个窗口透明、置顶、跳过任务栏、不抢焦点、鼠标穿透（都在 Rust 侧
 * 设置好了），所以它只报信，不打断用户正在敲的那个输入框。
 * 显示 2 秒后自己收起来。
 */
import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { noticeHide, noticeLast } from "../api";
import { Icon } from "../components/Icon";
import "./Notice.css";

interface Payload {
  text: string;
  /** ok / err / info */
  kind: string;
}

/** 停留时长与淡出时长，和 CSS 里的动画对上 */
const HOLD_MS = 2000;
const FADE_MS = 260;

function iconOf(kind: string) {
  if (kind === "ok") return "check";
  if (kind === "err") return "alert";
  return "info";
}

export default function NoticePage() {
  const [payload, setPayload] = useState<Payload | null>(null);
  const [leaving, setLeaving] = useState(false);

  const hide = useCallback(() => {
    void noticeHide().catch(() => {});
  }, []);

  useEffect(() => {
    // 事件可能比页面先到，挂载时先取最近一条
    void noticeLast()
      .then((p) => {
        if (p) setPayload(p);
      })
      .catch(() => {});
    const un = listen<Payload>("notice-show", (e) => {
      setLeaving(false);
      setPayload(e.payload);
    });
    return () => {
      void un.then((f) => f());
    };
  }, []);

  useEffect(() => {
    if (!payload) return;
    const t1 = window.setTimeout(() => setLeaving(true), HOLD_MS);
    const t2 = window.setTimeout(hide, HOLD_MS + FADE_MS);
    return () => {
      window.clearTimeout(t1);
      window.clearTimeout(t2);
    };
  }, [payload, hide]);

  return (
    <div className={`nt-root${leaving ? " out" : ""}`}>
      <div className={`nt-card ${payload?.kind ?? "info"}`}>
        <Icon name={iconOf(payload?.kind ?? "info")} size="sm" />
        <span className="nt-text">{payload?.text ?? ""}</span>
      </div>
    </div>
  );
}
