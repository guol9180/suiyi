/**
 * 前端共用的小格式化工具。
 * 只收有两个以上使用方的函数；单处使用就留在原地，别把这里做成垃圾桶。
 */

/** 耗时显示：毫秒 → 「0.9s」一位小数。翻译页、划词弹窗、OCR 面板与历史都用它 */
export function formatSecs(ms: number): string {
  return `${(ms / 1000).toFixed(1)}s`;
}

/** 历史时间显示成「今天 14:22」这种更好读的形式 */
export function formatTime(ms: number): string {
  const d = new Date(ms);
  const today = new Date();
  const yesterday = new Date(today.getTime() - 86_400_000);
  const day =
    d.toDateString() === today.toDateString()
      ? "今天"
      : d.toDateString() === yesterday.toDateString()
        ? "昨天"
        : `${d.getMonth() + 1} 月 ${d.getDate()} 日`;
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${day} ${hh}:${mm}`;
}
