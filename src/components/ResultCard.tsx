/**
 * 多服务结果卡：翻译页与划词弹窗共用，结构只此一份（AGENTS.md 规范 10）。
 * 页面差异全部走 props：
 * - 重试脚注只有翻译页要（弹窗没有单卡重试）；
 * - 复制按钮弹窗用纯图标省宽度；
 * - 「输出 N 字」只有翻译页显示。
 * 卡片的视觉结构在 base.css 的 .rescard 一节，页面 CSS 只许覆盖表面（底色、圆角、内距）。
 */
import { Icon } from "./Icon";
import { DictionaryCard } from "./DictionaryCard";
import { describeError, statusCodeOf } from "../errorText";
import { formatSecs } from "../format";
import type { CardState } from "../types";

interface ResultCardProps {
  card: CardState;
  /** 传入则失败卡显示排查提示与「重试」按钮 */
  onRetry?: (serviceId: string) => void;
  /** 复制按钮的文字；不传则渲染纯图标（弹窗窄，title 兜底语义） */
  copyLabel?: string;
  /** 成功纯文本卡是否显示「输出 N 字」 */
  showMeta?: boolean;
}

export function ResultCard({ card: c, onRetry, copyLabel, showMeta }: ResultCardProps) {
  return (
    <div className="rescard">
      <div className="rh">
        <span className={`dot${c.status === "error" ? " err" : ""}`} />
        <b>{c.name}</b>
        {c.status === "done" && c.elapsedMs != null && <span>{formatSecs(c.elapsedMs)}</span>}
        {c.status === "streaming" && <span>流式生成中…</span>}
        <span style={{ flex: 1 }} />
        {c.text && (
          <button
            className="btn mini"
            title={copyLabel ? undefined : "复制这条译文"}
            onClick={() => void navigator.clipboard.writeText(c.text)}
          >
            <Icon name="copy" size="sm" />
            {copyLabel}
          </button>
        )}
      </div>
      {c.error ? (
        <>
          {/* 卡片上只放结论，原文留着悬停可查；历史里存的是完整原文 */}
          <div className="rt rc-err" title={c.error}>{describeError(c.error).title}</div>
          {onRetry && (
            <div className="rc-foot">
              <span className="muted">
                {statusCodeOf(c.error)
                  ? `设置 → 服务配置里可以用「测试连接」查${statusCodeOf(c.error)}`
                  : "配额限流可在「设置 → 服务配置」用测试连接排查"}
              </span>
              <button className="btn mini" onClick={() => void onRetry(c.id)}>
                <Icon name="refresh" size="sm" />重试
              </button>
            </div>
          )}
        </>
      ) : c.dictionary ? (
        <DictionaryCard dict={c.dictionary} />
      ) : (
        <div className="rt">
          {c.text}
          {c.status === "streaming" && <span className="caret" />}
        </div>
      )}
      {showMeta && c.status === "done" && c.text && !c.dictionary && (
        <div className="rc-meta">输出 {c.text.length} 字</div>
      )}
    </div>
  );
}
