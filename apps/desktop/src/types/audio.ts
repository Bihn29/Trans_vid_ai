export interface AudioMixSettings {
  projectId: string;
  backgroundGain: number;
  voiceGain: number;
  musicGain: number;
  originalVoiceGain: number;
  duckingGain: number;
  fadeInMs: number;
  fadeOutMs: number;
  targetRmsDbfs: number;
  limiterPeak: number;
}
