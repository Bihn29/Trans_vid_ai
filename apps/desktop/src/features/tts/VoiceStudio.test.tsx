import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { VoiceStudio } from "./VoiceStudio";

const voices = [
  {
    providerId: "openai-compatible",
    voiceId: "alloy",
    displayName: "Alloy",
    sendsDataOffDevice: true,
  },
  {
    providerId: "openai-compatible",
    voiceId: "nova",
    displayName: "Nova",
    sendsDataOffDevice: true,
  },
];

describe("VoiceStudio", () => {
  it("routes two speakers and previews independently", () => {
    const assign = vi.fn();
    const preview = vi.fn();
    render(
      <VoiceStudio
        voices={voices}
        speakers={[
          { speakerId: "a", label: "A", voiceId: "alloy" },
          { speakerId: "b", label: "B", voiceId: "nova" },
        ]}
        onAssign={assign}
        onPreview={preview}
      />,
    );

    fireEvent.change(screen.getByLabelText(/A$/), {
      target: { value: "nova" },
    });
    const previewButtons = screen.getAllByText(/Nghe thử/i);
    expect(previewButtons).toHaveLength(2);
    fireEvent.click(previewButtons[1]!);

    expect(assign).toHaveBeenCalledWith("a", "nova");
    expect(preview).toHaveBeenCalledWith("b");
    expect(screen.getByText(/ra ngoài thiết bị/i)).toBeInTheDocument();
  });

  it("shows fitting warnings", () => {
    render(
      <VoiceStudio
        voices={voices}
        speakers={[]}
        warnings={["Hãy rút ngắn bản dịch"]}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(/rút ngắn/i);
  });
});
