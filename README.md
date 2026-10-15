# 随译 SuiYi

Windows 桌面 AI 翻译工具：划词翻译（`Alt+D`）与截图识别（`Alt+S`）两个全局入口，
多 AI 服务并发对比，译文来自你自己配置的服务。Tauri 2 + Rust + React 19。

> 文档分工：本文件是项目概览与技术决策索引；开发计划与验收标准看
> [DEVELOP_PLAN.md](./DEVELOP_PLAN.md)；开发规范看 [AGENTS.md](./AGENTS.md)。

## 下载

下载页 <https://suiyi.imhgl.com/> · 全部版本 <https://github.com/guol9180/suiyi/releases>
（NSIS / MSI 安装包与免安装 zip。安装包未签名，首次运行在 SmartScreen 提示里
点「更多信息」→「仍要运行」。）

## 技术决策

每条都是有意的取舍，实现细节与验收标准见对应源码与 [DEVELOP_PLAN.md](./DEVELOP_PLAN.md)。

**五个 WebView，各自独立**：主窗、划词弹窗、全屏冻结覆盖层、OCR 结果面板、轻提示
各自是独立 WebView，状态不共享、事件全广播由各窗口自行过滤。全局热键在 Rust 端
逐个注册——插件的 `with_shortcuts` 是「全成或全不成」，一个被占用会让另外两个一起失效。

**截图识别管线**（`src-tauri/src/screenshot.rs`）：Alt+S 用 GDI BitBlt 抓全部显示器的
物理像素冻结帧，前端在冻结帧上框选——按图像自然尺寸换算坐标，不碰各屏 DPI 设置，
混合缩放的多屏天然不错位。识别主路**紧贴选区裁剪**：识别范围与框选范围构造上相等，
框住半行就只返回半行；紧框识别为空（贴边小字、条状单行）才外扩 32px 重试，
行级过滤加交集精修兜底。引擎链 PaddleOCR（PP-OCRv4 + ONNX Runtime，随包分发、
完全离线）→ 回落 Windows 自带 OCR，识别不可能整个不可用。识别与翻译解耦：
结果先给用户，复制还是翻译、翻成什么语言由用户决定，不自动消耗额度。

**划词取词**（`src-tauri/src/selection.rs`）：剪贴板法。先等用户松开热键上的修饰键
（逐个体检左右键码——替用户补抬键会打乱目标程序的按键状态），SendInput 发 Ctrl+C，
80ms 粒度轮询读回，用完恢复原剪贴板。取词失败不静默：区分「前台是自己占用了屏幕」
「目标程序提权被 UIPI 拦截」「目标不响应模拟按键」三种原因并给出出路。

**翻译内核**（`src-tauri/src/translator.rs`）：OpenAI 兼容 `/chat/completions` 适配器，
SSE 逐块解析、增量上屏。全局共享 reqwest 连接池，启动时对已启用网关预热握手——
每次翻译不再重付 DNS + TLS。请求带与输入相称的 `max_tokens` 上限（字符数 ×2 + 128，
下限 256），模型跑偏复读时收敛长尾。非流式长文本（>500 字）按段并发、按序拼回；
流式服务不分段——增量上屏的顺序体验优先于总耗时。词典结构化模式强制非流式，
拿完整 JSON 才可解析。

**并发与容错**：多服务一次并发请求、各自流式出结果卡，单服务失败单独报错，
不影响其他服务。错误按状态码翻成人话并给修复动作，服务端原文折叠可查。

**数据与安全**：API Key 只进系统凭据管理器，配置文件零明文；凭据读取走进程内缓存
（凭据管理器是同步 COM 调用），写删同步更新。services.json 按 mtime 进程内缓存、
临时文件加改名原子写。历史落 SQLite（bundled、WAL），滚动保留 2000 条。

**插件沙箱**（`src-tauri/src/plugin_js.rs`）：QuickJS 每次调用新建 Runtime，
64MB 内存与栈深限制，无网络能力，宿主 API 白名单。清单校验失败的插件照常列出
并带原因——静默消失比报错更难排查。

**构建资产**：OCR 模型（约 16MB）不进仓库，构建脚本按 SHA256 拉取校验；
应用图标、favicon 与安装向导图由单一 SVG 真源生成。

## 快速开始

1. Windows 10 需装 WebView2 运行时（Windows 11 自带）。
2. 设置 → 服务配置：选内置提供商会自动填好地址与推荐模型，任何 OpenAI 兼容端点
   也能手填；API Key 只写进凭据管理器。
3. 「测试连接」用你配的模型**真实跑一次翻译**——`/models` 探不出模型权限，
   「测试通过但真翻译 403」就是这么防住的。
4. 任意应用选中文字按 `Alt+D`；屏幕内容按 `Alt+S` 框选。

同时启用多个服务时，每次翻译并发消耗这几个服务的额度。

## 从源码构建

需要 Node 18 以上、pnpm、Rust stable（MSVC 工具链）。

```bash
pnpm install
pnpm tauri dev      # 整体开发调试
pnpm dev            # 只起前端（端口 1420）
pnpm tauri build    # 打包安装程序
pnpm build          # 只构建前端（tsc + vite）
```

后端检查与测试在 `src-tauri` 下执行：

```bash
cargo check
cargo test
cargo test real_service_smoke -- --ignored --nocapture   # 真实服务冒烟，需已配置 Key
```

少数用例依赖宿主能力，能力缺失时打印原因并跳过而不是失败：凭据管理器需要真实
登录会话，OCR 需要系统语言包，语音合成需要可用音色。

应用图标与安装向导图改了 `design/brand/logo.svg` 后重新生成：

```bash
node design/brand/build.mjs   # 需要本机装有 Edge（用它把 SVG 渲染成位图）
```

OCR 模型不进仓库，构建前由脚本按 SHA256 拉到 `src-tauri/ocr/`
（`pnpm dev` / `pnpm build` 会自动跑，直接 `cargo build` 前需手动一次）：

```bash
pnpm ocr:assets     # 拉模型与 ONNX Runtime
pnpm ocr:check      # 只校验，不下载
```

打 `v*` 标签会触发 `.github/workflows/release.yml`：GitHub 的 Windows 机器上
跑测试、构建并发布，下载页同步部署。

## 项目结构

```
suiyi/
├─ docs/                  下载页（GitHub Pages 从这里发布）
├─ design/                界面设计稿：ui-redesign.html（v1.0，已实现）与 ui-mockup.html（基线）
├─ src/                   前端：React + TypeScript（页面、组件、样式）
├─ src-tauri/             后端：Rust（Tauri 命令层与业务内核）
├─ scripts/               构建辅助脚本（OCR 模型拉取）
└─ .github/workflows/     发布与下载页的 CI
```

逐文件的职责说明见 [DEVELOP_PLAN.md](./DEVELOP_PLAN.md) 的「目录结构」。

## 技术栈

| 层 | 选型 |
|---|---|
| 应用框架 | Tauri 2 |
| 后端 | Rust（stable，MSVC） |
| 前端 | Vite + React 19 + TypeScript |
| 网络 | reqwest（全局连接池 + SSE 流式） |
| OCR | PaddleOCR PP-OCRv4 + ort（回落 Windows.Media.OCR） |
| 取词与按键 | arboard、SendInput |
| 语音 | Windows.Media.SpeechSynthesis |
| 历史 | rusqlite（bundled SQLite，WAL） |
| 插件沙箱 | rquickjs（QuickJS） |
| 配置存储 | services.json（mtime 缓存）+ keyring（Key 只进系统凭据管理器） |

配置、历史与插件都放在 `%APPDATA%/com.suiyi.dev/` 下：`services.json`、`history.db`、`plugins/`。

## 已知限制

- 只在 Windows 上构建与验证过；macOS 与 Linux 尚未实现，代码里只有
  「当前平台不支持」的提示。
- OCR 用 PP-OCRv4 mobile 中英文模型：中英混排与常见符号够用，手写体、竖排与
  极小字号仍会错；回落 Windows 自带 OCR 时识别质量明显下降，且其回落语言
  固定请求 en-US 语言包。
- `Alt+D` 在 Chrome / Edge 里是「定位到地址栏」：浏览器里划词需改成
  `Ctrl+Alt+D`（设置 → 热键），否则选区会先被浏览器抢走。
- 插件不能联网，只能读写剪贴板（需声明权限）——联网扩展点要等把异步 HTTP
  桥接进 QuickJS 沙箱，不做半成品。
- 截图冻结所有显示器；混合 DPI（两块屏缩放不同）时覆盖层提示文字在副屏上
  略大或略小，裁剪本身不受影响。

## 相关文档

- [DEVELOP_PLAN.md](./DEVELOP_PLAN.md)：开发计划与验收标准
- [AGENTS.md](./AGENTS.md)：开发规范（动手改代码前先读）
- [design/ui-redesign.html](./design/ui-redesign.html)：工作台 / 历史 / 划词弹窗 / 截图识别的设计稿（v1.0，已实现）
- [design/ui-mockup.html](./design/ui-mockup.html)：设置等其余页面的设计基线（单文件，浏览器直接打开）
- [docs/index.html](./docs/index.html)：下载页源码

## 许可

MIT，全文见 [LICENSE](./LICENSE)。安装包未签名，正在申请
[SignPath Foundation](https://signpath.org/) 开源签名（材料与流程见
[docs/signing.md](./docs/signing.md)），接入后产物名与下载链接不变。
