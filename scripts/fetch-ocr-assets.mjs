/*
 * 下载截图识别用的 PaddleOCR（PP-OCRv4 mobile）ONNX 模型。
 *
 * 为什么要自己拉：模型一共 15MB 左右，放进 git 会让仓库永久变大；但应用要能离线识别，
 * 所以构建前把它们拉到 src-tauri/ocr/，再由 tauri.conf.json 的 bundle.resources 打进安装包。
 * 同一目录里还要放 ONNX Runtime 的 onnxruntime.dll —— 运行时由 ocr.rs 显式加载，
 * 不走 ort 的构建期下载，这样装到哪台机器都不会出现「DLL 找不到」。
 *
 * 只信任 ModelScope 的 RapidOCR 官方转换件（Apache-2.0），每个文件按 SHA256 校验：
 * 拉下来的东西只要和这里记的对不上就直接失败，不写进目录。
 * HuggingFace 在本机不可达，所以不做备用源；换源要连同哈希一起改。
 *
 * 用法：
 *   node scripts/fetch-ocr-assets.mjs           # 缺什么拉什么，已有且哈希正确就跳过
 *   node scripts/fetch-ocr-assets.mjs --check   # 只校验不下载（CI 里用）
 */
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = join(repoRoot, "src-tauri", "ocr");
const api = "https://modelscope.cn/models/RapidAI/RapidOCR/resolve/master/";
const apiFallback = "https://modelscope.cn/api/v1/models/RapidAI/RapidOCR/repo?Revision=master&FilePath=";

/**
 * ONNX Runtime 运行时。取官方 PyPI wheel：13.7MB，比 GitHub 上那个 72MB 的 zip 小得多，
 * 里面就一个 onnxruntime/capi/onnxruntime.dll。wheel 是 zip，用 Windows 自带的 tar.exe 解。
 * 换版本时连同 sha256 一起改。
 */
const RUNTIME = {
  file: "onnxruntime.dll",
  inner: "onnxruntime/capi/onnxruntime.dll",
  url: "https://files.pythonhosted.org/packages/3e/3b/986ca67c274932ba9ac5332fb10de56f643dfd433c74e33f8ae8f847cf24/onnxruntime-1.28.0-cp312-cp312-win_amd64.whl",
  bytes: 13755036,
  sha256: "c35064f9b3c43c81c5d5d282091401d0f1ff22796d93ccade4ea2ece5e137ab8",
};

/** 目标文件名 → { 源、字节数、sha256 }。哈希与体积来自 ModelScope 上的文件信息 */
const ASSETS = [
  {
    file: "det.onnx",
    source: "onnx/PP-OCRv4/det/ch_PP-OCRv4_det_mobile.onnx",
    bytes: 4745517,
    sha256: "d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9",
  },
  {
    file: "rec.onnx",
    source: "onnx/PP-OCRv4/rec/ch_PP-OCRv4_rec_mobile.onnx",
    bytes: 10857958,
    sha256: "48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b",
  },
  {
    file: "cls.onnx",
    source: "onnx/PP-OCRv4/cls/ch_ppocr_mobile_v2.0_cls_mobile.onnx",
    bytes: 585532,
    sha256: "e47acedf663230f8863ff1ab0e64dd2d82b838fceb5957146dab185a89d6215c",
  },
];

const checkOnly = process.argv.includes("--check");

function sha256Of(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

async function download(url, bytes) {
  const res = await fetch(url, { redirect: "follow" });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const buf = Buffer.from(await res.arrayBuffer());
  if (bytes && buf.length !== bytes) {
    throw new Error(`体积不符：期望 ${bytes} 字节，实际 ${buf.length}`);
  }
  return buf;
}

async function ensure(asset) {
  const target = join(outDir, asset.file);
  if (existsSync(target)) {
    const got = sha256Of(target);
    if (got === asset.sha256) {
      console.log(`✓ ${asset.file} 已就绪`);
      return true;
    }
    if (checkOnly) {
      console.error(`✗ ${asset.file} 哈希不符：${got}`);
      return false;
    }
    console.log(`… ${asset.file} 哈希不符，重新下载`);
    rmSync(target, { force: true });
  } else if (checkOnly) {
    console.error(`✗ 缺少 ${asset.file}（跑 pnpm ocr:assets 拉一次）`);
    return false;
  }

  let buf;
  try {
    buf = await download(api + asset.source, asset.bytes);
  } catch (e) {
    console.log(`… 直链失败（${e.message}），改用接口地址`);
    buf = await download(apiFallback + asset.source, asset.bytes);
  }
  const got = createHash("sha256").update(buf).digest("hex");
  if (got !== asset.sha256) {
    throw new Error(`${asset.file} 校验失败：期望 ${asset.sha256}，实际 ${got}`);
  }
  // 先写临时文件再改名：中途失败不会留下一个哈希不对但看着存在的模型
  const part = `${target}.part`;
  writeFileSync(part, buf);
  renameSync(part, target);
  console.log(`✓ ${asset.file} 下载完成（${(buf.length / 1048576).toFixed(1)} MB）`);
  return true;
}

mkdirSync(outDir, { recursive: true });
let ok = true;
for (const asset of ASSETS) {
  try {
    if (!(await ensure(asset))) ok = false;
  } catch (e) {
    console.error(`✗ ${asset.file} 获取失败：${e.message}`);
    ok = false;
  }
}

/** ONNX Runtime：下 wheel（按整包哈希校验）→ 用 tar.exe 解出 onnxruntime.dll */
async function ensureRuntime() {
  const target = join(outDir, RUNTIME.file);
  if (existsSync(target)) {
    console.log(`✓ ${RUNTIME.file} 已就绪`);
    return true;
  }
  if (checkOnly) {
    console.error(`✗ 缺少 ${RUNTIME.file}（跑 pnpm ocr:assets 拉一次）`);
    return false;
  }
  const buf = await download(RUNTIME.url, RUNTIME.bytes);
  const got = createHash("sha256").update(buf).digest("hex");
  if (got !== RUNTIME.sha256) {
    throw new Error(`onnxruntime wheel 校验失败：期望 ${RUNTIME.sha256}，实际 ${got}`);
  }
  const work = mkdtempSync(join(tmpdir(), "suiyi-ort-"));
  try {
    const wheel = join(work, "onnxruntime.whl");
    writeFileSync(wheel, buf);
    // Windows 自带的 bsdtar 能解 zip；只取需要的那一个文件
    execFileSync("tar", ["-xf", wheel, "-C", work, RUNTIME.inner], { stdio: "inherit" });
    const extracted = join(work, ...RUNTIME.inner.split("/"));
    if (!existsSync(extracted)) throw new Error(`wheel 里没有 ${RUNTIME.inner}`);
    // 临时目录可能在另一个盘，跨盘 rename 会 EXDEV，所以走复制
    copyFileSync(extracted, target);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
  console.log(`✓ ${RUNTIME.file} 就绪（${(readFileSync(target).length / 1048576).toFixed(1)} MB）`);
  return true;
}

try {
  if (!(await ensureRuntime())) ok = false;
} catch (e) {
  console.error(`✗ ${RUNTIME.file} 获取失败：${e.message}`);
  ok = false;
}

if (!ok) {
  console.error("OCR 模型不完整：截图识别会回落到 Windows 自带 OCR。");
  process.exit(checkOnly ? 1 : 0); // 正常构建不因为拉不到模型而失败，只是没有 Paddle 引擎
}
console.log("OCR 模型就绪。");
