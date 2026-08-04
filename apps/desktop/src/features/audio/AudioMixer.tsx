import { t } from "../../lib/i18n";
import type { AudioMixSettings } from "../../types/audio";

interface Props {
  settings: AudioMixSettings;
  separationMode?: "separated" | "fallback_attenuation";
  onChange?: (settings: AudioMixSettings) => void;
}

const controls = [
  ["backgroundGain", "audioBackground"],
  ["voiceGain", "audioVoice"],
  ["musicGain", "audioMusic"],
  ["originalVoiceGain", "audioOriginalVoice"],
  ["duckingGain", "audioDucking"],
] as const;

export function AudioMixer({ settings, separationMode, onChange }: Props) {
  return (
    <section className="audio-mixer" aria-labelledby="audio-mixer-heading">
      <div className="section-heading">
        <div>
          <h2 id="audio-mixer-heading">{t("audioMixer")}</h2>
        </div>
        <span className="local-badge">{t("audioLocalEngine")}</span>
      </div>
      {separationMode === "fallback_attenuation" && (
        <p className="tts-warning" role="alert">{t("audioFallbackWarning")}</p>
      )}
      <div className="audio-controls">
        {controls.map(([key, label]) => (
          <label key={key}>
            <span>{t(label)}</span>
            <input
              type="range"
              min="0"
              max={key === "duckingGain" ? "1" : "2"}
              step="0.05"
              value={settings[key]}
              onChange={(event) =>
                onChange?.({ ...settings, [key]: Number(event.target.value) })
              }
            />
            <output>{settings[key].toFixed(2)}</output>
          </label>
        ))}
      </div>
    </section>
  );
}
