import { fireEvent, render, screen } from "@testing-library/react";
import { vi } from "vitest";

import { ComposerPanel } from "./ComposerPanel";

const settings = {
  aspect: "source" as const,
  subtitleMode: "soft" as const,
  previewPreset: "draft" as const,
  speed: 1,
  blurRadius: 0,
  paddingColor: "#000000",
};

describe("ComposerPanel", () => {
  it("exposes only bounded typed composer choices", () => {
    render(<ComposerPanel settings={settings} />);
    expect(screen.getByRole("option", { name: "16:9" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: /phụ đề mềm/i })).toBeInTheDocument();
    expect(screen.queryByLabelText(/FFmpeg/i)).not.toBeInTheDocument();
  });

  it("emits a typed preview preset change", () => {
    const onChange = vi.fn();
    render(<ComposerPanel settings={settings} onChange={onChange} />);
    fireEvent.change(screen.getByLabelText(/chất lượng xem trước/i), { target: { value: "final" } });
    expect(onChange).toHaveBeenCalledWith({ ...settings, previewPreset: "final" });
  });
});
