# 随译 SuiYi

Windows 桌面翻译工具。三个入口：划词、截图、输入框。翻译请求发给你自己的服务。

## 下载

下载页 <https://suiyi.imhgl.com/> · 全部版本 <https://github.com/guol9180/suiyi/releases>

| 文件 | 说明 |
|---|---|
| `SuiYi-Setup-x64.exe` | NSIS 安装程序，推荐 |
| `SuiYi-Setup-x64.msi` | MSI 安装包，适合批量部署 |
| `SuiYi-Portable-x64.zip` | 免安装版，解压后运行 `suiyi.exe` |

安装包没有代码签名，首次运行 Windows 会提示「已保护你的电脑」，点「更多信息」→「仍要运行」。

## 功能

| 热键 | 作用 |
|---|---|
| `Alt` + `D` | 划词翻译：取选中文字，在光标附近弹出译文窗口 |
| `Alt` + `S` | 截图识别：冻结鼠标所在的显示器，框选后离线 OCR |
| `Alt` + `T` | 输入框转译：读取当前输入框内容，翻译后原位替换 |
| `Esc` | 关闭当前弹窗 |

**多服务并发对比**：可以同时启用多个服务，一次翻译并发请求，每个服务各占一张结果卡。
服务可排序、可单独启停；失败的服务单独提示，不影响其他服务出结果。

**服务配置**：任何 OpenAI 兼容的端点都能接（DeepSeek、智谱、Kimi、Ollama、OneAPI 中转等）。
Prompt 模板支持 `{{from}}` `{{to}}` `{{text}}` 变量，可调 Temperature、可关流式。
标签页里的「测试连接」会请求一次 `/models`，告诉你 Key 是否有效、网关是否可达、模型在不在权限范围内。

**词典结构化**：服务的「结果类型」选词典结构时，解析出词条、音标与义项；
模型没按要求返回 JSON 时退回纯文本展示，不会报错。

**朗读**：用 Windows 自带语音合成朗读译文，离线、不消耗翻译额度。

**历史记录**：每次翻译按来源（划词 / 截图 / 手输 / 输入框）入库，可搜索、按来源筛选、删除与清空。
失败的记录也会留下并带失败原因。保留最近 2000 条。

**插件**：把一个目录放进插件文件夹就能加插件，分翻译、OCR、语音、动作四类，跑在 QuickJS 沙箱里。
翻译类插件通过校验并启用后会自动出现在服务列表里参与并发对比。

## 快速开始

1. 安装后启动。Windows 11 自带 WebView2；Windows 10 若提示缺失，装一次 WebView2 运行时。
2. 打开「设置 → 服务配置」，点「添加服务」，协议选 OpenAI 兼容，填 Base URL、API Key 和模型名。
3. 点「测试连接」确认配置无误。
4. 在任意应用里选中文字按 `Alt+D` 试用。

翻译消耗的是你自己的 API 额度；同时启用多个服务时，每次翻译会同时消耗这几个服务的额度。

API Key 只写进 Windows 凭据管理器，配置文件里零明文。

## 系统要求

Windows 10 1809 及以上，x64。需要 WebView2 运行时（Windows 11 自带）。

## 从源码构建

需要 Node 18 以上、pnpm、Rust stable（MSVC 工具链）。

```bash
pnpm install
pnpm tauri dev      # 开发调试
pnpm tauri build    # 打包安装程序
pnpm build          # 只构建前端（tsc + vite）
```

后端检查与测试在 `src-tauri` 下执行：

```bash
cargo check
cargo test
cargo test real_service_smoke -- --ignored --nocapture   # 真实服务冒烟，需已配置 Key
```

少数用例依赖宿主能力，能力缺失时会打印原因并跳过而不是失败：凭据管理器需要真实登录会话，
OCR 需要系统装有语言包，语音合成需要可用音色。跑 `cargo test -- --nocapture` 可以看到跳过原因。

打 `v*` 标签会触发 `.github/workflows/release.yml`，在 GitHub 的 Windows 机器上跑测试、构建并发布。

## 项目结构

```
suiyi/
├─ docs/                  下载页（GitHub Pages 从这里发布）
├─ design/                界面设计基线，单文件 HTML
├─ src/                   前端：React + TypeScript
│  ├─ pages/              翻译页、设置页、划词弹窗、截图覆盖层
│  ├─ components/         图标、词典卡等共用组件
│  └─ styles/             tokens.css（设计变量）与 base.css（基础组件）
├─ src-tauri/src/         后端：Rust
│  ├─ config.rs           services.json 读写
│  ├─ keyring.rs          API Key 存取
│  ├─ translator.rs       OpenAI 兼容适配器与 SSE 流式
│  ├─ selection.rs        Alt+D 取词与光标处弹窗
│  ├─ screenshot.rs       Alt+S 冻结帧、框选与 OCR
│  ├─ writeback.rs        Alt+T 输入框写回、划词替换
│  ├─ speech.rs           本地语音朗读
│  ├─ history.rs          翻译历史（SQLite）
│  ├─ anki.rs             生词本（AnkiConnect）
│  ├─ plugin.rs           插件清单与发现
│  ├─ plugin_js.rs        插件沙箱（QuickJS）
│  └─ commands.rs         命令层，前后端唯一通道
├─ .github/workflows/     发布与下载页的 CI
└─ DEVELOP_PLAN.md        开发计划与验收标准
```

## 技术栈

| 层 | 选型 |
|---|---|
| 应用框架 | Tauri 2 |
| 后端 | Rust（stable，MSVC） |
| 前端 | Vite + React + TypeScript |
| 网络 | reqwest（SSE 流式） |
| 密钥存储 | keyring（系统凭据管理器） |
| 取词与按键 | arboard、SendInput |
| 截屏与 OCR | GDI BitBlt、Windows.Media.OCR |
| 语音 | Windows.Media.SpeechSynthesis |
| 历史 | rusqlite（bundled SQLite，WAL） |
| 插件沙箱 | rquickjs（QuickJS） |

配置、历史与插件都放在 `%APPDATA%/com.suiyi.dev/` 下：`services.json`、`history.db`、`plugins/`。

## 已知限制

- 只在 Windows 上构建与验证过；macOS 与 Linux 尚未实现，代码里只有「当前平台不支持」的提示。
- 截图识别的语言取决于系统 OCR 语言包，本机只装中文包时识别英文会有误差。
- 朗读用系统语音，音色一般，还没有语速与音色选择。
- 插件不能联网，只能读写剪贴板（需要申请权限）。
- 输入框转译写回后要撤销，得用目标程序自己的 `Ctrl+Z`。
- 热键固定为 Alt+D / Alt+S / Alt+T，暂不支持自定义。
- 截图识别只覆盖鼠标所在的那块屏幕，不支持跨屏框选。

## 相关文档

- [DEVELOP_PLAN.md](./DEVELOP_PLAN.md)：里程碑、验收标准与开发日志
- [design/ui-mockup.html](./design/ui-mockup.html)：界面设计基线（单文件，浏览器直接打开）
- [docs/index.html](./docs/index.html)：下载页源码

## 许可

尚未设置开源协议。在补充 LICENSE 之前，本仓库不授予任何使用许可。
