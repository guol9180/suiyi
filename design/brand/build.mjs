/*
 * 随译 SuiYi · 品牌标记构建脚本
 *
 * 用法：node design/brand/build.mjs
 *
 * 做三件事：
 *   1. 用仓库里的 @tauri-apps/cli 从 design/brand/logo.svg 重出整套应用图标
 *      （icon.ico / icon.icns / icon.png / 各尺寸 PNG / Square*Logo）。
 *      CLI 是本地依赖，全程离线；它顺手写的 android/、ios/ 两个目录这里清掉，本项目只发 Windows。
 *      resvg 解析 SVG，所以这一步同时是「SVG 是否合法」的检查：文件坏了这里就报错退出。
 *   2. 把 logo.svg 原样复制到 public/icon.svg（应用内各窗口的 favicon）
 *      与 docs/img/icon.svg（下载页 favicon）。
 *   3. 自检：无外部引用、ico 尺寸齐全、PNG 带 alpha 通道、icns 文件头正确。
 *
 * 标记本身怎么改：直接编辑 logo.svg。色值、几何、字形来源都记在那个文件的顶部注释里。
 */

import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, readFileSync, rmSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..", "..");
const master = join(here, "logo.svg");
const iconsDir = join(root, "src-tauri", "icons");
const favicons = [join(root, "public", "icon.svg"), join(root, "docs", "img", "icon.svg")];
const cli = join(root, "node_modules", "@tauri-apps", "cli", "tauri.js");

/** 必须存在的产物与最低体积，太小说明生成中途失败了 */
const expected = [
  ["icon.ico", 4000],
  ["icon.icns", 20000],
  ["icon.png", 5000],
  ["32x32.png", 400],
  ["128x128.png", 1500],
  ["128x128@2x.png", 3000],
  ["StoreLogo.png", 500],
  ["Square44x44Logo.png", 500],
  ["Square310x310Logo.png", 3000],
];

const fail = (msg) => {
  console.error("失败：" + msg);
  process.exit(1);
};

const step = (msg) => console.log("· " + msg);

// ── 0. 前置检查 ────────────────────────────────────────────────
if (!existsSync(master)) fail("找不到 " + master);
if (!existsSync(cli)) fail("找不到 Tauri CLI，先跑一次 pnpm install");

const svg = readFileSync(master, "utf8");

// 真源必须自足：除了 xml 命名空间，不许出现任何外部地址，也不许内嵌位图
const urls = svg.match(/https?:\/\/[^\s"'<>]+/g) ?? [];
const foreign = urls.filter((u) => !u.startsWith("http://www.w3.org/"));
if (foreign.length) fail("logo.svg 里出现了外部引用：" + foreign.join(", "));
if (/<image[\s>]/.test(svg) || /base64/i.test(svg)) fail("logo.svg 里内嵌了位图");
if (/<script[\s>]/i.test(svg)) fail("logo.svg 里出现了脚本");
step("logo.svg 自检通过（" + svg.length + " 字节，无外部引用）");

// ── 1. 生成应用图标 ────────────────────────────────────────────
const icon = spawnSync(process.execPath, [cli, "icon", master, "-o", iconsDir], {
  cwd: root,
  stdio: ["ignore", "pipe", "inherit"],
  encoding: "utf8",
});
if (icon.status !== 0) fail("tauri icon 退出码 " + icon.status + "（SVG 可能不合法）");

// 本项目不发移动端，CLI 顺手写的两个目录清掉
for (const extra of ["android", "ios"]) {
  const dir = resolve(iconsDir, extra);
  if (dir.startsWith(resolve(iconsDir)) && existsSync(dir)) {
    rmSync(dir, { recursive: true, force: true });
    step("清掉 " + extra + "/");
  }
}

for (const [name, min] of expected) {
  const file = join(iconsDir, name);
  if (!existsSync(file)) fail("没有生成 " + name);
  const size = statSync(file).size;
  if (size < min) fail(name + " 只有 " + size + " 字节，明显不对");
}

// ico 的尺寸清单：Windows 任务栏与资源管理器按这些档位取图
const ico = readFileSync(join(iconsDir, "icon.ico"));
const icoCount = ico.readUInt16LE(4);
const icoSizes = [];
for (let i = 0; i < icoCount; i++) {
  const off = 6 + i * 16;
  icoSizes.push((ico[off] === 0 ? 256 : ico[off]) + "x" + (ico[off + 1] === 0 ? 256 : ico[off + 1]));
}
if (!icoSizes.includes("16x16") || !icoSizes.includes("32x32") || !icoSizes.includes("256x256")) {
  fail("icon.ico 缺少小尺寸档，实际有 " + icoSizes.join(" / "));
}
step("icon.ico 尺寸：" + icoSizes.join(" / "));

// PNG 必须是带 alpha 的真彩色（type 6），否则任务栏上会带白底方块
for (const name of ["icon.png", "32x32.png", "128x128.png"]) {
  const png = readFileSync(join(iconsDir, name));
  const sig = png.subarray(0, 8).toString("hex");
  if (sig !== "89504e470d0a1a0a") fail(name + " 不是 PNG");
  const colorType = png[25];
  if (colorType !== 6) fail(name + " 的颜色类型是 " + colorType + "，不是带 alpha 的 6");
}
step("PNG 均为带 alpha 的真彩色");

const icns = readFileSync(join(iconsDir, "icon.icns"));
if (icns.subarray(0, 4).toString("ascii") !== "icns") fail("icon.icns 文件头不对");
step("icon.icns 文件头正确");

// ── 2. 同步 favicon ───────────────────────────────────────────
for (const target of favicons) {
  copyFileSync(master, target);
  if (readFileSync(target).compare(readFileSync(master)) !== 0) fail(target + " 与真源不一致");
  step("已同步 " + target.replace(root + "\\", "").replace(root + "/", ""));
}

console.log("完成。应用图标 " + expected.length + " 项已更新，favicon 已同步。");
