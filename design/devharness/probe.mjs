// 用 CDP 在真实渲染里量几何越界。仅用于验收，不属于产品代码。
// 用法：node probe.mjs <url> <width> <height> [waitMs]
// 例：node design/devharness/probe.mjs "http://localhost:4181/dist/app.html#services" 900 620
//
// 只报「真溢出」：父级自己可滚（overflow 非 visible）时，超出算滚动区正常内容。

const [url, w, h, waitMsArg, shotPath] = process.argv.slice(2);
const W = Number(w || 900);
const H = Number(h || 620);
const WAIT = Number(waitMsArg || 3500);
const PORT = 9333 + (process.pid % 500);

const EDGE = "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe";

const { spawn } = await import("node:child_process");
const { mkdtempSync, writeFileSync } = await import("node:fs");
const { tmpdir } = await import("node:os");
const { join } = await import("node:path");

const profile = mkdtempSync(join(tmpdir(), "suiyi-probe-"));
const child = spawn(EDGE, [
  "--headless=new",
  "--disable-gpu",
  "--no-sandbox",
  "--hide-scrollbars",
  `--remote-debugging-port=${PORT}`,
  `--user-data-dir=${profile}`,
  `--window-size=${W},${H}`,
  url,
], { stdio: "ignore" });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function targets() {
  const res = await fetch(`http://127.0.0.1:${PORT}/json/list`);
  return res.json();
}

let page = null;
for (let i = 0; i < 40 && !page; i++) {
  try {
    const list = await targets();
    page = list.find((t) => t.type === "page" && t.webSocketDebuggerUrl);
  } catch {
    /* 还没起来 */
  }
  if (!page) await sleep(250);
}
if (!page) {
  child.kill();
  console.log("PROBE-FAIL 打不开调试端口");
  process.exit(1);
}

const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r, { once: true }));

let id = 0;
const pending = new Map();
ws.addEventListener("message", (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.id && pending.has(msg.id)) {
    pending.get(msg.id)(msg);
    pending.delete(msg.id);
  }
});
const send = (method, params) =>
  new Promise((r) => {
    const myId = ++id;
    pending.set(myId, r);
    ws.send(JSON.stringify({ id: myId, method, params }));
  });

const evaluate = async (expr) => {
  const res = await send("Runtime.evaluate", {
    expression: expr,
    returnByValue: true,
    awaitPromise: true,
  });
  if (res.result?.exceptionDetails) return "EVAL-ERR " + JSON.stringify(res.result.exceptionDetails.text);
  return res.result?.result?.value;
};

await send("Runtime.enable");
await sleep(WAIT);

// 覆盖层要拖出一个选区才有东西可看；只按下+移动，不松开，选区就留在画面上
const DRAG = `(() => {
  const root = document.querySelector(".overlay-root");
  if (!root) return "skip";
  const mk = (type, x, y) =>
    root.dispatchEvent(new MouseEvent(type, { clientX: x, clientY: y, bubbles: true }));
  mk("mousedown", 200, 150);
  mk("mousemove", 320, 240);
  mk("mousemove", calcX(), calcY());
  function calcX() { return Math.round(window.innerWidth * 0.62); }
  function calcY() { return Math.round(window.innerHeight * 0.66); }
  return "dragged";
})()`;
await evaluate(DRAG);
await sleep(400);

// 覆盖层只引了自己的 CSS，变量有没有解析出来直接看计算值
const STYLES = `(() => {
  const root = document.documentElement;
  const token = (n) => getComputedStyle(root).getPropertyValue(n).trim() || "(未定义)";
  const cs = (s) => { const el = document.querySelector(s); return el ? getComputedStyle(el) : null; };
  const out = ["--c-primary = " + token("--c-primary"), "--glass-dark-tint = " + token("--glass-dark-tint")];
  const hint = cs(".ov-hint"), size = cs(".ov-size"), sel = cs(".ov-sel");
  if (hint) out.push(".ov-hint  bg=" + hint.backgroundColor + " color=" + hint.color + " font=" + hint.fontSize);
  if (size) out.push(".ov-size  bg=" + size.backgroundColor + " color=" + size.color + " font=" + size.fontSize);
  if (sel) out.push(".ov-sel   border=" + sel.borderTopColor + " " + sel.borderTopWidth);
  return out.join("\\n");
})()`;

const MEASURE = `(() => {
  const shell = document.querySelector(".app-shell, .popup-root, .overlay-root");
  if (!shell) return "找不到根容器（.app-shell / .popup-root / .overlay-root）";
  const rootSel = shell.className;
  const lines = [];
  const name = (el) => el.tagName.toLowerCase() + "." + String(el.className || "").trim().replace(/\\s+/g, ".").slice(0, 40);
  shell.querySelectorAll("*").forEach((el) => {
    const r = el.getBoundingClientRect();
    if (!r.width && !r.height) return;
    const p = el.parentElement;
    if (!p) return;
    const pr = p.getBoundingClientRect();
    const cs = getComputedStyle(p);
    // 父级自己可滚（或裁剪）时，超出属于滚动区正常内容，不算布局事故
    const scrollY = ["auto", "scroll", "hidden"].includes(cs.overflowY);
    const scrollX = ["auto", "scroll", "hidden"].includes(cs.overflowX);
    const d = [];
    if (r.bottom - pr.bottom > 1 && !scrollY) d.push("下溢" + Math.round(r.bottom - pr.bottom));
    if (r.right - pr.right > 1 && !scrollX) d.push("右溢" + Math.round(r.right - pr.right));
    if (pr.top - r.top > 1 && !scrollY) d.push("上溢" + Math.round(pr.top - r.top));
    if (pr.left - r.left > 1 && !scrollX) d.push("左溢" + Math.round(pr.left - r.left));
    if (!d.length) return;
    lines.push(name(el) + " ⇒ 超出 " + name(p) + " {overflow:" + cs.overflowX + "/" + cs.overflowY + "} " + d.join(" "));
  });
  return lines.length ? lines.slice(0, 30).join("\\n") : "无越界";
})()`;

const BOXES = `(() => {
  const pick = [
    ".side", ".main", ".cols", ".list-col", ".form-col", ".fallback", ".seg", ".set-body",
    ".panel", ".panel .f", ".panel .inp", ".row2", ".row2 .f",
  ];
  const out = pick.map((s) => {
    const el = document.querySelector(s);
    if (!el) return s + ": 无";
    const r = el.getBoundingClientRect();
    return s + ": x=" + Math.round(r.x) + " w=" + Math.round(r.width) + " h=" + Math.round(r.height) + " bottom=" + Math.round(r.bottom);
  });
  const main = document.querySelector(".main");
  if (main) out.push("main 滚动高度=" + main.scrollHeight + " 可见高度=" + main.clientHeight);
  return out.join("\\n");
})()`;

console.log("=== 越界 ===");
console.log(await evaluate(MEASURE));
console.log("=== 盒模型 ===");
console.log(await evaluate(BOXES));
console.log("=== 计算样式 ===");
console.log(await evaluate(STYLES));

if (shotPath) {
  await send("Page.enable");
  const shot = await send("Page.captureScreenshot", { format: "png" });
  if (shot.result?.data) {
    writeFileSync(shotPath, Buffer.from(shot.result.data, "base64"));
    console.log("截图已保存: " + shotPath);
  } else {
    console.log("截图失败");
  }
}

ws.close();
child.kill();
