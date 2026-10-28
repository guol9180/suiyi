// 截图框选覆盖层：整屏无损定格帧在画面就绪后才亮窗（无闪现、无压暗、无压缩糊感），
// 悬停高亮光标下的窗口、单击整窗选取，拖动框选区域，松开后交给后端裁剪+OCR；
// 帧图同时是框选坐标的换算标尺（naturalWidth 比例换算，DPI 与混合缩放都稳）
import { useEffect, useRef, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import "./Overlay.css";

interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/// 并集原点（屏幕物理像素）：WindowHit 的屏幕坐标减它就是冻结帧坐标
interface Vs {
  x: number;
  y: number;
}

/// 窗口命中：cx 系为窗口内 CSS 像素（画高亮框用），fx 系为冻结帧像素（提交识别用）
interface Hit {
  cx: number;
  cy: number;
  cw: number;
  ch: number;
  fx: number;
  fy: number;
  fw: number;
  fh: number;
  title: string;
}

/// 悬停探窗节流：时间与位移双门槛，避免鼠标每动一像素就 EnumWindows 一轮
const PROBE_INTERVAL_MS = 80;
const PROBE_MIN_DIST = 24;
/// 按下到松开位移小于该值视为「单击」：落点有窗口时直接整窗选取
const CLICK_SLOP = 4;

export default function OverlayPage() {
  const [freezeUrl, setFreezeUrl] = useState<string | null>(null);
  const [vs, setVs] = useState<Vs | null>(null);
  const [rect, setRect] = useState<Rect | null>(null);
  const [hit, setHit] = useState<Hit | null>(null);
  const [failed, setFailed] = useState(false);
  const dragStart = useRef<{ sx: number; sy: number } | null>(null);
  const downInfo = useRef<{ sx: number; sy: number; hit: Hit | null } | null>(null);
  const busyRef = useRef(false);
  const imgRef = useRef<HTMLImageElement | null>(null);
  const probeLast = useRef<{ x: number; y: number; t: number } | null>(null);
  const probeSeq = useRef(0);

  useEffect(() => {
    // 会话写入与窗口创建几乎同时完成，重试几次更稳
    let tries = 0;
    const tick = () => {
      tries += 1;
      void invoke<{ path: string; vsX: number; vsY: number }>("get_screenshot")
        .then((p) => {
          setFreezeUrl(convertFileSrc(p.path));
          setVs({ x: p.vsX, y: p.vsY });
        })
        .catch(() => {
          if (tries < 6) window.setTimeout(tick, 500);
          else setFailed(true);
        });
    };
    tick();

    // 常驻挂载时靠事件开始新一轮：清掉上一轮的选框状态，重新拉取冻结帧
    const un = listen("screenshot-started", () => {
      dragStart.current = null;
      downInfo.current = null;
      probeLast.current = null;
      busyRef.current = false;
      setRect(null);
      setHit(null);
      setFailed(false);
      void invoke<{ path: string; vsX: number; vsY: number }>("get_screenshot")
        .then((p) => {
          setFreezeUrl(convertFileSrc(p.path));
          setVs({ x: p.vsX, y: p.vsY });
        })
        .catch(() => {});
    });
    return () => {
      void un.then((f) => f());
    };
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

  /// 换算标尺：帧图 1:1 物理像素铺满窗口，渲染框与 naturalWidth 的比值就是
  /// 该窗口的 物理/CSS 比例，比窗口 scale_factor 更贴近实际渲染
  function scaleOf(): { box: DOMRect; kx: number; ky: number } | null {
    const img = imgRef.current;
    const box = img?.getBoundingClientRect();
    if (!img || !box || box.width <= 0 || box.height <= 0) return null;
    return {
      box,
      kx: img.naturalWidth / box.width,
      ky: img.naturalHeight / box.height,
    };
  }

  /// 悬停探窗：把光标换算成屏幕物理坐标交给后端 EnumWindows，结果画成高亮框
  function probeWindow(e: React.MouseEvent) {
    const k = scaleOf();
    const v = vs;
    if (!k || !v) return;
    const px = v.x + e.clientX * k.kx;
    const py = v.y + e.clientY * k.ky;
    const now = performance.now();
    const last = probeLast.current;
    if (
      last &&
      now - last.t < PROBE_INTERVAL_MS &&
      Math.hypot(px - last.x, py - last.y) < PROBE_MIN_DIST
    ) {
      return;
    }
    probeLast.current = { x: px, y: py, t: now };
    const seq = ++probeSeq.current;
    void invoke<{ x: number; y: number; w: number; h: number; title: string } | null>(
      "window_at_point",
      { x: Math.round(px), y: Math.round(py) },
    )
      .then((r) => {
        // 只认最新一次探测的结果，慢回来的旧结果直接丢弃
        if (seq !== probeSeq.current) return;
        if (!r) {
          setHit(null);
          return;
        }
        setHit({
          cx: (r.x - v.x) / k.kx,
          cy: (r.y - v.y) / k.ky,
          cw: r.w / k.kx,
          ch: r.h / k.ky,
          fx: r.x - v.x,
          fy: r.y - v.y,
          fw: r.w,
          fh: r.h,
          title: r.title,
        });
      })
      .catch(() => {});
  }

  function onDown(e: React.MouseEvent) {
    if (busyRef.current) return;
    dragStart.current = { sx: e.clientX, sy: e.clientY };
    downInfo.current = { sx: e.clientX, sy: e.clientY, hit };
    setHit(null); // 开始拖拽后高亮让位给选框
    setRect({ x: e.clientX, y: e.clientY, w: 0, h: 0 });
  }

  function onMove(e: React.MouseEvent) {
    if (!dragStart.current) {
      probeWindow(e);
      return;
    }
    const { sx, sy } = dragStart.current;
    setRect({
      x: Math.min(sx, e.clientX),
      y: Math.min(sy, e.clientY),
      w: Math.abs(e.clientX - sx),
      h: Math.abs(e.clientY - sy),
    });
  }

  function onUp(e: React.MouseEvent) {
    if (!dragStart.current) return;
    const down = downInfo.current;
    dragStart.current = null;
    downInfo.current = null;

    // 单击且按下时命中了窗口 → 整窗选取。WindowHit 与并集原点同为屏幕物理
    // 像素，直接相减就是冻结帧坐标，不经任何缩放换算
    if (
      down?.hit &&
      Math.hypot(e.clientX - down.sx, e.clientY - down.sy) < CLICK_SLOP
    ) {
      const h = down.hit;
      busyRef.current = true;
      void invoke("finish_region", { x: h.fx, y: h.fy, w: h.fw, h: h.fh })
        .catch((err) => console.error(String(err)))
        .finally(() => {
          busyRef.current = false;
        });
      return;
    }

    if (!rect || rect.w < 8 || rect.h < 8) {
      setRect(null);
      return;
    }
    /*
     * 把选框换算成「冻结帧自身的像素坐标」。
     *
     * 不乘窗口缩放系数、也不假设 CSS 像素与物理像素的关系：直接拿图片的渲染框和
     * naturalWidth/naturalHeight 做比例换算。用户看到的是这张图，裁图也按这张图来，
     * 两边永远对得上 —— DPI、滚动条、多屏混合缩放都不会再让框选和识别区域错位。
     */
    const s = scaleOf();
    if (!s) {
      return;
    }
    const ix = (rect.x - s.box.left) * s.kx;
    const iy = (rect.y - s.box.top) * s.ky;
    busyRef.current = true;
    void invoke("finish_region", { x: ix, y: iy, w: rect.w * s.kx, h: rect.h * s.ky })
      .catch((err) => console.error(String(err)))
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
      {/* 定格帧 1:1 物理像素铺满：给用户「屏幕没变化」的画面，也是坐标换算标尺；
          画面加载完成即报 overlay-ready，后端收到才亮窗，消除按下热键后的闪现 */}
      {freezeUrl && (
        <img
          ref={imgRef}
          className="ov-img"
          src={freezeUrl}
          draggable={false}
          alt=""
          onLoad={() => {
            void emit("overlay-ready");
          }}
        />
      )}
      {/* 悬停高亮：光标下整窗的边框与标题，拖拽开始后让位给选框 */}
      {hit && !rect && (
        <div
          className="ov-hit"
          style={{ left: hit.cx, top: hit.cy, width: hit.cw, height: hit.ch }}
        >
          {hit.title && <span className="ov-hit-name">{hit.title}</span>}
        </div>
      )}
      {rect && (
        <div className="ov-sel" style={{ left: rect.x, top: rect.y, width: rect.w, height: rect.h }} />
      )}
      {rect && rect.w > 4 && (
        <span className="ov-size" style={{ left: rect.x, top: rect.y + rect.h + 8 }}>
          {Math.round(rect.w)} × {Math.round(rect.h)}
        </span>
      )}
      <div className="ov-hint">拖动框选区域，单击选取整个窗口（Esc 取消）</div>
    </div>
  );
}
