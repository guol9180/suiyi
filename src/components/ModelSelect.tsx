/**
 * 模型选择：自定义下拉，替换掉原先的原生 `input + datalist`。
 *
 * 为什么不用 datalist：它的样式完全跟系统走，和这套界面格格不入；
 * 也不支持分组、键盘导航与「刷新」这类操作。
 *
 * 这里同时保留手填能力 —— 中转站、火山方舟的接入点 ID（ep-…）
 * 这类值服务端列表里不会有，必须能自己敲。
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { Icon } from "./Icon";
import { describeError } from "../errorText";
import "./ModelSelect.css";

export interface ModelSelectProps {
  value: string;
  onChange: (value: string) => void;
  /** 服务端 /models 返回的候选 */
  remote: string[];
  /** 供应商预设里的推荐值 */
  preset: string[];
  placeholder?: string;
  /** 输入框下方的一行说明 */
  hint?: string;
  status: "idle" | "loading" | "error";
  /** 拉取失败的原因（服务端原文，展示时翻成人话） */
  error?: string;
  /** 能不能拉：没填 Key / 没填地址时为 false，面板里给出对应提示 */
  canFetch: boolean;
  onRefresh: () => void;
}

export function ModelSelect(props: ModelSelectProps) {
  const { value, onChange, remote, preset, status, error, canFetch, onRefresh } = props;
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const wrapRef = useRef<HTMLDivElement>(null);

  /** 预设里已经在服务端列表里的就不重复列 */
  const remoteOnly = useMemo(
    () => remote.filter((m) => !preset.includes(m)),
    [remote, preset],
  );
  const groups = useMemo(() => {
    const q = query.trim().toLowerCase();
    const filter = (list: string[]) => (q ? list.filter((m) => m.toLowerCase().includes(q)) : list);
    return [
      { key: "remote", label: "服务端返回", items: filter(remoteOnly) },
      { key: "preset", label: "预设推荐", items: filter(preset) },
    ].filter((g) => g.items.length > 0);
  }, [remoteOnly, preset, query]);
  const flat = useMemo(() => groups.flatMap((g) => g.items), [groups]);

  // 关掉面板时清掉过滤词与高亮，下次打开是从头开始
  useEffect(() => {
    if (!open) {
      setQuery("");
      setActive(0);
    }
  }, [open]);

  // 点击外部关闭
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (wrapRef.current && !wrapRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  function pick(model: string) {
    onChange(model);
    setOpen(false);
  }

  function onKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Escape") {
      setOpen(false);
      return;
    }
    if (!open && (e.key === "ArrowDown" || e.key === "Enter")) {
      setOpen(true);
      return;
    }
    if (!open) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((i) => Math.min(i + 1, Math.max(flat.length - 1, 0)));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (flat[active]) pick(flat[active]);
    }
  }

  const emptyText = !canFetch
    ? "先填好 Base URL 与 API Key，保存后这里会自动拉取模型"
    : status === "loading"
      ? "正在拉取模型…"
      : status === "error"
        ? describeError(error).title
        : "服务端没有返回模型列表，可以直接手填模型名";

  return (
    <div className="ms" ref={wrapRef}>
      <div className="ms-input">
        <input
          className="inp mono"
          role="combobox"
          aria-expanded={open}
          aria-controls="ms-listbox"
          aria-autocomplete="list"
          value={value}
          placeholder={props.placeholder ?? "模型名"}
          onChange={(e) => {
            onChange(e.target.value);
            setQuery(e.target.value);
            setActive(0);
            setOpen(true);
          }}
          onFocus={() => setOpen(true)}
          onKeyDown={onKeyDown}
        />
        <button
          type="button"
          className="ms-toggle"
          aria-label={open ? "收起模型列表" : "展开模型列表"}
          onClick={() => setOpen((v) => !v)}
        >
          <Icon name={open ? "chev-up" : "chev-down"} size="sm" />
        </button>
      </div>

      {props.hint && <div className="fhint">{props.hint}</div>}

      {open && (
        <div className="ms-pop" role="listbox" id="ms-listbox">
          {flat.length === 0 ? (
            <div className="ms-empty">{emptyText}</div>
          ) : (
            groups.map((g) => (
              <div key={g.key}>
                <div className="ms-group">{g.label}</div>
                {g.items.map((m) => {
                  const idx = flat.indexOf(m);
                  return (
                    <div
                      key={g.key + m}
                      role="option"
                      aria-selected={m === value}
                      className={`ms-opt${m === value ? " on" : ""}${idx === active ? " hot" : ""}`}
                      onMouseEnter={() => setActive(idx)}
                      onMouseDown={(e) => e.preventDefault()}
                      onClick={() => pick(m)}
                    >
                      <span className="mono">{m}</span>
                      {m === value && <Icon name="check" size="sm" />}
                    </div>
                  );
                })}
              </div>
            ))
          )}
          <div className="ms-foot">
            <button
              type="button"
              className="btn mini"
              disabled={!canFetch || status === "loading"}
              onClick={onRefresh}
            >
              <Icon name="refresh" size="sm" />
              {status === "loading" ? "拉取中" : "刷新模型"}
            </button>
            {status === "error" && <span className="ms-err">{describeError(error).title}</span>}
          </div>
        </div>
      )}
    </div>
  );
}
