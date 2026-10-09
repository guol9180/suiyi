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
| `Alt` + `S` | 截图识别：所有显示器一起冻结，可跨屏框选，识别在本机完成 |
| `Esc` | 关闭当前弹窗 |

**多服务并发对比**：可以同时启用多个服务，一次翻译并发请求，每个服务各占一张结果卡。
服务可排序、可单独启停；失败的服务单独提示，不影响其他服务出结果。

**热键可改**：两个热键都能重新录制。被其他程序占用时设置页会指出来并可重试，
改过的键可以一键恢复默认。

注意 `Alt+D` 在 Chrome / Edge 里是「定位到地址栏」：浏览器里划词建议改成
`Ctrl+Alt+D`（设置 → 热键），否则选区会先被浏览器抢走。`Alt+S` 一般不受影响。

**托盘与关闭行为**：随译常驻右下角托盘。点窗口右上角 × 会问一次「直接关闭 / 收进托盘」，
勾上「记住我的选择」之后不再问。收进托盘时窗口藏起来、热键继续可用，单击托盘图标叫回窗口，
右键菜单里有「显示主窗口 / 检查更新 / 退出随译」。想改回来去
设置 → 通用 → 窗口与更新。

**更新**：设置 → 关于 里的「获取更新」会读 GitHub Releases 的最新版本，
有新版本时一键下载；下载完随译退出并打开安装向导，装完在向导最后一步勾选
「运行 SuiYi」即可。启动时自动检查可以在设置 → 通用 里关掉。

**服务配置**：点「添加服务」先挑提供商，内置 DeepSeek、智谱 GLM、Kimi、阿里百炼、火山方舟、
硅基流动、本地 Ollama 与自定义中转；选中即填好 Base URL、协议与推荐模型，一个按钮直达那家的密钥控制台，
回来「从剪贴板粘贴」再「保存并测试」即可。本地服务勾上「不需要密钥」就能不填 Key。
任何 OpenAI 兼容的端点也都能手填接入（OneAPI 中转、私有网关等）。
Prompt 模板支持 `{{from}}` `{{to}}` `{{text}}` 变量，可调 Temperature、可关流式。
标签页里的「测试连接」会请求一次 `/models`，告诉你 Key 是否有效、网关是否可达、模型在不在权限范围内。
401 / 402 / 403 / 404 / 429 会翻成一句人话并给出修复按钮，服务端原文折叠在「详情」里。

**词典结构化**：服务的「结果类型」选词典结构时，解析出词条、音标与义项；
模型没按要求返回 JSON 时退回纯文本展示，不会报错。

截图识别的 OCR 会先把选区向外多读一圈（32 像素）再按行过滤回来：Windows 自带的 OCR
认不了「紧框一行」的细长条图，多读一圈上下文才稳，带进来的邻行不会混进译文。

**朗读**：用 Windows 自带语音合成朗读译文，离线、不消耗翻译额度。

**历史记录**：每次翻译按来源（划词 / 截图 / 手输 / 输入框）入库，可搜索、按来源筛选、删除与清空。
失败的记录也会留下并带失败原因。保留最近 2000 条。
（输入框转译已在 v0.8.1 移除，历史里旧的「输入框」记录仍会展示。）

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

## 常见问题

**升级后桌面/任务栏还是旧图标？** 安装包里的图标是新的（可以从 exe 属性里看），
Windows 只是把图标缓存了起来。安装程序装完会自动跑一次 `ie4uinit.exe -show` 并广播
`SHChangeNotify`，新装或升级后一般直接就是新图标；如果还显示旧的，
说明是这次安装之前留下的缓存，手动做一次即可：先跑 `ie4uinit.exe -show`，
仍不生效就重启资源管理器（任务管理器 → Windows 资源管理器 → 重新启动）。
任务栏上「已钉住」的那一项由注册表缓存，程序改不了，取消固定再钉一次就好。

## 从源码构建

需要 Node 18 以上、pnpm、Rust stable（MSVC 工具链）。

```bash
pnpm install
pnpm tauri dev      # 开发调试
pnpm tauri build    # 打包安装程序
pnpm build          # 只构建前端（tsc + vite）
```

应用图标、favicon 与安装向导用图（NSIS 头部/侧栏、MSI 横幅/对话框）都由同一份真源生成，
改了 `design/brand/logo.svg` 之后跑一次：

```bash
node design/brand/build.mjs   # 需要本机装有 Edge（用它把 SVG 渲成位图）
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
│  ├─ tray.rs             系统托盘图标与菜单（右下角常驻入口）
│  ├─ notice.rs           光标旁的轻提示窗口（不抢焦点）
│  ├─ update.rs           检查更新、下载安装包、拉起安装向导
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
- 热键默认是 Alt+D / Alt+S，可以在设置里改；录制时至少要带一个修饰键。
- 截图识别会冻结所有显示器，可在任意一块屏起框、跨屏拖选；混合 DPI（两块屏缩放不同）时，
  覆盖层里的提示文字与尺寸徽标按窗口 DPI 缩放，在另一块屏上会略大或略小，裁剪本身不受影响。

## 相关文档

- [DEVELOP_PLAN.md](./DEVELOP_PLAN.md)：里程碑、验收标准与开发日志
- [design/ui-mockup.html](./design/ui-mockup.html)：界面设计基线（单文件，浏览器直接打开）
- [docs/index.html](./docs/index.html)：下载页源码

## 许可

尚未设置开源协议。在补充 LICENSE 之前，本仓库不授予任何使用许可。
