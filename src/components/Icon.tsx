/**
 * 线性图标组件。图形定义在 IconSprite 里，通过 <use> 引用。
 * 颜色、线宽、尺寸由 base.css 的 .ico 控制，跟随所在文字颜色。
 */

export type IconName =
  | "pin"
  | "close"
  | "copy"
  | "swap"
  | "refresh"
  | "more"
  | "search"
  | "globe"
  | "speaker"
  | "bookmark"
  | "plus"
  | "trash"
  | "save"
  | "frame"
  | "bolt"
  | "lock"
  | "check"
  | "alert"
  | "info"
  | "play"
  | "pause"
  | "grip"
  | "chev-down"
  | "chev-up"
  | "sliders"
  | "arrow-right";

interface IconProps {
  name: IconName;
  /** sm 为 14px，默认 16px */
  size?: "md" | "sm";
  className?: string;
  title?: string;
}

export function Icon({ name, size = "md", className, title }: IconProps) {
  return (
    <svg
      className={`ico${size === "sm" ? " sm" : ""}${className ? ` ${className}` : ""}`}
      aria-hidden={title ? undefined : true}
      role={title ? "img" : undefined}
      focusable="false"
    >
      {title ? <title>{title}</title> : null}
      <use href={`#i-${name}`} />
    </svg>
  );
}
