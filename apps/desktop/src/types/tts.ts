export interface VoiceOption {
  providerId: string;
  voiceId: string;
  displayName: string;
  sendsDataOffDevice: boolean;
}

export interface SpeakerVoice {
  speakerId: string;
  label: string;
  voiceId: string;
}
