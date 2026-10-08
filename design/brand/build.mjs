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

import { spawn, spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
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

// ── 2. 安装向导用图 ───────────────────────────────────────────
//
// 尺寸是安装器定死的，改不了：NSIS 头部 150×57、侧栏 164×314；
// WiX 横幅 493×58、对话框 493×312。而且必须是不带 alpha 的 24bpp BMP。
// 做法：Edge 以 4 倍尺寸渲染一段 HTML（排版写在 CSS 里，改起来直观），
// 再用 GDI+ 缩到目标尺寸写成 BMP，最后校验 BMP 头里的宽高与位深。
const EDGE = "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe";
// 生成到 src-tauri/installer：那份目录同时放 NSIS 的安装钩子脚本，
// 而且 tauri.conf.json 里的路径是相对 src-tauri 解析的，放一起最不容易错。
const artDir = join(root, "src-tauri", "installer");
// 1 = 按目标尺寸直接渲染。
// 之前是 4 倍渲染再缩回去，细笔画被重采样平均成灰色，安装向导里看着发虚；
// 现在 Edge 直接按目标像素渲染，文字由 DirectWrite 在最终尺寸上做抗锯齿，最清楚。
const SCALE = 1;
const WASH = `
  radial-gradient(60% 50% at 12% 6%, rgba(121,165,255,.30), rgba(121,165,255,0) 70%),
  radial-gradient(60% 55% at 88% 94%, rgba(186,170,255,.26), rgba(186,170,255,0) 70%),
  #f5f6f7`;

/** 去掉外层 XML 注释，留下可以直接内联的 <svg> */
const inlineMark = svg.replace(/^<!--[\s\S]*?-->\s*/, "");

/**
 * 页面外壳：按「设计尺寸」排版，再用 transform 整体放大 SCALE 倍交给 Edge 渲染。
 * 这样模板里的 px 就是最终尺寸的 px（改起来直观），缩放由浏览器做，缩回去边缘也干净。
 * 注意 .stage 必须铺满整个窗口——只写设计尺寸的话，截图会拍到右侧和下侧的空区。
 */
function stage(w, h, css, inner) {
  return `<!doctype html><html><head><meta charset="utf-8"><style>
  *{margin:0;padding:0;box-sizing:border-box}
  html,body{width:100vw;height:100vh;overflow:hidden;background:${WASH};
       font-family:"Microsoft YaHei","Segoe UI",sans-serif;color:#0f1115}
  .stage{width:${w}px;height:${h}px;transform:scale(${SCALE});transform-origin:top left;background:${WASH}}
  .mark svg{display:block}
  ${css}
  </style></head><body><div class="stage">${inner}</div></body></html>`;
}

/** 一条横排（头部/横幅）：标记 + 名称 + 一行说明 */
function artRow(w, h, { mark, title, tagline, pad, gap, titleSize, taglineSize }) {
  const css = `
  .stage{display:flex;align-items:center;gap:${gap}px;padding:0 ${pad}px}
  .mark svg{width:${mark}px;height:${mark}px}
  .t{font-size:${titleSize}px;font-weight:600;letter-spacing:.2px}
  .g{margin-top:2px;font-size:${taglineSize}px;color:#3f4650}`;
  const inner = `<div class="mark">${inlineMark}</div>
  <div><div class="t">${title}</div><div class="g">${tagline}</div></div>`;
  return stage(w, h, css, inner);
}

/** 一竖排（侧栏/对话框）：标记居中，下面接名称、说明与可选的要点 */
function artColumn(
  w,
  h,
  { mark, tagline, bullets = [], pad, foot, titleSize, taglineSize, bulletSize, footSize },
) {
  const css = `
  .stage{display:flex;flex-direction:column;align-items:center;justify-content:center;
       gap:${Math.round(mark * 0.14)}px;padding:${pad}px 16px;text-align:center;position:relative}
  .mark svg{width:${mark}px;height:${mark}px}
  .t{font-size:${titleSize}px;font-weight:600;letter-spacing:.3px}
  .g{font-size:${taglineSize}px;color:#3f4650;line-height:1.5}
  ul{margin-top:${Math.round(mark * 0.12)}px;list-style:none;text-align:left}
  li{font-size:${bulletSize}px;color:#2b323c;line-height:1.95;padding-left:16px;position:relative}
  li::before{content:"";position:absolute;left:0;top:.72em;width:6px;height:6px;border-radius:50%;background:#3964fe}
  /* 页脚走正常流 + margin-top:auto：绝对定位会在内容高的时候压到正文上 */
  .foot{margin-top:auto;font-size:${footSize}px;color:#7b828c}`;
  const list = bullets.length ? `<ul>${bullets.map((b) => `<li>${b}</li>`).join("")}</ul>` : "";
  const inner = `<div class="mark">${inlineMark}</div>
  <div class="t">随译 SuiYi</div>
  <div class="g">${tagline}</div>
  ${list}
  ${foot ? `<div class="foot">${foot}</div>` : ""}`;
  return stage(w, h, css, inner);
}

/**
 * 用 Edge 按 1:1 截出目标尺寸的 PNG。
 *
 * 不能直接把 `--screenshot` 的窗口开成目标尺寸：Edge 无头在 57px 高的窗口上会挂住
 * （实测 150×57 超时，其余三个尺寸 0.9 秒就出图）。所以开一个正常大小的窗口，
 * 用 CDP 的 clip 精确截取左上角那块 —— 文字仍是在最终像素尺寸上渲染的，边角最干净。
 */
async function shot(html, w, h, pngPath) {
  const tmp = mkdtempSync(join(tmpdir(), "suiyi-art-"));
  const page = join(tmp, "art.html");
  writeFileSync(page, html, "utf8");
  const port = 9100 + (process.pid % 700);
  const child = spawn(
    EDGE,
    [
      "--headless=new",
      "--disable-gpu",
      "--no-sandbox",
      "--hide-scrollbars",
      "--force-device-scale-factor=1",
      // 关掉 LCD 子像素抗锯齿：图片会被烤成位图，彩边会留在图上
      "--disable-lcd-text",
      `--remote-debugging-port=${port}`,
      `--user-data-dir=${join(tmp, "profile")}`,
      "--window-size=1200,900",
      "file:///" + page.replace(/\\/g, "/"),
    ],
    { stdio: "ignore" },
  );

  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  try {
    let target = null;
    for (let i = 0; i < 40 && !target; i++) {
      try {
        const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
        target = list.find((t) => t.type === "page" && t.webSocketDebuggerUrl);
      } catch {
        /* 还没起来 */
      }
      if (!target) await sleep(250);
    }
    if (!target) fail("Edge 调试端口没起来，无法渲染 " + w + "×" + h);

    const ws = new WebSocket(target.webSocketDebuggerUrl);
    await new Promise((r, j) => {
      ws.addEventListener("open", r, { once: true });
      ws.addEventListener("error", j, { once: true });
      setTimeout(() => j(new Error("连接调试端口超时")), 10000);
    });
    let id = 0;
    const pending = new Map();
    const send = (method, params) =>
      new Promise((r) => {
        const myId = ++id;
        pending.set(myId, r);
        ws.send(JSON.stringify({ id: myId, method, params }));
      });
    ws.addEventListener("message", (ev) => {
      const msg = JSON.parse(ev.data);
      if (msg.id && pending.has(msg.id)) {
        pending.get(msg.id)(msg);
        pending.delete(msg.id);
      }
    });

    // 等页面真的加载完（含字体）。连上调试端口时页面往往还没渲染完，
    // 直接截会拿到一张空帧 —— 整张图会变成黑色。
    await send("Page.enable");
    for (let i = 0; i < 50; i++) {
      const st = await send("Runtime.evaluate", {
        expression: 'document.readyState === "complete" && document.fonts.status === "loaded"',
        returnByValue: true,
      });
      if (st.result?.result?.value === true) break;
      await sleep(100);
    }
    await sleep(200); // 再留一帧给合成器
    const res = await send("Page.captureScreenshot", {
      format: "png",
      clip: { x: 0, y: 0, width: w, height: h, scale: 1 },
    });
    const data = res.result?.data;
    if (!data) fail("截图失败（" + w + "×" + h + "）");
    writeFileSync(pngPath, Buffer.from(data, "base64"));
    ws.close();
  } finally {
    child.kill();
    // Edge 进程退出后还会握着 profile 里的文件一会儿，删不掉就留给系统清理，
    // 不因为一个临时目录把整个生成流程判失败
    await new Promise((r) => setTimeout(r, 300));
    try {
      rmSync(tmp, { recursive: true, force: true });
    } catch {
      /* 忽略 */
    }
  }
}

/** PNG → 目标尺寸的 24bpp BMP，并校验 BMP 头 */
function toBmp(png, bmp, w, h) {
  const r = spawnSync(
    "powershell",
    [
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      join(here, "to-bmp.ps1"),
      "-In",
      png,
      "-Out",
      bmp,
      "-Width",
      String(w),
      "-Height",
      String(h),
    ],
    { encoding: "utf8" },
  );
  if (r.status !== 0 || !existsSync(bmp)) {
    fail("转 BMP 失败：" + (r.stderr || r.stdout || "无输出"));
  }
  const bytes = readFileSync(bmp);
  const bw = bytes.readInt32LE(18);
  const bh = bytes.readInt32LE(22);
  const bpp = bytes.readUInt16LE(28);
  if (bw !== w || bh !== h || bpp !== 24) {
    fail(`${bmp} 的 BMP 头不对：${bw}×${bh} ${bpp}bpp（应为 ${w}×${h} 24bpp）`);
  }
}

async function buildInstallerArt() {
  mkdirSync(artDir, { recursive: true });
  const tmpPng = join(tmpdir(), "suiyi-art-" + process.pid + ".png");
  const jobs = [
    // NSIS
    [
      "header.bmp",
      150,
      57,
      artRow(150, 57, {
        mark: 30,
        title: "随译 SuiYi",
        tagline: "桌面翻译助手",
        pad: 10,
        gap: 8,
        titleSize: 15,
        taglineSize: 10.5,
      }),
    ],
    [
      "sidebar.bmp",
      164,
      314,
      artColumn(164, 314, {
        mark: 62,
        tagline: "选中即译 · 截图即译",
        pad: 26,
        foot: "译文来自你自己的服务",
        titleSize: 19,
        taglineSize: 12,
        bulletSize: 12,
        footSize: 10.5,
      }),
    ],
    // MSI（WiX）
    [
      "banner.bmp",
      493,
      58,
      artRow(493, 58, {
        mark: 34,
        title: "随译 SuiYi",
        tagline: "选中即译 · 截图即译",
        pad: 18,
        gap: 12,
        titleSize: 17,
        taglineSize: 11.5,
      }),
    ],
    [
      "dialog.bmp",
      493,
      312,
      artColumn(493, 312, {
        // 尺寸按「不溢出」倒推：上下 padding 26 + 标记 76 + 段间距 + 标题/副标题 +
        // 两条单行要点 + 页脚 ≈ 300，落在 312 里不会挤到贴边
        mark: 76,
        tagline: "选中即译 · 截图即译 · 输入框直译",
        bullets: [
          "Alt+D 划词翻译，Alt+S 截图识别，Alt+T 输入框转译",
          "密钥只存在本机凭据管理器，截图识别不上传",
        ],
        pad: 26,
        foot: "译文来自你自己的服务",
        titleSize: 24,
        taglineSize: 14,
        bulletSize: 14,
        footSize: 10.5,
      }),
    ],
  ];
  for (const [name, w, h, html] of jobs) {
    await shot(html, w * SCALE, h * SCALE, tmpPng);
    if (process.env.SUIYI_KEEP_ART_PNG) {
      copyFileSync(tmpPng, join(tmpdir(), "suiyi-art-" + name + ".png"));
    }
    toBmp(tmpPng, join(artDir, name), w, h);
    step(`安装向导图 ${name}（${w}×${h}，24bpp）`);
  }
  rmSync(tmpPng, { force: true });
}

await buildInstallerArt();

// ── 3. 同步 favicon ───────────────────────────────────────────
for (const target of favicons) {
  copyFileSync(master, target);
  if (readFileSync(target).compare(readFileSync(master)) !== 0) fail(target + " 与真源不一致");
  step("已同步 " + target.replace(root + "\\", "").replace(root + "/", ""));
}

console.log("完成。应用图标 " + expected.length + " 项已更新，favicon 已同步。");
