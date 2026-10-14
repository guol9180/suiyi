# 随译 SuiYi · 开发计划

> Windows 桌面 AI 翻译助手：划词翻译、截图识别，多 AI 服务配置。
> 界面基线：`design/ui-mockup.html`（v0.8，与 v0.9.2 实现对齐）；重设计提案 `design/ui-redesign.html`（v1.0，未实现）。
> 开发规范与避坑细则见 `AGENTS.md`。

## 1. 技术栈

| 层 | 选型 | 说明 |
|---|---|---|
| 应用框架 | Tauri 2 | 五个 WebView 入口：主窗口、划词弹窗、截图覆盖层、OCR 结果窗、轻提示 |
| 后端 | Rust stable (MSVC) | 业务内核：服务编排、取词、截图、OCR、朗读、历史、更新 |
| 前端 | Vite + React + TypeScript | React 19；页面在 `src/pages`，命令出口统一在 `src/api.ts` |
| 配置存储 | services.json + keyring | API Key 只进系统凭据管理器，配置文件零明文 |
| 网络 | reqwest stream | OpenAI 兼容 `/chat/completions` 与 SSE 流式 |
| 取词与按键 | arboard + SendInput | 剪贴板往返取词，发送真实虚拟键码 |
| 截屏与 OCR | GDI BitBlt + PaddleOCR | 本地 PP-OCRv4 离线识别，失败回落 Windows.Media.OCR |
| 历史与生词本 | rusqlite (WAL) | `history.db`，保留最近 2000 条 |
| 插件沙箱 | rquickjs (QuickJS) | 四类扩展点，仅 clipboard 权限，无网络 |
| 朗读 | Windows.Media.SpeechSynthesis | 本地合成 → 临时 WAV → PlaySoundW 异步播放 |

## 2. 目录结构

```
suiyi/
├─ AGENTS.md                 开发规范与避坑手册（先读）
├─ design/
│  ├─ ui-mockup.html         UI 基线稿（与实现对齐）
│  └─ ui-redesign.html       v1.0 重设计提案（未实现）
├─ src/
│  ├─ api.ts                 所有后端调用的唯一出口
│  ├─ pages/                 Translate、Settings、Popup、OcrResult、Overlay、Notice、About
│  ├─ components/            Icon、IconSprite、DictionaryCard、SpeechBar、ModelSelect
│  └─ styles/                tokens.css（设计变量）、base.css（基础组件）
├─ src-tauri/
│  ├─ src/commands.rs        Tauri 命令层，前后端唯一通道
│  ├─ src/config.rs          services.json 读写与服务 Schema
│  ├─ src/translator.rs      OpenAI 兼容适配器与 SSE 流式
│  ├─ src/selection.rs       Alt+D 取词与光标处弹窗
│  ├─ src/screenshot.rs      Alt+S 冻结帧、覆盖层框选、OCR 流程与结果窗
│  ├─ src/ocr.rs             PaddleOCR 引擎加载与推理
│  ├─ src/speech.rs          本地语音合成与播放
│  ├─ src/history.rs         历史记录（SQLite）
│  ├─ src/wordbook.rs        生词本（本地 + AnkiConnect 同步）
│  ├─ src/anki.rs            AnkiConnect 协议
│  ├─ src/plugin.rs          插件清单与发现
│  ├─ src/plugin_js.rs       插件 QuickJS 沙箱
│  ├─ src/update.rs          检查更新、下载、拉起安装向导
│  ├─ src/tray.rs            系统托盘
│  ├─ src/notice.rs          轻提示窗口
│  ├─ src/hotkeys.rs         全局热键注册与冲突重试
│  ├─ src/keyring.rs         API Key 存取
│  └─ src/lib.rs             插件装配、窗口与热键初始化
└─ DEVELOP_PLAN.md           本文件
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

### M2 截图识别 —— 已完成

Alt+S 冻结所有显示器、全屏覆盖层框选（松开即提交，无确认按钮）、按冻结帧像素
坐标裁剪、本地 PaddleOCR 离线识别（失败自动回落 Windows.Media.OCR）、识别文本
投递结果窗并自动并发翻译。选区贴边时向外多取一圈像素再按行映射回框内。

验收：对屏幕任意区域框选，出识别文本与译文；未框到文字时给出重试出口。

### M3 划词替换与生词本 —— 已完成

- 划词弹窗「替换原文」：取词时记下目标窗口句柄，替换前把焦点送回去并校验，
  校验不过就取消并恢复剪贴板。
- 生词本：通过 AnkiConnect 把「原文 → 第一条译文」推进本地 Anki
  （牌组不存在会自动创建，重复词条按「已存在」处理不报错）。
  设置 → 通用 里可配置 AnkiConnect 地址与牌组，并有一键连通性探测。
- 输入框转译（Alt+T 写回）v0.8.1 起从产品移除：全局热键改写用户编辑器内容的
  焦点风险与剪贴板残留代价大于收益；相关代码与界面已清理，
  历史数据中的旧「输入框」记录仍保留展示。

验收边界：AnkiConnect 协议层已用假服务端做端到端测试（探测版本、牌组缺失自动创建、
写入词条、重复词条识别为已存在、连不上时的报错），**真实 Anki 的联调仍需人工确认一次**。

### M4 词典结构化、朗读与历史 —— 已完成

- 词典结构化：服务的结果类型设为「词典结构」时，使用内置 JSON 模板并强制非流式，
  解析出词条、音标、义项（词性、释义、例句）。解析失败会保留模型原始输出并回退为纯文本，
  不会因为模型多说了两句就报错。
- 朗读：Windows 本地语音合成（`SpeechSynthesizer`）→ 临时 WAV → `PlaySoundW` 异步播放。
  选本地引擎是有意的：朗读不消耗翻译额度，也不要求联网。合成与播放拆开，
  合成部分可以无副作用地验收。语音合成页已提供音色选择与 0.8× / 1.0× / 1.5× 语速。
- 历史记录：落 SQLite（`history.db` 与 services.json 同目录），记录原文、译文、
  来源（划词 / 截图 / 手动输入）、服务、耗时与失败原因；支持在原文与译文中模糊搜索、
  按来源与服务过滤、删除单条与清空；失败记录同样入库，插入后按时间裁到最近 2000 条。

### M5 插件系统 —— 已完成

插件目录：`%APPDATA%/com.suiyi.dev/plugins/<插件目录>/manifest.json`

- **清单**：id、name、version、description、author、main、kind、permissions。
- **校验**：入口脚本必须落在自己的插件目录内（防路径穿越）；kind 与权限必须在白名单里，
  出现未知项直接拒绝而不是忽略——不能让插件以为自己拿到了没拿到的能力。
  校验失败的插件也会列出来并带上原因，静默消失比报错更难排查。
- **沙箱**：QuickJS（rquickjs），每次调用新建 Runtime，限制 64 MB 内存与栈深；
  出入参都走 JSON；宿主 API 挂在全局 `host` 上。
- **四类扩展点**：
  - `translation`：`translate({ text, from, to })`。通过校验并启用后自动出现在服务列表里，
    直接参与多服务并发对比，不需要在服务配置页再配一遍；
  - `ocr`：`ocr({ pngBase64 })`。框选裁剪后优先交给插件，插件失败自动回落到系统离线 OCR；
  - `speech`：`speak({ text })`。装了语音插件就优先用插件朗读，否则用本地合成；
  - `action`：`run({ source, translated })`。出现在划词弹窗的「更多」菜单里，返回的文本作为提示。
- **权限**：目前只有 `clipboard`（读写剪贴板）。插件**没有网络能力**——
  需要联网的扩展点要等把异步 HTTP 桥接进沙箱，这里不做半成品。
- **界面**：设置 → 插件，可查看插件目录、启停插件、一键生成可用的示例插件；
  服务列表里的插件服务带「插件」标记，并且不能在服务配置页编辑或排序。

### M6 跨平台 —— 计划中

macOS 的辅助功能与录屏权限引导；Linux 以 X11 优先，Wayland 走外部调用方案。

## 4. 开发约定

1. 一个步骤一个 commit，提交信息写清里程碑与范围，不把无关改动混进同一个提交。
2. 依赖只加当前阶段需要的，不预装。
3. API Key 与任何密钥永远不进仓库和配置文件。
4. 每个里程碑结束跑一次完整验收，再开下一个。
5. 界面改动以 `design/ui-mockup.html` 为准；设计稿与实现出现分歧时先改稿再改码。
6. 环境细则、验证要求与避坑清单以 `AGENTS.md` 为准（渲染验证、grep 复查、CSS 注释禁 `*/` 等）。

## 5. 环境现状

- Node v22.23.2、pnpm 12.5.1（`D:\dev\environment\nodejs`）；pnpm 需用 `pnpm.cmd` 调用（执行策略拦截 .ps1）。
- Rust 1.98.1，工具链在 `D:\dev\environment\rustup\toolchains\stable-x86_64-pc-windows-msvc`
  - `D:\dev\environment\cargo\bin\cargo.exe` 是 rustup 垫片，需先配好默认 toolchain，或直接调用工具链目录
- 已配置的翻译服务：Z.ai（`https://api.z.ai/api/paas/v4`，模型 `glm-5.3-flash`）
- OCR 语言包：系统仅安装 `zh-Hans-CN`
- `pnpm build` / `cargo test` / `pnpm ocr:assets` 已验证通过；OCR 模型已就位（`src-tauri/ocr/`）

## 6. 待确认

1. ~~仓库当前没有 LICENSE~~ 已解决：采用 MIT，`LICENSE` 已入库（v0.9.0）。
2. Windows 回落 OCR 仍优先请求 `en-US` 语言包（本机只有中文包），是否改为按内容自适应——
   本地 PaddleOCR 主路径中英混排后此问题只影响回落场景，保留观察。
3. `design/ui-redesign.html`（v1.0 工作台重设计）是否实施、分几步实施，待定。

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
- 2026-09-30 M2 验收：截图识别链路修通。曾同时挂着四个缺陷——会话状态被劈成
  static 与托管状态两把锁、OCR 在 STA 线程上初始化 COM 失败、DataWriter 没有绑定
  输出流、覆盖层位置落在屏幕原点。多屏策略改为只截鼠标所在的那块显示器。
- 2026-09-30 设计系统落地：tokens/base 迁移到 v0.4，26 个线性图标替换全部 emoji，
  划词弹窗语言行独立并加互换、动作条分层，设置页重排并新增「测试连接」命令。
- 2026-09-30 M3 接线：`writeback.rs` 从死文件变成可用功能，补 Ctrl+A/Ctrl+V 模拟、
  注册 Alt+T、增加写回失败降级与弹窗告知；划词弹窗的「替换原文」同期接通。
- 2026-09-30 M4 起步：词典结构化（词典结构结果类型走 JSON 模板、解析失败回退纯文本）
  与本地语音朗读（合成与播放分离，合成有无副作用验收测试）。
- 2026-09-30 品牌资产：用 `tauri icon` 从品牌图重新生成全套应用图标，
  替换掉脚手架默认图标；顺带删掉工具附带的 Android / iOS 图标目录（本项目不做移动端）。
- 2026-09-30 M4 完成：新增历史记录（rusqlite + WAL，含搜索、按来源过滤、裁剪到 2000 条），
  翻译页 / 划词弹窗 / 截图识别 / 输入框转译四条链路都会归类入库。
- 2026-09-30 M5 完成：插件系统落地。manifest 校验（含路径穿越与权限白名单）、
  QuickJS 沙箱、四类扩展点，以及设置里的插件页；翻译插件直接进入服务列表参与并发对比。
- 2026-09-30 M3 收尾：接入 AnkiConnect 生词本（后端模块 + 弹窗动作 + 设置项），
  并补上对假服务端的协议级端到端测试；真实 Anki 的联调留待人工。
- 2026-10-01 v0.2.0–v0.4.0：多服务并发对比与服务排序、服务商预设添加
  （选中即填地址 / 协议 / 推荐模型）、热键可改与占用提示、托盘与关闭行为、
  关于页雏形。详细变更以 GitHub Releases 为准。
- 2026-10-08 v0.4.1–v0.7.0：轻提示窗口、大区域跳过 OCR 放大重试、
  模型下拉自动拉取（自定义下拉，分「服务端返回 / 预设推荐」两组）、
  服务页默认进提供商选择、关于页、热键自占用修复。
- 2026-10-09 v0.8.0：取词 / OCR / 写回三处修复，界面整理，关于页更新与关闭行为。
- 2026-10-09 v0.8.1：移除输入框转译（Alt+T），窗口 × 改为询问「直接关闭 / 收进托盘」，
  修复截图第二张仍是旧图。
- 2026-10-09 v0.9.0：截图识别改用本地 PaddleOCR（回落系统 OCR）、更新下载修复、
  「测试连接」改为真实跑一次翻译、采用 MIT 许可；交给 ort 前先探一次 DLL 可加载性。
- 2026-10-09 v0.9.1：结果面板移除「原图覆盖」入口，只留原文与译文。
- 2026-10-13 v0.9.2：更新下载进度只增不减、断线重试停在高水位；
  发布 CI 显式覆盖同名资产。
- 2026-10-14 设计稿 v0.8：`design/ui-mockup.html` 与 v0.9.2 实现逐节对齐
  （系统标题栏、设置页签结构、token 实值、窗口参数、移除功能清场）。
  期间发现并修复 CSS 注释含 `*/` 导致 `:root` 整体失效、设计稿渲染全白的问题，
  并确立规范：CSS 注释禁 `*/`、版式改动必须渲染截图验收（已录入 AGENTS.md）。
- 2026-10-14 新增 `design/ui-redesign.html` v1.0 重设计提案（工作台布局：
  翻译 / 历史 / 设置一级页签、服务识别色条、历史日期分组；未实现）。
- 2026-10-14 新增 `AGENTS.md` 开发规范；本文件全面更新至 v0.9.2 现状。
