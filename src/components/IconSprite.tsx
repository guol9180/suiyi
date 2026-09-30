/**
 * 图标图形定义。每个页面（主窗口、划词弹窗、截图覆盖层）都是独立文档，
 * 各自渲染一份。新增图标时同时更新 Icon.tsx 的 IconName。
 */

export function IconSprite() {
  return (
    <svg width="0" height="0" style={{ position: "absolute" }} aria-hidden="true" focusable="false">
      <defs>
        <symbol id="i-pin" viewBox="0 0 16 16">
          <path d="M5 2.6h6" />
          <path d="M6.2 2.6v3.3L4.6 8.4h6.8L9.8 5.9V2.6" />
          <path d="M8 8.4v5" />
        </symbol>
        <symbol id="i-close" viewBox="0 0 16 16">
          <path d="M4.2 4.2 11.8 11.8M11.8 4.2 4.2 11.8" />
        </symbol>
        <symbol id="i-copy" viewBox="0 0 16 16">
          <rect x="2.6" y="2.6" width="7.6" height="7.6" rx="1.4" />
          <path d="M5.8 13.4h6.2a1.4 1.4 0 0 0 1.4-1.4V5.8" />
        </symbol>
        <symbol id="i-swap" viewBox="0 0 16 16">
          <path d="M2.8 5.6h9.4l-2.4-2.4" />
          <path d="M13.2 10.4H3.8l2.4 2.4" />
        </symbol>
        <symbol id="i-refresh" viewBox="0 0 16 16">
          <path d="M12.8 8a4.8 4.8 0 1 1-1.5-3.5" />
          <path d="M13 2.6V6.2H9.4" />
        </symbol>
        <symbol id="i-more" viewBox="0 0 16 16">
          <circle cx="3.6" cy="8" r="1.15" fill="currentColor" stroke="none" />
          <circle cx="8" cy="8" r="1.15" fill="currentColor" stroke="none" />
          <circle cx="12.4" cy="8" r="1.15" fill="currentColor" stroke="none" />
        </symbol>
        <symbol id="i-search" viewBox="0 0 16 16">
          <circle cx="7.2" cy="7.2" r="4.1" />
          <path d="M10.3 10.3 13.4 13.4" />
        </symbol>
        <symbol id="i-globe" viewBox="0 0 16 16">
          <circle cx="8" cy="8" r="5.5" />
          <path d="M2.5 8h11" />
          <path d="M8 2.5c1.6 1.7 2.4 3.5 2.4 5.5S9.6 11.8 8 13.5C6.4 11.8 5.6 10 5.6 8S6.4 4.2 8 2.5z" />
        </symbol>
        <symbol id="i-speaker" viewBox="0 0 16 16">
          <path d="M3.4 6.2h2.2L8.7 3.4v9.2L5.6 9.8H3.4z" />
          <path d="M10.9 6.4a2.5 2.5 0 0 1 0 3.2" />
          <path d="M12.7 4.7a4.8 4.8 0 0 1 0 6.6" />
        </symbol>
        <symbol id="i-bookmark" viewBox="0 0 16 16">
          <path d="M4.4 2.6h7.2v10.8L8 10.6l-3.6 2.8z" />
          <path d="M8 5.2v3.2M6.4 6.8h3.2" />
        </symbol>
        <symbol id="i-plus" viewBox="0 0 16 16">
          <path d="M8 3.4v9.2M3.4 8h9.2" />
        </symbol>
        <symbol id="i-trash" viewBox="0 0 16 16">
          <path d="M3.2 4.4h9.6" />
          <path d="M6.3 4.4V3.3a1 1 0 0 1 1-1h1.4a1 1 0 0 1 1 1v1.1" />
          <path d="M4.6 4.4l.7 8.1a1.2 1.2 0 0 0 1.2 1.1h3a1.2 1.2 0 0 0 1.2-1.1l.7-8.1" />
        </symbol>
        <symbol id="i-save" viewBox="0 0 16 16">
          <path d="M8 2.6v7.2" />
          <path d="M5.3 7.2 8 9.9l2.7-2.7" />
          <path d="M3.2 12.4h9.6" />
        </symbol>
        <symbol id="i-frame" viewBox="0 0 16 16">
          <path d="M2.8 6V4a1.2 1.2 0 0 1 1.2-1.2h2" />
          <path d="M10 2.8h2A1.2 1.2 0 0 1 13.2 4v2" />
          <path d="M13.2 10v2a1.2 1.2 0 0 1-1.2 1.2h-2" />
          <path d="M6 13.2H4A1.2 1.2 0 0 1 2.8 12v-2" />
        </symbol>
        <symbol id="i-bolt" viewBox="0 0 16 16">
          <path d="M8.9 2 4 8.9h3.4l-.5 5.1 5-6.9H8.5z" />
        </symbol>
        <symbol id="i-lock" viewBox="0 0 16 16">
          <rect x="3.6" y="7.2" width="8.8" height="6.2" rx="1.3" />
          <path d="M5.8 7.2V5.4a2.2 2.2 0 0 1 4.4 0v1.8" />
        </symbol>
        <symbol id="i-check" viewBox="0 0 16 16">
          <path d="M3.4 8.3 6.4 11.3 12.6 5.1" />
        </symbol>
        <symbol id="i-alert" viewBox="0 0 16 16">
          <path d="M8 2.8 14.2 13.2H1.8z" />
          <path d="M8 6.6v2.9" />
          <circle cx="8" cy="11.4" r=".85" fill="currentColor" stroke="none" />
        </symbol>
        <symbol id="i-info" viewBox="0 0 16 16">
          <circle cx="8" cy="8" r="5.5" />
          <path d="M8 7.4v3.6" />
          <circle cx="8" cy="5.1" r=".85" fill="currentColor" stroke="none" />
        </symbol>
        <symbol id="i-play" viewBox="0 0 16 16">
          <path d="M5.6 3.6 12.2 8l-6.6 4.4z" fill="currentColor" stroke="none" />
        </symbol>
        <symbol id="i-pause" viewBox="0 0 16 16">
          <rect x="4.9" y="3.8" width="2.2" height="8.4" rx=".5" fill="currentColor" stroke="none" />
          <rect x="8.9" y="3.8" width="2.2" height="8.4" rx=".5" fill="currentColor" stroke="none" />
        </symbol>
        <symbol id="i-grip" viewBox="0 0 16 16">
          <circle cx="5.6" cy="4.4" r="1" fill="currentColor" stroke="none" />
          <circle cx="10.4" cy="4.4" r="1" fill="currentColor" stroke="none" />
          <circle cx="5.6" cy="8" r="1" fill="currentColor" stroke="none" />
          <circle cx="10.4" cy="8" r="1" fill="currentColor" stroke="none" />
          <circle cx="5.6" cy="11.6" r="1" fill="currentColor" stroke="none" />
          <circle cx="10.4" cy="11.6" r="1" fill="currentColor" stroke="none" />
        </symbol>
        <symbol id="i-chev-down" viewBox="0 0 16 16">
          <path d="M4.4 6.6 8 10.2l3.6-3.6" />
        </symbol>
        <symbol id="i-chev-up" viewBox="0 0 16 16">
          <path d="M4.4 9.4 8 5.8l3.6 3.6" />
        </symbol>
        <symbol id="i-sliders" viewBox="0 0 16 16">
          <path d="M3 5.4h6.3" />
          <path d="M11.6 5.4h1.4" />
          <circle cx="9.6" cy="5.4" r="1.4" />
          <path d="M3 10.6h1.4" />
          <path d="M6.7 10.6h6.3" />
          <circle cx="4.9" cy="10.6" r="1.4" />
        </symbol>
        <symbol id="i-arrow-right" viewBox="0 0 16 16">
          <path d="M2.8 8h10.4" />
          <path d="M9.6 4.4 13.2 8l-3.6 3.6" />
        </symbol>
      </defs>
    </svg>
  );
}
