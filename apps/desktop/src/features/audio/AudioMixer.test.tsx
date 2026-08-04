import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { AudioMixSettings } from "../../types/audio";
import { AudioMixer } from "./AudioMixer";

const settings: AudioMixSettings = {
  projectId: "00000000-0000-4000-8000-000000000001",
  backgroundGain: 0.75,
  voiceGain: 1,
  musicGain: 0.5,
  originalVoiceGain: 0,
  duckingGain: 0.4,
  fadeInMs: 30,
  fadeOutMs: 50,
  targetRmsDbfs: -18,
  limiterPeak: 0.95,
};

describe("AudioMixer", () => {
  it("updates typed gain controls", () => {
    const change = vi.fn();
    render(<AudioMixer settings={settings} onChange={change} />);
    fireEvent.change(screen.getByLabelText(/Nhạc nền/i), { target: { value: "0.9" } });
    expect(change).toHaveBeenCalledWith({ ...settings, musicGain: 0.9 });
  });

  it("discloses attenuation fallback", () => {
    render(<AudioMixer settings={settings} separationMode="fallback_attenuation" />);
    expect(screen.getByRole("alert")).toHaveTextContent(/giảm âm lượng/i);
  });
});
