# 随译 SuiYi

Windows 桌面 AI 翻译工具：划词翻译与截图识别两个全局热键入口，多个 AI 服务
同时并发对比，译文来自你自己配置的服务。Tauri 2 + Rust + React 19。

心智模型三句话：

- 两条入口覆盖两个场景——屏幕上**选中的文字**按 `Alt+D`，**画面里的文字**按 `Alt+S` 框选；
- 翻译请求发给**你自己配置的服务**（任何 OpenAI 兼容端点），多个服务并发出稿、逐字流式、同屏对比；
- 敏感信息自持——API Key 只进系统凭据管理器，OCR 在本机离线完成，截图不上传。

> 文档分工：本文件帮新人建立整体认识；开发计划与验收标准看
> [DEVELOP_PLAN.md](./DEVELOP_PLAN.md)；动手改代码前先读 [AGENTS.md](./AGENTS.md)。

## 上手

**安装**：从 [Releases](https://github.com/guol9180/suiyi/releases) 取
NSIS / MSI 安装包或免安装 zip（下载页 <https://suiyi.imhgl.com/>）。
安装包未签名，首次运行在 SmartScreen 提示里点「更多信息」→「仍要运行」。
Windows 10 需装 WebView2 运行时，Windows 11 自带。

**配置服务**：设置 → 服务配置 → 添加服务。选内置提供商（DeepSeek、智谱 GLM、
Kimi、阿里百炼、火山方舟、硅基流动、Ollama）会自动填好地址与推荐模型，
任何 OpenAI 兼容端点也能手填。API Key 只写进凭据管理器，配置文件零明文。
「测试连接」会用你配的模型**真实跑一次翻译**——模型列表对所有人可见，
`/models` 探不出权限，「测试通过但真翻译 403」就是这么防住的。

**试用**：任意应用选中文字按 `Alt+D`；屏幕上的文字按 `Alt+S` 框选。
同时启用多个服务时，每次翻译并发消耗这几个服务的额度。

**从源码跑**：需要 Node 18+、pnpm、Rust stable（MSVC）。

```bash
pnpm install
pnpm tauri dev      # 开发调试
pnpm tauri build    # 打包安装程序
```

OCR 模型（约 16MB）不进仓库，`pnpm dev` / `pnpm build` 会自动按 SHA256 拉取校验；
直接 `cargo build` 前手动跑一次 `pnpm ocr:assets`。后端测试在 `src-tauri` 下
`cargo test`（依赖宿主能力的用例——凭据管理器、系统语言包、语音音色——缺失时
打印原因并跳过，不会失败）。

## 一条译文的旅程

**划词翻译（Alt+D）**

1. 任意应用里选中文字，按下 `Alt+D`。
2. Rust 端等修饰键松开（逐个体检左右键码——替用户补抬键会打乱目标程序的
   按键状态），经 SendInput 发出 Ctrl+C，80ms 粒度轮询剪贴板读回文本，
   随后恢复用户原剪贴板。
3. 文本先暂存再投递给光标旁的划词弹窗：弹窗启动时就预创建，复用窗口立即
   收到事件，新建窗口挂载后自取暂存——热键到出字之间没有固定延迟。
4. 弹窗把文本同时发给每个已启用服务，谁先出字谁先上屏（SSE 增量），
   单个服务失败单独标错，不影响其他。
5. 「更多」菜单里可以把第一条译文替换回原窗口（取词时记下了目标窗口句柄，
   回焦校验通过才粘贴）、朗读、进生词本、跑动作插件。

**截图识别（Alt+S）**

1. 按下 `Alt+S`，全屏蒙版立即铺开：一张无损定格帧 1:1 物理像素铺满屏幕——
   无压暗、无压缩糊感，静态桌面下与实时屏幕视觉一致（GDI BitBlt 后台抓帧，
   BMP 无损落盘经资产协议加载）。覆盖层随应用启动预创建，画面加载完成才亮窗，
   按下即出、没有窗口闪过的一瞬。
2. 拖拽框选，或悬停后单击直接选取整个窗口：光标下的窗口边框与标题实时高亮
   （按 Z 序命中检测，边界取 DWM 扩展框架，与肉眼看到的窗口边缘一致）。
   坐标按定格帧自然尺寸换算，不碰各屏 DPI 设置——混合缩放的多屏天然不错位。
3. 松开后**紧贴选区裁剪**再送识别：识别范围与框选范围构造上相等，
   框住半行就只返回半行；紧框一个字都认不出（贴边小字、条状单行）才外扩
   32px 重试，行级过滤加交集精修兜底。
4. 识别由随包分发的 PaddleOCR（PP-OCRv4 + ONNX Runtime）离线完成；
   模型缺失或加载失败自动回落 Windows 自带 OCR，识别不会整体不可用。
5. 结果面板先出原文——复制，或选好目标语言点「翻译」。翻译不自动开始，
   额度的决定权在你。

主窗口手动输入走同一条翻译管线（同一内核、同一结果卡组件），不重复展开。

## 代码地图

```
suiyi/
├─ src/                   前端：React + TypeScript（页面、组件、样式）
├─ src-tauri/             后端：Rust（Tauri 命令层与业务内核）
├─ design/                界面设计稿：ui-redesign.html（v1.0，已实现）与 ui-mockup.html（基线）
├─ docs/                  下载页（GitHub Pages 从这里发布）
├─ scripts/               构建辅助脚本（OCR 模型拉取）
└─ .github/workflows/     发布与下载页的 CI
```

第一次读代码，建议按这个顺序进五个文件：

| 文件 | 它是什么 |
|---|---|
| `src-tauri/src/lib.rs` | 装配层：状态注册、热键、预创建窗口、启动预热，从这里看全局 |
| `src-tauri/src/translator.rs` | 翻译内核：OpenAI 兼容适配、SSE 流式、连接池、长文本分段 |
| `src-tauri/src/screenshot.rs` | 截图管线：冻结帧、裁剪、OCR 引擎链、结果投递 |
| `src-tauri/src/selection.rs` | 划词取词与替换：剪贴板法、修饰键等待、回焦校验 |
| `src/pages/Popup.tsx` · `src/pages/OcrResult.tsx` | 两个结果窗口的前端：并发结果卡与流式上屏 |

其余核心：`ocr.rs`（引擎加载与回落）、`hotkeys.rs`（热键注册）、
`plugin_js.rs`（插件沙箱）、`commands.rs`（前端 ↔ 后端命令层）。

技术栈：reqwest（全局连接池 + SSE）、PaddleOCR + ort、arboard + SendInput、
rusqlite（bundled SQLite，WAL）、rquickjs（QuickJS）、Windows.Media.SpeechSynthesis。
配置、历史与插件都放在 `%APPDATA%/com.suiyi.dev/` 下。

## 关键取舍

| 决策 | 为什么 |
|---|---|
| 紧贴选区裁剪后再识别 | 识别范围与框选范围构造上相等；外扩与行过滤只做空结果兜底 |
| PaddleOCR 随包分发，Windows OCR 只做回落 | 离线可用是底线；系统 OCR 识别条状单行会返回 0 行，当不了主力 |
| 识别与翻译解耦 | 识别是本地零成本动作，翻译花钱——额度的决定权留给用户 |
| 全局连接池 + 启动预热 | 每次翻译重付 DNS + TLS 是纯浪费，国内网关一次几十到上百毫秒 |
| 流式服务不做长文本分段并发 | 增量上屏的顺序体验优先于总耗时；非流式才按段并发、按序拼回 |
| 请求按输入封顶 max_tokens | 模型跑偏复读时收敛长尾；正常译文与输入同量级，不受影响 |
| 思考模式默认关闭（服务级开关） | 思考 token 让首字慢一个量级还照常计费；翻译场景关思考几乎稳赚，难文本可对单个服务重开 |
| API Key 只进系统凭据管理器 | 配置文件零明文，泄露面收敛到 OS 级保护 |
| 插件跑在 QuickJS 且无网络 | 第三方代码能力最小化；联网扩展点等异步 HTTP 桥，不做半成品 |

## 边界与限制

- 只在 Windows 上构建与验证；macOS / Linux 尚未实现。
- OCR 是 PP-OCRv4 mobile 中英文模型：手写体、竖排、极小字号会错；
  回落 Windows 自带 OCR 时质量明显下降，且固定请求 en-US 语言包。
- `Alt+D` 在 Chrome / Edge 里是「定位到地址栏」，浏览器里划词需改成
  `Ctrl+Alt+D`（设置 → 热键）。
- 截图冻结所有显示器；混合 DPI 下覆盖层提示文字在副屏略大或略小，裁剪不受影响。

## 参与开发

- 规范先行：[AGENTS.md](./AGENTS.md)（提交粒度、密钥、CSS、验证要求）。
- 计划与逐文件职责：[DEVELOP_PLAN.md](./DEVELOP_PLAN.md)。
- 界面以两份设计稿为准：[ui-redesign.html](./design/ui-redesign.html)（v1.0，已实现）、
  [ui-mockup.html](./design/ui-mockup.html)（基线）。
- CI：打 `v*` 标签自动跑测试、构建并发布 Release，下载页同步部署。

## 许可

MIT，全文见 [LICENSE](./LICENSE)。安装包未签名，正在申请
[SignPath Foundation](https://signpath.org/) 开源签名
（流程见 [docs/signing.md](./docs/signing.md)），接入后产物名与下载链接不变。
