import { t } from "../../lib/i18n";
import type { SpeakerVoice, VoiceOption } from "../../types/tts";

interface Props {
  voices: VoiceOption[];
  speakers: SpeakerVoice[];
  warnings?: string[];
  onAssign?: (speakerId: string, voiceId: string) => void;
  onPreview?: (speakerId: string) => void;
}

export function VoiceStudio({
  voices,
  speakers,
  warnings = [],
  onAssign,
  onPreview,
}: Props) {
  return (
    <section className="voice-studio" aria-labelledby="voice-heading">
      <div className="section-heading">
        <div>
          <h2 id="voice-heading">{t("voiceStudio")}</h2>
        </div>
        <span className="cloud-badge">{t("cloudVoiceDisclosure")}</span>
      </div>

      {speakers.length === 0 ? (
        <p className="translation-empty">{t("noSpeakers")}</p>
      ) : (
        <div className="voice-rows">
          {speakers.map((speaker) => (
            <article className="voice-row" key={speaker.speakerId}>
              <strong>{speaker.label}</strong>
              <select
                aria-label={`${t("voiceFor")} ${speaker.label}`}
                value={speaker.voiceId}
                onChange={(event) =>
                  onAssign?.(speaker.speakerId, event.target.value)
                }
              >
                {voices.map((voice) => (
                  <option
                    key={`${voice.providerId}:${voice.voiceId}`}
                    value={voice.voiceId}
                  >
                    {voice.displayName}
                  </option>
                ))}
              </select>
              <button type="button" onClick={() => onPreview?.(speaker.speakerId)}>
                {t("previewVoice")}
              </button>
            </article>
          ))}
        </div>
      )}

      {warnings.map((warning) => (
        <p className="tts-warning" key={warning} role="alert">
          {warning}
        </p>
      ))}
    </section>
  );
}
