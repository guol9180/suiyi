# 随译 SuiYi

Windows 桌面 AI 翻译助手。划词翻译、截图识别、输入框转译，翻译引擎可自由配置。

## 里程碑

| 里程碑 | 内容 | 状态 |
|---|---|---|
| M0 | 多 AI 服务配置、凭据管理器、流式翻译内核 | 已完成 |
| M1 | Alt+D 划词取词，光标处弹出翻译窗 | 已完成 |
| M2 | Alt+S 截图识别：冻结帧、框选、离线 OCR、翻译 | 开发中 |
| M3 | Alt+T 输入框转译、划词替换、Anki 生词本 | 部分完成（前两项已实现） |
| M4 | 词典结构化结果、朗读、历史记录 | 计划中 |
| M5 / M6 | 插件系统 / macOS 与 Linux 移植 | 计划中 |

详细拆解与验收标准见 [DEVELOP_PLAN.md](./DEVELOP_PLAN.md)，界面基线见 [design/ui-mockup.html](./design/ui-mockup.html)。

## 已实现的能力

**多 AI 服务配置**

- OpenAI 兼容协议即配即用（DeepSeek、智谱、Kimi、Ollama、OneAPI 中转等）
- 服务可启用停用、调整顺序；翻译时多服务并发对比
- API Key 只写入 Windows 凭据管理器，配置文件零明文
- Prompt 模板支持 `{{from}}` `{{to}}` `{{text}}` 变量注入，可调 Temperature 与流式开关

**流式翻译**

- 基于 SSE 增量渲染，每个服务一张独立结果卡，失败单独提示并可直接重试
- 全局设置：并发数、单服务超时

**划词翻译**

- 全局热键 Alt+D，剪贴板往返取词，无边框弹窗定位在光标处并做屏幕越界回退
- 失焦自动隐藏，可固定窗口；Esc 关闭
- 弹窗内含复制、替换原文、搜索、浏览器打开、重译等动作

## 技术栈

| 层 | 选型 |
|---|---|
| 应用框架 | Tauri 2 |
| 后端 | Rust（stable，MSVC） |
| 前端 | Vite + React + TypeScript |
| 网络 | reqwest（SSE 流式） |
| 密钥存储 | keyring（系统凭据管理器） |
| 取词与按键模拟 | arboard、SendInput |
| 截屏与 OCR | GDI BitBlt、Windows.Media.OCR |
| 配置 | `%APPDATA%/com.suiyi.dev/services.json` |

## 开发环境

本机 Rust 工具链装在 `D:\dev\environment`。注意 `D:\dev\environment\cargo\bin\cargo.exe` 是 rustup 垫片，
在未配置默认 toolchain 的会话里直接调用会报 `rustup could not choose a version of cargo`，
改用带版本的工具链目录：

```powershell
$tc = "D:\dev\environment\rustup\toolchains\stable-x86_64-pc-windows-msvc\bin"
$env:Path = "$tc;" + $env:Path
$env:RUSTUP_HOME = "D:\dev\environment\rustup"
$env:CARGO_HOME = "D:\dev\environment\cargo"
```

前端依赖用 pnpm（`D:\dev\environment\nodejs\pnpm.cmd`）。

## 常用命令

```powershell
pnpm install
pnpm tauri dev      # 开发调试，自动弹窗
pnpm tauri build    # 打包安装程序
pnpm build          # 只构建前端（tsc + vite）
```

后端检查与测试（在 `src-tauri` 下执行）：

```powershell
cargo check
cargo test
cargo test real_service_smoke -- --ignored --nocapture   # 真实服务冒烟，需已配置 Key
```

`keyring` 的单元测试需要真实的用户登录会话（会读写 Windows 凭据管理器）。
在没有登录会话的环境（部分沙箱、服务账户）里会报 `ERROR_NO_SUCH_LOGON_SESSION`，属环境限制。

## 目录结构

```
suiyi/
├─ design/                 UI 设计稿（单文件 HTML，浏览器直接打开）
├─ src/                    前端：React + TS
│  ├─ pages/               翻译页、设置页、划词弹窗、截图覆盖层
│  └─ styles/              设计 token 与基础组件样式
├─ src-tauri/              Rust 后端
│  └─ src/                 配置层、凭据层、翻译内核、取词、截图 OCR、命令层
└─ DEVELOP_PLAN.md         里程碑与验收标准
```

## 许可

尚未设置开源协议。在补充 LICENSE 之前，本仓库不授予任何使用许可。
