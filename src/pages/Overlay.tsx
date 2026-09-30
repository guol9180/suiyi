// 截图框选覆盖层：显示冻结帧，拖动框选，松开后交给后端裁剪+OCR
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./Overlay.css";

interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export default function OverlayPage() {
  const [dataUrl, setDataUrl] = useState<string | null>(null);
  const [rect, setRect] = useState<Rect | null>(null);
  const [failed, setFailed] = useState(false);
  const dragStart = useRef<{ sx: number; sy: number } | null>(null);
  const busyRef = useRef(false);

  useEffect(() => {
    // 会话写入与窗口创建几乎同时完成，重试几次更稳
    let tries = 0;
    const tick = () => {
      tries += 1;
      void invoke<{ dataUrl: string }>("get_screenshot")
        .then((p) => setDataUrl(p.dataUrl))
        .catch(() => {
          if (tries < 6) window.setTimeout(tick, 500);
          else setFailed(true);
        });
    };
    tick();
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        void invoke("cancel_screenshot").catch(() => {});
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  function onDown(e: React.MouseEvent) {
    if (busyRef.current) return;
    dragStart.current = { sx: e.clientX, sy: e.clientY };
    setRect({ x: e.clientX, y: e.clientY, w: 0, h: 0 });
  }

  function onMove(e: React.MouseEvent) {
    if (!dragStart.current) return;
    const { sx, sy } = dragStart.current;
    setRect({
      x: Math.min(sx, e.clientX),
      y: Math.min(sy, e.clientY),
      w: Math.abs(e.clientX - sx),
      h: Math.abs(e.clientY - sy),
    });
  }

  function onUp() {
    if (!dragStart.current) return;
    dragStart.current = null;
    if (!rect || rect.w < 8 || rect.h < 8) {
      setRect(null);
      return;
    }
    busyRef.current = true;
    void invoke("finish_region", { x: rect.x, y: rect.y, w: rect.w, h: rect.h })
      .catch((e) => console.error(String(e)))
      .finally(() => {
        busyRef.current = false;
      });
  }

  if (failed) {
    return <div className="ov-failed">截图会话已失效，按 Esc 关闭</div>;
  }

  return (
    <div
      className="overlay-root"
      onMouseDown={onDown}
      onMouseMove={onMove}
      onMouseUp={onUp}
    >
      {dataUrl && <img className="ov-img" src={dataUrl} draggable={false} alt="" />}
      <div className="ov-dim" />
      {rect && (
        <div className="ov-sel" style={{ left: rect.x, top: rect.y, width: rect.w, height: rect.h }}>
          {dataUrl && (
            <img
              className="ov-sel-img"
              src={dataUrl}
              style={{ marginLeft: -rect.x, marginTop: -rect.y }}
              draggable={false}
              alt=""
            />
          )}
        </div>
      )}
      {rect && rect.w > 4 && (
        <span className="ov-size" style={{ left: rect.x, top: rect.y + rect.h + 8 }}>
          {Math.round(rect.w)} × {Math.round(rect.h)}
        </span>
      )}
      <div className="ov-hint">拖动框选要识别的区域 · 松开自动识别并翻译 · Esc 取消</div>
    </div>
  );
}
