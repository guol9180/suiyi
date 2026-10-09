# 代码签名（SignPath Foundation）

随译的安装包目前**没有签名**：Windows 会显示「未知发布者」，SmartScreen 也可能拦一次。
个人开发者的正常说法是「买一张 OV/EV 证书」，但那条路要钱、要个人身份验证、还要 USB 令牌，
和云端 CI 冲突。开源项目有一条免费且适合 CI 的路子，本项目就走它。

## 为什么是 SignPath Foundation

[SignPath Foundation](https://signpath.org/) 给开源项目免费签发代码签名证书：

- **不需要你的个人身份证明**：他们校验「这个二进制确实从你的开源仓库构建出来」，并以自己的名义背书。
- **私钥在对方的 HSM 里**：不用你保管令牌，GitHub Actions 里通过 API 提交签名请求即可。
- **对 OSS 免费**：代价是走一遍审核。

前提条件（本项目都已满足）：

| 要求 | 本项目现状 |
|---|---|
| 仓库公开 | https://github.com/guol9180/suiyi 公开 |
| OSI 认可的开源许可 | `LICENSE`（MIT） |
| 有可下载的正式版本 | GitHub Releases + 下载页 https://suiyi.imhgl.com/ |
| 项目在持续维护 | 版本按 tag 持续发布，CI 自动出包 |
| 构建可复现 | GitHub Actions 从 tag 构建，产物名固定 |

> 备选是 Certum 的开源签名证书：同样免费，但要本人做身份验证，签名走 SimplySign 云签名，
> 接进 GitHub 托管 Runner 很麻烦。除非 SignPath 审核不过，否则不用考虑。

## 申请材料（可直接粘贴）

申请入口：<https://signpath.org/apply>。表单里通常要填下面这些，照着填即可。

**Project name**

```
SuiYi (随译) - Windows translation assistant
```

**Repository URL**

```
https://github.com/guol9180/suiyi
```

**License**

```
MIT
```

**Project description (English)**

```
SuiYi is a lightweight Windows desktop translation assistant written in Rust
(Tauri 2) with a React frontend. It offers two global-hotkey entry points:
text selection translation and screenshot OCR translation. Translation requests
go directly from the user's machine to services the user configures themselves
(DeepSeek, Zhipu, Kimi, Ollama, or any OpenAI-compatible endpoint); the project
operates no backend and collects no telemetry. Screenshot OCR runs fully offline
with local PaddleOCR (PP-OCRv4 ONNX) models.

Builds are produced exclusively by GitHub Actions on windows-latest from signed
release tags, and published as NSIS, MSI and portable ZIP artifacts on GitHub
Releases. The installers are currently unsigned, which triggers Windows
"unknown publisher" warnings for end users. We would like to join the SignPath
Foundation program to sign our release artifacts.
```

**Download page**

```
https://suiyi.imhgl.com/
```

**Build workflow (for their review)**

```
.github/workflows/release.yml  (GitHub Actions, windows-latest, triggered by v* tags)
```

## 审核通过后要做的三步

1. 登录 SignPath.io，在项目里建一个 **signing policy** 与一份 **artifact configuration**，
   把下面四个值抄下来：
   - `organization id`
   - `project slug`
   - `signing policy slug`
   - `artifact configuration slug`
2. 在 GitHub 仓库 `Settings → Secrets and variables → Actions` 里加一个 Secret：

   | 名称 | 值 |
   |---|---|
   | `SIGNPATH_API_TOKEN` | SignPath 里生成的 API token |

   同一页的 **Variables** 标签下加一个变量（不是 Secret）：

   | 名称 | 值 |
   |---|---|
   | `SIGNPATH_READY` | `true` |

3. 把四个 slug 填进 `.github/workflows/release.yml` 的签名步骤（那里标了 `<-- 填这里`）。
   填完打个 tag，CI 会：构建 → 提交给 SignPath 签 → 取回已签名产物 → 发布。

签名覆盖三个文件：`suiyi.exe`（进免安装包的那个）、NSIS 安装包、MSI。
产物名与下载链接保持不变，下载页无需改动。

## 验证

签名完成后，在任意一台 Windows 上执行：

```powershell
Get-AuthenticodeSignature .\SuiYi-Setup-x64.exe | Format-List Status, SignerCertificate
```

`Status` 应为 `Valid`，签名者显示 SignPath Foundation 的证书主体。
另外确认 CI 日志里出现「已签名」而不是「跳过签名」。

## 在签名生效之前

下载页与 README 里的「未签名」说明保持不动：审核通过、真正出了签名包之后再一起改掉。
没有配置 `SIGNPATH_API_TOKEN` 时，release 工作流会跳过签名步骤并在日志里写明
「本次未签名」，不影响正常发布。
