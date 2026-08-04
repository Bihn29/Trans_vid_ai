export type AspectPreset = "source" | "landscape16x9" | "square1x1" | "vertical9x16";
export type SubtitleMode = "none" | "soft" | "burned";
export type PreviewPreset = "draft" | "final";

export interface ComposerSettings {
  aspect: AspectPreset;
  subtitleMode: SubtitleMode;
  previewPreset: PreviewPreset;
  speed: number;
  blurRadius: number;
  paddingColor: string;
}
