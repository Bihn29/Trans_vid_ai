import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { TranslationReview } from "./TranslationReview";

const providers = [
  { providerId: "local", displayName: "Cục bộ", sendsDataOffDevice: false },
  { providerId: "openai-compatible", displayName: "Cloud", sendsDataOffDevice: true },
];

describe("TranslationReview", () => {
  it("discloses cloud transfer before use", () => {
    render(<TranslationReview providers={providers} rows={[]} />);
    expect(screen.getByText(/trên thiết bị/i)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Nhà cung cấp dịch"), {
      target: { value: "openai-compatible" },
    });
    expect(screen.getByRole("alert")).toHaveTextContent(/ra ngoài thiết bị/i);
  });

  it("blocks a translation that changes a locked proper name", () => {
    const onSave = vi.fn();
    render(
      <TranslationReview
        onSave={onSave}
        providers={providers}
        rows={[
          {
            id: "segment-1",
            sourceText: "Alice 来了",
            translatedText: "Alice đã đến",
            lockedNames: ["Alice"],
          },
        ]}
      />,
    );
    fireEvent.change(screen.getByLabelText("Bản dịch segment-1"), {
      target: { value: "Cô ấy đã đến" },
    });
    expect(screen.getByText(/Tên riêng bị khóa/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Lưu đoạn dịch" })).toBeDisabled();
    expect(onSave).not.toHaveBeenCalled();
  });
});
