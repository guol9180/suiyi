/**
 * 常见 AI 提供商的预设。前端是这份表的唯一真源：卡片选中后直接填进服务表单，
 * 不需要后端参与，也就不给 ServiceConfig 增加任何「提供商」字段。
 *
 * 每条都记了 docsUrl 与 checkedAt —— 这些地址、模型名都是从官方文档里抄的，
 * 文档会变，改预设前请重新打开链接核对，并把 checkedAt 一起更新。
 * 模型清单只是「推荐值 + 下拉候选」，真正的可用列表以服务端 /models 返回为准，
 * 所以模型输入框始终允许手填（中转站与自建网关全靠它）。
 */

import type { Protocol } from "./types";

export interface ProviderPreset {
  id: string;
  name: string;
  /** 卡片上的一行说明 */
  blurb: string;
  protocol: Protocol;
  /** 选中后填进 Base URL 的值；空串表示这家的地址要用户自己粘 */
  baseUrl: string;
  /** 输入框占位提示，用于地址含变量、必须替换的提供商 */
  baseUrlPlaceholder?: string;
  baseUrlNote?: string;
  /** 文档里核对过的模型，作为下拉候选；空数组表示以 /models 为准 */
  models: string[];
  modelHint?: string;
  /** 密钥控制台；null 表示这家不需要密钥 */
  keyUrl: string | null;
  requiresKey: boolean;
  docsUrl: string;
  checkedAt: string;
}

export const PROVIDERS: ProviderPreset[] = [
  {
    id: "deepseek",
    name: "DeepSeek",
    blurb: "官方 API，flash 便宜量大",
    protocol: "open_ai_compatible",
    baseUrl: "https://api.deepseek.com",
    models: ["deepseek-flash", "deepseek-v4-pro"],
    keyUrl: "https://platform.deepseek.com/api_keys",
    requiresKey: true,
    docsUrl: "https://api-docs.deepseek.com/zh-cn/quick_start/pricing",
    checkedAt: "2026-10-08",
  },
  {
    id: "zai",
    name: "智谱 GLM",
    blurb: "bigmodel.cn 开放平台",
    protocol: "open_ai_compatible",
    baseUrl: "https://open.bigmodel.cn/api/paas/v4",
    models: ["glm-5.3", "glm-5.3-flash", "glm-5.2"],
    keyUrl: "https://bigmodel.cn/usercenter/proj-mgmt/apikeys",
    requiresKey: true,
    docsUrl: "https://docs.bigmodel.cn/cn/guide/develop/openai/introduction",
    checkedAt: "2026-10-08",
  },
  {
    id: "kimi",
    name: "Kimi",
    blurb: "月之暗面 Moonshot",
    protocol: "open_ai_compatible",
    baseUrl: "https://api.moonshot.cn/v1",
    models: ["kimi-k3"],
    keyUrl: "https://platform.kimi.com/console/api-keys",
    requiresKey: true,
    docsUrl: "https://platform.kimi.com/docs/api/chat",
    checkedAt: "2026-10-08",
  },
  {
    id: "dashscope",
    name: "阿里百炼（通义）",
    blurb: "qwen 系列，地址带工作空间 ID",
    protocol: "open_ai_compatible",
    // 文档给的是带工作空间的地址，抄不了固定值，让用户从控制台复制
    baseUrl: "",
    baseUrlPlaceholder: "https://{WorkspaceId}.cn-beijing.maas.aliyuncs.com/compatible-mode/v1",
    baseUrlNote: "去百炼控制台复制兼容模式的地址，中间的 {WorkspaceId} 换成你的工作空间 ID。",
    models: ["qwen3.8-max", "qwen-plus", "qwen-turbo"],
    keyUrl: "https://bailian.console.aliyun.com/model/settings/api-key",
    requiresKey: true,
    docsUrl:
      "https://help.aliyun.com/zh/model-studio/developer-reference/compatibility-of-openai-with-dashscope",
    checkedAt: "2026-10-08",
  },
  {
    id: "volc",
    name: "火山方舟（豆包）",
    blurb: "模型字段通常填接入点 ID",
    protocol: "open_ai_compatible",
    baseUrl: "https://ark.cn-beijing.volces.com/api/v3",
    baseUrlNote: "主机名带地域，控制台在别的地域时按官方文档改；地址与鉴权说明见文档链接。",
    // 方舟的模型有两种写法：接入点 ID（ep-…）或模型 ID，文档里没有固定默认值
    models: [],
    modelHint: "填接入点 ID（ep-…），或 doubao-seed-2.0 这类模型 ID",
    keyUrl: "https://console.volcengine.com/ark",
    requiresKey: true,
    docsUrl: "https://www.volcengine.com/docs/82379/1298454",
    checkedAt: "2026-10-08",
  },
  {
    id: "siliconflow",
    name: "硅基流动",
    blurb: "一个 Key 调多家开源模型",
    protocol: "open_ai_compatible",
    baseUrl: "https://api.siliconflow.cn/v1",
    models: ["Pro/deepseek-ai/DeepSeek-R1"],
    keyUrl: "https://cloud.siliconflow.cn/account/ak",
    requiresKey: true,
    docsUrl: "https://docs.siliconflow.cn/cn/userguide/quickstart",
    checkedAt: "2026-10-08",
  },
  {
    id: "ollama",
    name: "Ollama（本地）",
    blurb: "跑在自己机器上，不需要密钥",
    protocol: "open_ai_compatible",
    baseUrl: "http://localhost:11434/v1",
    models: [],
    modelHint: "填本机已拉取的模型，如 qwen3:8b",
    keyUrl: null,
    requiresKey: false,
    docsUrl: "https://docs.ollama.com/api/openai-compatibility",
    checkedAt: "2026-10-08",
  },
  {
    id: "custom",
    name: "自定义 / 中转",
    blurb: "OneAPI、私有网关等任意兼容端点",
    protocol: "open_ai_compatible",
    baseUrl: "",
    models: [],
    keyUrl: null,
    requiresKey: true,
    docsUrl: "",
    checkedAt: "2026-10-08",
  },
];

/** 从地址里取主机名；取不到（空串、带变量的模板）返回空 */
export function hostOf(baseUrl: string): string {
  try {
    return new URL(baseUrl.trim()).host.toLowerCase();
  } catch {
    return "";
  }
}

/**
 * 已保存的服务反查它属于哪家预设：只按主机名比对。
 * 用不上就返回 undefined（自定义地址、插件服务都走这条路）。
 */
export function presetOf(baseUrl: string): ProviderPreset | undefined {
  const host = hostOf(baseUrl);
  if (!host) return undefined;
  return PROVIDERS.find((p) => p.baseUrl && hostOf(p.baseUrl) === host);
}

export interface CleanKey {
  /** 清洗后的 Key */
  value: string;
  /** 给用户看的一行回显：只出现尾 4 位，绝不显示全量 */
  note: string;
  /** 清洗时发现的可疑点，没有则为空 */
  warn: string;
}

/**
 * 粘贴板里的 Key 常见有两种脏：整段带引号、前后带换行。
 * 这里顺手洗掉，并把「可疑」说出来 —— 缺字符这类错误只有用户自己能看出来，
 * 所以要给他尾号和长度去跟控制台对。
 */
export function cleanKey(raw: string): CleanKey {
  const value = raw.trim().replace(/^["'`]+/, "").replace(/["'`]+$/, "").trim();
  const tail = value.length > 4 ? "…" + value.slice(-4) : value;
  const note = value ? `已粘贴 ${tail}（${value.length} 字符）` : "剪贴板里没有文本";
  let warn = "";
  if (/\s/.test(value)) {
    warn = "里面还有空格或换行，可能是复制时多带了内容";
  } else if (/[^\x20-\x7e]/.test(value)) {
    warn = "里面有非 ASCII 字符，Key 一般只有字母数字和连字符";
  } else if (value.length < 16) {
    warn = "长度偏短，确认没漏字符";
  }
  return { value, note, warn };
}
