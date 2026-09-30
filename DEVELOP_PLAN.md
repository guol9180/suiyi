# 随译 SuiYi · 开发计划

> Windows 桌面 AI 翻译助手：划词翻译、截图识别、输入框转译、多 AI 服务配置。
> 界面基线：`design/ui-mockup.html`（v0.4）。

## 1. 技术栈

| 层 | 选型 | 说明 |
|---|---|---|
| 应用框架 | Tauri 2 | 轻量跨平台，WebView 渲染前端 |
| 后端 | Rust stable (MSVC) | 业务内核：服务编排、取词、OCR、写回 |
| 前端 | Vite + React + TypeScript | 三个入口：主窗口、划词弹窗、截图覆盖层 |
| 配置存储 | services.json + keyring | API Key 只进系统凭据管理器，配置文件零明文 |
| 网络 | reqwest stream | OpenAI 兼容 `/chat/completions` 与 SSE |
| 取词与按键 | arboard + SendInput | 剪贴板往返取词，SendInput 发送真实虚拟键码 |
| 截屏与 OCR | GDI BitBlt + Windows.Media.OCR | 抓虚拟屏幕冻结帧，离线识别 |
| 后续引入 | rusqlite | M4 历史记录 |

## 2. 目录结构

```
suiyi/
├─ design/                  UI 设计稿（单文件 HTML）
├─ src/                     前端
│  ├─ pages/                翻译页、设置页、划词弹窗、截图覆盖层
│  ├─ styles/               tokens.css（设计变量）、base.css（基础组件）
│  └─ api.ts                所有后端调用的唯一出口
├─ src-tauri/
│  ├─ src/config.rs         services.json 读写与服务 Schema
│  ├─ src/keyring.rs        API Key 存取
│  ├─ src/translator.rs     OpenAI 兼容适配器与 SSE 流式
│  ├─ src/selection.rs      Alt+D 取词与光标处弹窗
│  ├─ src/screenshot.rs     Alt+S 冻结帧、覆盖层、OCR
│  ├─ src/writeback.rs      Alt+T 输入框写回
│  └─ src/commands.rs       Tauri 命令层，前后端唯一通道
└─ DEVELOP_PLAN.md          本文件
```

## 3. 里程碑

### M0 内核闭环 —— 已完成

| 步骤 | 内容 | 验收 |
|---|---|---|
| S0.1 | 环境准备：Rust 与 MSVC Build Tools | `cargo --version` 出版本 |
| S0.2 | Tauri 2 react-ts 脚手架 | `pnpm tauri dev` 弹出窗口 |
| S0.3 | 设计 token 与基础组件样式 | 设置页静态样式就位 |
| S0.4 | 配置层 config.rs 与凭据层 keyring.rs | Rust 单元测试通过 |
| S0.5 | 命令层：服务增删改查、密钥、全局设置 | 前端调通全部命令 |
| S0.6 | 设置页：服务列表与编辑表单 | 配置真实保存、重启不丢 |
| S0.7 | 翻译内核：流式请求与事件推送 | 假服务返回流式文本 |
| S0.8 | 最小翻译界面 | 真实 Key 流式翻译成功 |

### M1 划词翻译 —— 已完成

全局热键 Alt+D；剪贴板往返取词（记录原内容、清空、模拟 Ctrl+C、读回、还原）；
光标处无边框置顶弹窗，屏幕越界回退；失焦自动隐藏，可固定；Esc 关闭。

验收：在浏览器或 PDF 中划词后按 Alt+D 弹出译文。

### M2 截图识别 —— 开发中

Alt+S 抓取虚拟屏幕冻结帧、全屏覆盖层框选、按逻辑坐标裁剪原图、
Windows.Media.OCR 离线识别、识别文本投递给划词弹窗自动翻译。

验收：对屏幕任意区域框选，出识别文本与译文。

### M3 输入框转译与写回 —— 计划中

Alt+T 读取当前焦点输入框内容，翻译后原位写回；写入前做焦点与文本快照校验，
失败则降级为复制并通知；提供撤销窗口；划词弹窗增加替换原文动作；Anki Connect 生词本。

验收：聊天输入框里的中文一键变英文；误切窗口不会写错位置。

### M4 词典结构化、朗读与历史 —— 计划中

结构化词典结果、系统语音朗读、基于 rusqlite 的历史记录与搜索。

### M5 插件系统 —— 计划中

manifest 与 JS 沙箱（QuickJS），翻译、OCR、语音、动作四类扩展点。

### M6 跨平台 —— 计划中

macOS 的辅助功能与录屏权限引导；Linux 以 X11 优先，Wayland 走外部调用方案。

## 4. 开发约定

1. 一个步骤一个 commit，提交信息写清里程碑与范围，不把无关改动混进同一个提交。
2. 依赖只加当前阶段需要的，不预装。
3. API Key 与任何密钥永远不进仓库和配置文件。
4. 每个里程碑结束跑一次完整验收，再开下一个。
5. 界面改动以 `design/ui-mockup.html` 为准；设计稿与实现出现分歧时先改稿再改码。

## 5. 环境现状

- Node v22.23.2、pnpm 12.5.1（`D:\dev\environment\nodejs`）
- Rust 1.98.1，工具链在 `D:\dev\environment\rustup\toolchains\stable-x86_64-pc-windows-msvc`
  - `D:\dev\environment\cargo\bin\cargo.exe` 是 rustup 垫片，需先配好默认 toolchain，或直接调用工具链目录
- 已配置的翻译服务：Z.ai（`https://api.z.ai/api/paas/v4`，模型 `glm-5.3-flash`）
- OCR 语言包：系统仅安装 `zh-Hans-CN`

## 6. 待确认

1. 仓库当前没有 LICENSE，但已经公开。是否补开源协议、选哪一个。
2. 应用图标仍是 Tauri 脚手架默认图标，需要替换为随译自己的图标。
3. OCR 目前优先请求 `en-US` 语言包，本机只有中文包，是否需要改成按内容自适应。

## 7. 开发日志

- 2026-09-29 S0.1 完成：安装 Rust 与 MSVC Build Tools，用 rsproxy 镜像跑通 hello world。
- 2026-09-29 S0.2 完成：Tauri 2 react-ts 脚手架并入现有目录，identifier `com.suiyi.dev`。
- 2026-09-29 S0.3 完成：tokens.css、base.css 迁入设计 token。
- 2026-09-29 S0.4 与 S0.5 完成：配置层、凭据层、命令层，含凭据管理器真实读写测试。
- 2026-09-29 S0.6 完成：设置页服务列表与编辑表单，services.json 自动生成。
- 2026-09-29 S0.7 与 S0.8 完成：OpenAI 兼容流式内核（含 SSE mock 端到端测试）与翻译页。
- 2026-09-29 M0 验收通过：Z.ai Key 与 glm-5.3-flash 真实服务流式翻译成功。
  期间确认 bigmodel.cn 会返回 403 model_access_denied，Z.ai 需使用 `https://api.z.ai/api/paas/v4`。
- 2026-09-30 M1 完成：Alt+D 全局热键、剪贴板取词、光标处无边框弹窗。
  取词最初用 SendInput 注入 Unicode 字符，无法触发目标程序的复制快捷键，改为发送真实虚拟键码。
- 2026-09-30 M2 主体完成：Alt+S 冻结帧、覆盖层框选、GDI 裁剪、Windows.Media.OCR、结果投递弹窗。
  覆盖层一度整屏黑，原因是 JPEG 不支持 Alpha 通道，编码前需把 RGBA 转成 RGB。
- 2026-09-30 设计稿 v0.4：重做配色、字阶与状态设计，补齐失败态与空态，去掉装饰性 emoji。
- 2026-09-30 仓库清理：删除脚手架残留资源与过期的设计稿导出图，设计稿合并为 `design/ui-mockup.html` 单一基线，
  重写 README 与开发计划，修复开发日志的编码损坏。
