/**
 * 把服务端返回的原始错误翻成人话。
 *
 * 起因：DeepSeek 对错误的 Key 回的是
 *   401 Unauthorized: {"error":{"message":"Authentication Fails, Your api key: ****Xk7Q is invalid …"}}
 * 用户看到这一串只会得出「它说我没授权」。
 *
 * 这里的取舍：**标题只写结论，原文一律保留**。诊断要靠原文，所以折叠起来而不是丢掉；
 * 能从原文抓到 Key 尾号就单独抬出来 —— 那是用户拿去跟控制台对照的唯一线索。
 */

export interface ErrorInfo {
  /** 给用户看的一句结论 */
  title: string;
  /** 可能的原因或下一步动作 */
  hint?: string;
  /** HTTP 状态码，抓不到为空串 */
  code: string;
  /** 服务端回显的 Key 尾号（如 Xk7Q），抓不到为空串 */
  tail: string;
  /** 原始错误文本 */
  raw: string;
}

const CODE_RE = /\b(4\d{2}|5\d{2})\b/;
/** DeepSeek 写 "api key: ****Xk7Q is invalid"，OpenAI 写 "sk-***xYz" */
const KEY_TAIL_RE = /\*{2,}\s*([A-Za-z0-9_-]{2,12})\b/;

function tailOf(raw: string): string {
  const m = raw.match(KEY_TAIL_RE);
  return m ? m[1] : "";
}

export function statusCodeOf(raw: string | undefined): string {
  if (!raw) return "";
  const m = raw.match(CODE_RE);
  return m ? m[1] : "";
}

export function describeError(raw: string | undefined): ErrorInfo {
  const text = (raw ?? "").trim();
  const code = statusCodeOf(text);
  const tail = tailOf(text);
  const base = { code, tail, raw: text };

  if (!text) return { ...base, title: "没有拿到错误详情" };

  switch (code) {
    case "401":
      return {
        ...base,
        title: "密钥无效：服务端不认这枚 Key",
        hint: "常见原因：复制时漏了字符、Key 已在控制台删除、或用的是另一个站点的 Key。去控制台重新建一枚，回来用「从剪贴板粘贴」重贴一次。",
      };
    case "402":
      return { ...base, title: "账户余额不足", hint: "去服务商控制台充值，或先换一个服务。" };
    case "403":
      return {
        ...base,
        title: "这枚 Key 没有该模型的权限",
        hint: "确认账号已开通这个模型，或把模型名换成账号可用的那一个。",
      };
    case "404":
      return {
        ...base,
        title: "地址或模型名不对",
        hint: "Base URL 要指向 OpenAI 兼容的版本段（如 /v1），模型名要和服务端一致。",
      };
    case "429":
      return { ...base, title: "请求太频繁或额度用尽", hint: "等一会儿再试，或去控制台看用量。" };
  }
  if (code.startsWith("5")) {
    return { ...base, title: `服务端出错（${code}）`, hint: "不是本地配置的问题，隔一会儿重试。" };
  }
  if (/超时|timed? ?out/i.test(text)) {
    return { ...base, title: "请求超时", hint: "网络或代理不通，或这家服务此刻很慢。" };
  }
  if (/连接失败|连接超时|dns|error sending request|connection/i.test(text)) {
    return { ...base, title: "连不上这家服务", hint: "检查 Base URL、网络与代理设置。" };
  }
  if (/尚未设置 API Key|请先设置 API Key/.test(text)) {
    return { ...base, title: "还没有填 API Key", hint: "在下面的输入框里填上，或用「从剪贴板粘贴」。" };
  }

  // 认不出来的错误：直接把原文本当标题，至少不丢信息
  const firstLine = text.split("\n")[0];
  return { ...base, title: firstLine.length > 120 ? firstLine.slice(0, 120) + "…" : firstLine };
}
