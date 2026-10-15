/**
 * 语言方向行：来源、互换、目标。翻译页与划词弹窗共用（AGENTS.md 规范 10）。
 * 两个 select 的类名由页面传入——主窗用 .inp（自适应宽），弹窗用固定宽的 .pop-lang。
 */
import { Icon } from "./Icon";
import { SOURCE_LANGS, TARGET_LANGS, swapLanguages } from "../types";

interface LangPickerProps {
  from: string;
  to: string;
  onChangeFrom: (v: string) => void;
  onChangeTo: (v: string) => void;
  /** 两个 select 的类名，宽度策略由页面 CSS 决定 */
  selectClassName: string;
}

export function LangPicker({ from, to, onChangeFrom, onChangeTo, selectClassName }: LangPickerProps) {
  return (
    <>
      <select className={selectClassName} value={from} onChange={(e) => onChangeFrom(e.target.value)}>
        {SOURCE_LANGS.map((l) => (
          <option key={l} value={l}>{l}</option>
        ))}
      </select>
      <button
        className="swap"
        title="互换语言方向"
        aria-label="互换语言方向"
        onClick={() => {
          const next = swapLanguages(from, to);
          onChangeFrom(next.from);
          onChangeTo(next.to);
        }}
      >
        <Icon name="swap" size="sm" />
      </button>
      <select className={selectClassName} value={to} onChange={(e) => onChangeTo(e.target.value)}>
        {TARGET_LANGS.map((l) => (
          <option key={l} value={l}>{l}</option>
        ))}
      </select>
    </>
  );
}
