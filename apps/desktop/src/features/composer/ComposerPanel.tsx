import { t } from "../../lib/i18n";
import type { ComposerSettings } from "../../types/composer";

interface Props {
  settings: ComposerSettings;
  onChange?: (settings: ComposerSettings) => void;
}

export function ComposerPanel({ settings, onChange }: Props) {
  const update = <K extends keyof ComposerSettings>(key: K, value: ComposerSettings[K]) =>
    onChange?.({ ...settings, [key]: value });

  return (
    <section className="composer-panel" aria-labelledby="composer-heading">
      <div className="section-heading">
        <div>
          <h2 id="composer-heading">{t("composer")}</h2>
        </div>
        <span className="local-badge">{t("composerTypedPlan")}</span>
      </div>
      <div className="composer-controls">
        <label>
          <span>{t("composerAspect")}</span>
          <select aria-label={t("composerAspect")} value={settings.aspect}
            onChange={(event) => update("aspect", event.target.value as ComposerSettings["aspect"])}>
            <option value="source">{t("composerSourceAspect")}</option>
            <option value="landscape16x9">16:9</option>
            <option value="square1x1">1:1</option>
            <option value="vertical9x16">9:16</option>
          </select>
        </label>
        <label>
          <span>{t("composerSubtitles")}</span>
          <select aria-label={t("composerSubtitles")} value={settings.subtitleMode}
            onChange={(event) => update("subtitleMode", event.target.value as ComposerSettings["subtitleMode"])}>
            <option value="none">{t("composerSubtitleNone")}</option>
            <option value="soft">{t("composerSubtitleSoft")}</option>
            <option value="burned">{t("composerSubtitleBurned")}</option>
          </select>
        </label>
        <label>
          <span>{t("composerPreset")}</span>
          <select aria-label={t("composerPreset")} value={settings.previewPreset}
            onChange={(event) => update("previewPreset", event.target.value as ComposerSettings["previewPreset"])}>
            <option value="draft">{t("composerDraft")}</option>
            <option value="final">{t("composerFinal")}</option>
          </select>
        </label>
        <label>
          <span>{t("composerSpeed")}</span>
          <input aria-label={t("composerSpeed")} type="range" min="0.25" max="4" step="0.05"
            value={settings.speed} onChange={(event) => update("speed", Number(event.target.value))} />
          <output>{settings.speed.toFixed(2)}×</output>
        </label>
        <label>
          <span>{t("composerBlur")}</span>
          <input aria-label={t("composerBlur")} type="range" min="0" max="20" step="0.5"
            value={settings.blurRadius} onChange={(event) => update("blurRadius", Number(event.target.value))} />
          <output>{settings.blurRadius.toFixed(1)}</output>
        </label>
      </div>
    </section>
  );
}
