/**
 * 服务识别色：同一服务永远拿到同一颜色（按 id 稳定散列），
 * 与启用顺序无关——服务排序变化不打翻色卡。
 * 色板与 tokens.css 的品牌蓝同源，其余为 v1.0 提案附录的识别色。
 */
const SVC_PALETTE = [
  "#3964FE", // 品牌蓝
  "#8B5CF6", // 紫
  "#0F9D8F", // 青
  "#E8790A", // 橙
  "#B4291F", // 红
  "#7A5AF8", // 深紫
  "#2E7DB2", // 海蓝
  "#C23A96", // 品红
] as const;

export function svcColor(id: string): string {
  let h = 2166136261; // FNV-1a 偏移基
  for (let i = 0; i < id.length; i++) {
    h ^= id.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return SVC_PALETTE[(h >>> 0) % SVC_PALETTE.length];
}
