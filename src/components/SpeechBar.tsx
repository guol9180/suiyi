// 朗读条：播放/暂停、进度、时长、语速、音色。
// 进度与时长全部来自系统（MCI 的 position / length），不在这里做任何估算。
import { Icon } from "./Icon";
import type { SpeechState } from "../types";

/** 可选的语速档位，与 src-tauri/src/speech.rs 的速率区间一致 */
export const SPEECH_RATES = [0.8, 1, 1.5];

/** 毫秒转 0:08 这种时长显示 */
export function clock(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000));
  const mm = Math.floor(total / 60);
  const ss = String(total % 60).padStart(2, "0");
  return `${mm}:${ss}`;
}

export function SpeechBar(props: {
  state: SpeechState;
  /** 音色显示名，空串表示系统默认 */
  voiceLabel: string;
  /** 试听时用的文本，为空则按钮禁用 */
  canPlay: boolean;
  onToggle: () => void;
  onStop: () => void;
  onRate: (rate: number) => void;
  onPickVoice: () => void;
}) {
  const { state } = props;
  const total = state.durationMs;
  const percent = total > 0 ? Math.min(100, (state.positionMs / total) * 100) : 0;

  return (
    <div className="tts">
      <button
        className="play"
        disabled={!state.active && !props.canPlay}
        aria-label={state.playing ? "暂停" : "播放"}
        onClick={props.onToggle}
      >
        <Icon name={state.playing ? "pause" : "play"} />
      </button>

      <div
        className="track"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={Math.round(total / 1000)}
        aria-valuenow={Math.round(state.positionMs / 1000)}
        aria-label="朗读进度"
      >
        <i style={{ width: `${percent}%` }} />
      </div>

      <span className="clock">
        {clock(state.positionMs)} / {clock(total)}
      </span>

      <span className="seg">
        {SPEECH_RATES.map((r) => (
          <span
            key={r}
            className={Math.abs(state.rate - r) < 0.01 ? "on" : ""}
            onClick={() => props.onRate(r)}
          >
            {r.toFixed(1)}×
          </span>
        ))}
      </span>

      {state.active && (
        <button className="tts-stop" onClick={props.onStop} title="停止朗读" aria-label="停止朗读">
          <Icon name="close" size="sm" />
        </button>
      )}

      <span className="chip acc mini vname" title={props.voiceLabel || "系统默认音色"}>
        {props.voiceLabel || "系统默认"}
      </span>
      <button className="btn mini" onClick={props.onPickVoice}>
        切换音色
      </button>
    </div>
  );
}
