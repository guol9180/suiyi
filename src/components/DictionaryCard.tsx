import type { DictionaryResult } from "../types";

/** 词典结构化结果的展示卡：词条、音标、义项（词性 / 释义 / 例句） */
export function DictionaryCard({ dict }: { dict: DictionaryResult }) {
  return (
    <div className="dict">
      <div className="dict-head">
        <span className="w">{dict.word}</span>
        {dict.phonetic && <span className="ph">{dict.phonetic}</span>}
      </div>
      {dict.senses.map((s, i) => (
        <div className="sense" key={i}>
          {s.pos && <span className="pos">{s.pos}</span>}
          <div className="def">{s.def}</div>
          {s.example && <div className="eg">{s.example}</div>}
        </div>
      ))}
    </div>
  );
}
