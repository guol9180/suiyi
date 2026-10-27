# 随译 SuiYi · 开发计划

> Windows 桌面 AI 翻译工具：划词翻译、截图识别，多 AI 服务并发对比。
> 本文件管开发计划与验收标准；项目概览与技术栈见 `README.md`，开发规范见 `AGENTS.md`。
> 界面：工作台 / 历史 / 划词弹窗 / 截图识别见 `design/ui-redesign.html`（v1.0，已实现）；
> 设置等其余页面基线 `design/ui-mockup.html`。

## 1. 目录结构

```
suiyi/
├─ README.md                 项目概览、构建与使用文档
├─ AGENTS.md                 开发规范（动手前先读）
├─ design/
│  ├─ ui-mockup.html         UI 基线稿（与实现对齐）
│  └─ ui-redesign.html       v1.0 重设计提案（未实现）
├─ scripts/
│  └─ fetch-ocr-assets.mjs   OCR 模型与 ONNX Runtime 拉取
├─ src/
│  ├─ api.ts                 所有后端调用的唯一出口
│  ├─ pages/                 Translate、Settings、Popup、OcrResult、Overlay、Notice、About
│  ├─ components/            Icon、IconSprite、DictionaryCard、SpeechBar、ModelSelect
│  └─ styles/                tokens.css（设计变量）、base.css（基础组件）
├─ src-tauri/
│  ├─ src/main.rs            程序入口
│  ├─ src/lib.rs             插件装配、窗口与热键初始化
│  ├─ src/commands.rs        Tauri 命令层，前后端唯一通道
│  ├─ src/config.rs          services.json 读写与服务 Schema
│  ├─ src/keyring.rs         API Key 存取
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
│  ├─ src/hotkeys.rs         全局热键注册与冲突重试
│  ├─ src/update.rs          检查更新、下载、拉起安装向导
│  ├─ src/tray.rs            系统托盘
│  └─ src/notice.rs          轻提示窗口
└─ DEVELOP_PLAN.md           本文件
```

## 2. 里程碑

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
投递结果窗。识别主路紧贴选区裁剪（框哪识别哪，无整行泄漏）；紧框识别为空时
才外扩一圈重试，按行过滤加交集精修兜底。识别与翻译解耦：结果窗默认只出原文
（可复制），由用户选目标语言点「翻译」后再并发翻译。

验收：对屏幕任意区域框选，出识别文本；框住一行中间的片段只返回片段；
点「翻译」后出各服务译文，切语言可重翻；未框到文字时给出重试出口。

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
  记录时间取**翻译开始时刻**（同一轮的多条服务记录共享它），列表排序才与翻译轮次一致。

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

### M7 翻译提速（第一批）—— 已完成

对照 Manggo/Pot 的调研结论落地的三批改动：

- **连接复用**：`translator::http_client()` 全局共享 reqwest 客户端（连接池按 host
  复用 TCP+TLS，启用 HTTP/2 与 keepalive），翻译与 `/models` 探测共用同一个池；
  启动时对已启用服务的网关后台预热一次握手，首次翻译免 DNS+TLS 等待。
- **划词零等待**：待译文本先暂存进 `PopupPending`（复用窗口立即推事件，新建窗口
  挂载后经 `take_popup_pending` 自取、取走即清），删除后端 150ms 与前端 120ms 的
  挂载等待——弹窗本就预创建，划词到出字不再有固定延迟，也不靠定时器赌挂载。
- **快模型预设**：提供商预设的 `models[0]` 即选中后的默认模型，翻译场景把快档排到
  首位（智谱 glm-5.3-flash、百炼 qwen-turbo），火山方舟提示选 lite 档接入点；
  质量档保留在候选里，用户随时可换。

验收：`cargo test` 全绿（含客户端全局缓存、暂存取一次即清两条测试）；划词到出字
无固定延迟；选中提供商默认填快档模型。未排期的后续候选：请求 max_tokens 封顶、
services.json 与凭据读取缓存、长文本按句分段并发。

### M7+ 翻译提速（第二批）—— 已完成

- **max_tokens 封顶**：请求体带与输入相称的上限（字符数 ×2 + 128，下限 1024——
  思考型模型的思考 token 计入 max_tokens，下限太小会被思考烧完预算、正文为空），
  模型跑偏复读时收敛长尾，正常译文不受影响。
- **空结果兜底**：模型返回空正文时明确报错并记为失败，流内夹带的 error 对象
  （余额不足、内容审查等）转发为可读错误——都不再静默返回空串。
- **思考模式开关**：服务级三态（关闭 / 跟随模型默认 / 开启，默认关闭）。
  「思考开关」是各家私有扩展，按 Base URL 主机名映射请求体附加字段——
  智谱与方舟走 `thinking.type`，百炼与硅基流动走 `enable_thinking`；
  识别不出的提供商不发参数（严格网关会拒绝未知字段；DeepSeek、Kimi 用模型名区分）。
  翻译场景默认关思考：首字快一个量级且省思考 token。
- **服务配置缓存**：services.json 按「存在性 + mtime」进程内缓存，保存写穿更新；
  外部手改文件也会因 mtime 变化自然失效。`list_services` 只在首次启动落盘默认值，
  不再每次调用都写盘。
- **凭据缓存**：API Key 读取进程内缓存（凭据管理器是同步 COM 调用），
  写入/删除时同步更新缓存；绕过应用手改凭据管理器需重启应用才恢复一致。
- **长文本分段并发**：仅非流式服务，文本超过 500 字按段（目标 800 字，换行优先、
  句读次之、硬切兜底）并发请求、按序拼回；流式服务不分段——增量上屏的顺序体验
  优先，各段增量交错会打乱文本。任一段失败整体失败并带段序号，历史只记一条。

验收：`cargo test` 全绿（缓存命中判定、外部改动失效、分段拼回等于原文、
每块不超限等测试）；流式服务行为不变；词典模式永不分段。

## 3. 计划约定

通用开发规范（提交粒度、密钥、CSS、验证要求等）见 `AGENTS.md`，此处不重复。计划特有：

1. 依赖只加当前阶段需要的，不预装。
2. 每个里程碑结束跑一次完整验收，再开下一个。

## 4. 环境现状

- 终端为 Windows PowerShell 5.1（System32 自带）。
- Node v22.23.2、pnpm 12.5.1（`D:\dev\environment\nodejs`）；pnpm 用 `pnpm.cmd` 调用（见 `AGENTS.md`）。
- Rust 1.98.1，工具链在 `D:\dev\environment\rustup\toolchains\stable-x86_64-pc-windows-msvc`
  - `D:\dev\environment\cargo\bin\cargo.exe` 是 rustup 垫片，需先配好默认 toolchain，或直接调用工具链目录
- 已配置的翻译服务：Z.ai（`https://api.z.ai/api/paas/v4`，模型 `glm-5.3-flash`）
- OCR 语言包：系统仅安装 `zh-Hans-CN`
- `pnpm build` / `cargo test` / `pnpm ocr:assets` 已验证通过；OCR 模型已就位（`src-tauri/ocr/`）

## 5. 待确认

1. Windows 回落 OCR 仍优先请求 `en-US` 语言包（本机只有中文包），是否改为按内容自适应——
   本地 PaddleOCR 主路径中英混排后此问题只影响回落场景，保留观察。

