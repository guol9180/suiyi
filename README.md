# 随译 SuiYi

桌面 AI 翻译助手（Windows 首发）：划词翻译 / 截图 OCR / 输入框转译 / **多 AI 服务灵活配置**。

> 当前为 M0 里程碑：服务配置 + 流式翻译内核已完整可用。M1+（划词、截图 OCR、输入框转译、插件系统等）见 [DEVELOP_PLAN.md](./DEVELOP_PLAN.md)。

## 功能（M0）

- **多 AI 服务配置**：OpenAI 兼容协议（DeepSeek / 智谱 / Kimi / Ollama / OneAPI 中转…）即配即用
  - 服务可启用/停用、排序，翻译时**多服务并发对比**
  - API Key 只存 **Windows 凭据管理器**，配置文件零明文
  - Prompt 模板可自定义（`{{from}} / {{to}} / {{text}}` 变量注入）、Temperature、流式开关
- **流式翻译**：SSE 增量渲染，每个服务独立结果卡，失败自动提示
- 全局设置：并发数 / 单服超时

## 技术栈

| 层 | 选型 |
|---|---|
| 框架 | Tauri 2（Rust 后端 + WebView 前端） |
| 前端 | Vite + React + TypeScript |
| 网络 | reqwest（SSE 流式） |
| 密钥存储 | keyring（系统凭据管理器） |
| 配置 | `%APPDATA%/com.suiyi.dev/services.json` |

## 本地开发

环境要求：Node ≥ 18、pnpm、Rust stable (MSVC)。

```bash
pnpm install
pnpm tauri dev    # 开发调试（自动弹窗）
pnpm tauri build  # 打包安装程序
```

首次启动后到「设置 → 服务配置」添加服务：协议选 OpenAI 兼容，填 Base URL / API Key / 模型，启用即可翻译。

## 运行测试

```bash
cd src-tauri
cargo test                          # 单元测试（含 SSE mock 端到端、凭据管理器读写）
cargo test real_service_smoke -- --ignored --nocapture   # 真实服务冒烟（需已配置 Key）
```

## 文档

- [开发计划与里程碑](./DEVELOP_PLAN.md)
- [UI 设计稿](./design/ui-mockup.html)

## License

暂未设置开源协议（私有阶段）。公开发布前会补充。
