# 随译 SuiYi · 开发计划

> 桌面 AI 翻译助手（Windows 首发）：划词翻译 / 截图 OCR / 输入框转译 / 多 AI 服务灵活配置。
> 设计基线：`design/ui-mockup.html`（v0.2）。工作名「随译 SuiYi」为占位，可随时改。

## 1. 技术栈（定稿）

| 层 | 选型 | 说明 |
|---|---|---|
| 应用框架 | Tauri 2 | 轻量、跨平台；WebView 渲染前端 |
| 后端 | Rust（stable, msvc） | 业务内核：服务编排 / 取词 / OCR / 写回 |
| 前端 | Vite + React + TypeScript | 设置页与弹窗 UI；设计 token 从设计稿迁移 |
| 状态管理 | zustand | 轻量 |
| 配置存储 | services.json + keyring crate | API Key 只进 Windows 凭据管理器，配置文件零明文 |
| 网络 | reqwest（stream） | OpenAI 兼容 `/chat/completions` + SSE 流式 |
| 数据库 | rusqlite（M4 起） | 翻译历史 |
| 后续按阶段引入 | tauri-plugin-global-shortcut、enigo、windows crate | M1 热键/取词、M2 OCR、M3 写回 |

## 2. 目标目录结构（M0 完成后）

```
suiyi/
├─ design/                  UI 设计稿（已有 ui-mockup.html / .png）
├─ src/                     前端（React + TS）
│  ├─ pages/translate/      主翻译窗口（M0 先做最小版）
│  ├─ pages/settings/       设置窗口（服务配置等）
│  ├─ components/           Toggle / Chip / Field 等基础组件
│  └─ styles/tokens.css     设计变量（色板/圆角/阴影，对应设计稿）
├─ src-tauri/
│  ├─ src/config.rs         services.json 读写 + 服务 Schema
│  ├─ src/keyring.rs        API Key 存取（系统凭据管理器）
│  ├─ src/translator.rs     OpenAI 兼容适配器 + SSE 流式
│  └─ src/commands.rs       Tauri 命令层（前后端唯一通道）
└─ DEVELOP_PLAN.md          本文件
```

## 3. 里程碑

### M0 内核闭环（当前阶段）—— 目标：窗口里配好 AI 服务，流式翻出译文
| 步骤 | 内容 | 验收 |
|---|---|---|
| S0.1 | 环境准备：安装 Rust（rustup + MSVC Build Tools）；确认 pnpm.cmd 可用 | `cargo --version` 出版本 |
| S0.2 | 脚手架：create-tauri-app（react-ts 模板）落到本目录 | `pnpm tauri dev` 弹出空窗口 |
| S0.3 | 设计 token：tokens.css + 基础组件样式（按设计稿④的表单风格） | 设置页静态样式就位 |
| S0.4 | 配置层：config.rs（默认 services.json + 增删改查）+ keyring.rs | Rust 单元测试通过 |
| S0.5 | 命令层：list/save/delete_service、get/set_api_key、test_connection | 前端能调通全部命令 |
| S0.6 | 设置页：服务列表 + 编辑表单（对应设计稿④） | 配置真实保存、重启不丢 |
| S0.7 | 翻译内核：translator.rs 流式请求 → `emit("translate-delta")` | 假服务返回流式文本 |
| S0.8 | 最小翻译界面：输入 → 选服务 → 流式译文显示 | **真 Key 流式翻译成功** |

### M1 划词翻译（Windows）
S1.1 全局热键 Alt+D → S1.2 取词（模拟 Ctrl+C 读剪贴板；UIA 备选）→ S1.3 光标处无边框置顶弹窗（对应设计稿①）→ S1.4 多服务并发对比 + 回退链。
验收：浏览器 / PDF 里划词出译文，Esc 关闭，服务失败自动切换。

### M2 截图 OCR / 原图翻译
S2.1 全屏遮罩框选 → S2.2 Windows.Media.OCR 离线识别 → S2.3 结果面板 + 翻译（对应设计稿③）→ S2.4 原图翻译覆盖标签。
验收：对屏幕任意区域框选，出识别文本与译文，译文可覆盖回原位。

### M3 写回与生词本
S3.1 输入框转译 Alt+T（读焦点控件 → 翻译 → 快照校验后写回，失败回退复制+通知）→ S3.2 划词「翻译并替换」→ S3.3 Anki Connect 生词本。
验收：聊天输入框中文一键变英文；错点窗口不会写错地方。

### M4 词典结构化 + TTS + 历史记录
结构化词典结果（resultType: dictionary）、朗读、rusqlite 历史与搜索。

### M5 插件系统
manifest + JS 沙箱（QuickJS），翻译/OCR/语音/动作四类扩展点，参考 Manggo 插件格式设计。

### M6 跨平台
macOS（辅助功能/录屏权限引导）、Linux（X11 优先，Wayland 用外部调用方案）。

## 4. 开发约定
1. **一步一步来**：每个 S 步骤一个 commit（`M0-S0.4: 配置层实现`），完成并汇报后再进下一步；
2. 依赖只加当前阶段需要的，不预装；
3. `.gitignore` 第一天建好；API Key / 密钥永远不进仓库与配置文件；
4. 每个里程碑结束跑一次完整验收，再开下一个。

## 5. 环境现状（2025-09-29 检查）
- Node v22.23.2 ✓　Git 2.55 ✓
- pnpm / npm 已装于 `D:\dev\environment\nodejs`，但 `.ps1` 垫片被执行策略拦截 → 调用 `.cmd` 版本即可（或以进程级 Bypass 运行）
- **Rust 未安装** → S0.1 处理（rustup + MSVC Build Tools，首次安装约 2~4 GB 下载）

## 6. 待确认
1. 前端框架 React + TS 是否 OK？（想用 Vue 请在 S0.2 之前提出）
2. M0 验收需要一个 OpenAI 兼容服务：DeepSeek / 智谱 / Kimi / OneAPI 中转的 API Key，或本机 Ollama；
3. S0.1 安装 Rust 需要联网下载并可能弹出 VS Build Tools 安装器，届时会再次征求批准。
