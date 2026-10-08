/**
 * 热键冲突时的替代建议。设置页与主窗口横幅共用，避免两处规则漂移。
 *
 * 规则故意简单：默认加 Ctrl+Alt；连 Ctrl+Alt 也占了就换成 Ctrl+Shift。
 */
export function suggestionFor(accel: string): string {
  const key = accel.split("+").pop() ?? "";
  if (accel.includes("ctrl") && accel.includes("alt")) return `ctrl+shift+${key}`;
  return `ctrl+alt+${key}`;
}

/** "alt+d" → "Alt+D"，给横幅和提示行用 */
export function formatAccel(accel: string): string {
  return accel
    .split("+")
    .map((part) => {
      const p = part.trim();
      if (p.length === 1) return p.toUpperCase();
      return p.charAt(0).toUpperCase() + p.slice(1).toLowerCase();
    })
    .join("+");
}
