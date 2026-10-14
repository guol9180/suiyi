# AGENTS.md · 随译 SuiYi 开发规范

> 本文件是开发规范，不是问题台账。动手前先读；里程碑与验收标准见 `DEVELOP_PLAN.md`。

## 项目

Windows 桌面 AI 翻译工具：划词翻译（Alt+D）+ 截图识别（Alt+S），多 AI 服务并发对比。
Tauri 2 + Rust 后端 + React 19 + TypeScript + Vite 8。版本号以 `package.json` 为准。

## 常用命令

```bash
pnpm install            # 依赖（registry 走 npmmirror）
pnpm dev                # 前端开发（端口 1420）
pnpm tauri dev          # 整体开发调试
pnpm build              # tsc + vite 前端构建
pnpm ocr:assets         # 拉取 OCR 模型到 src-tauri/ocr/（模型不入库，打 Tauri 包前必须执行）
pnpm tauri build        # 打包安装程序
cd src-tauri && cargo test   # 后端测试
```

发版：打 `v*` 标签触发 `.github/workflows/release.yml`（测试 → 构建 → 签名 → 发布 → 刷新下载页）。

## 环境注意

- pnpm 一律用 `pnpm.cmd` 调用；`pnpm.ps1` 会被本机执行策略拦截。
- 代理沙箱内只用文件工具改源码；写 `node_modules`、`target`、`.git` 的命令（pnpm / cargo / git）在无沙箱限制的终端执行。

## 硬性规范

1. API Key 只存 Windows 凭据管理器（keyring，命名空间 suiyi）；不得进仓库、配置文件、日志。
2. 版本号唯一来源是 `package.json`，`tauri.conf.json` 同步修改；界面用 `__APP_VERSION__` 显示。
3. CSS 注释文本不得包含 `*/`，分隔用「与」「、」——否则注释提前闭合，其后规则全部失效。
4. CSS 代码区不得出现全宽标点（；：（））——会破坏声明终止，吞掉后续规则。
5. 版式或结构改动后：先做 div/section 开闭配平计数，再整页渲染截图逐节目检；只读代码不算验证。
6. 批量替换之后，必须对被替换内容做全局 grep 复查。
7. 一次提交只做一件事，提交信息用中文写清范围。

## 界面与设计规范

- 视觉真源是 `src/styles/tokens.css`；品牌蓝 #3964FE 只作按钮底，作文字用 #2B52E0（4.38:1 不达标）。
- 热键教育只出现在两处：首次使用引导、热键被占用横幅。不进常驻工具栏、侧栏、右栏。
- 输入框转译与截图原图覆盖已移除，界面与设计稿不得再出现；全局热键只有 Alt+D / Alt+S。
- 设计稿两份，角色勿混淆：`design/ui-mockup.html` 与实现逐字对齐（改界面先对照）；`design/ui-redesign.html` 是未实现的重设计提案，不得当现状引用。
- 图标为 16px 线性 SVG（见 `components/IconSprite`），不使用图标字体。

## 行为约定（写文案与测试按此）

- 「测试连接」= 用当前协议与模型真实跑一次翻译（消耗几十个 token），不是 /models 探测。
- 历史记录在后端落库；前端关窗不影响完成与入库，前端不重复记。
- 窗口清单：主窗 1024×700（最小 900×620，系统标题栏）、划词弹窗 430×540（无边框+Acrylic）、OCR 窗 460×600（无边框+Acrylic）、轻提示 420×66（不聚焦、鼠标穿透）。
- 设置 8 个页面都在主窗口「设置」页签内，不是独立窗口。
- 数据目录 `%APPDATA%/com.suiyi.dev/`：`services.json`、`history.db`、`plugins/`。

## 依赖锁定

- `ort` 锁定 `=2.0.0-rc.10`：rc.13 起 Tensor/Session API 不兼容；升级前必须核对 API 变更并同步修改调用代码。

## 已知边界（不是 bug）

- 仅支持 Windows 10 1809+ x64 + WebView2；macOS / Linux 未实现。
- OCR 为 PP-OCRv4 中英文模型：手写体、竖排、极小字号会识别错误；模型缺失自动回落系统 OCR。
- 插件无网络能力，仅 clipboard 权限。
- 安装包未签名，首次运行有 SmartScreen 提示（SignPath 接入见 `docs/signing.md`）。
