import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { App } from "./App";

const { convertFileSrcMock, invokeMock, listenMock, openMock } = vi.hoisted(() => ({
  convertFileSrcMock: vi.fn((path: string) => `http://asset.localhost/${encodeURIComponent(path)}`),
  invokeMock: vi.fn(),
  listenMock: vi.fn(() => Promise.resolve(() => undefined)),
  openMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: convertFileSrcMock,
  invoke: invokeMock,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: openMock }));

vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => undefined);

const importedSource = {
  projectId: "00000000-0000-4000-8000-000000000001",
  artifact: {
    id: "00000000-0000-4000-8000-000000000002",
    relative_path: "source/original.mp4",
    size_bytes: 1_048_576,
  },
  probeArtifactId: "00000000-0000-4000-8000-000000000003",
  originalName: "video thử nghiệm.mp4",
  absolutePath: "C:\\AppData\\studio.vietdub.desktop\\projects\\project\\source\\original.mp4",
  metadata: {
    duration_ms: 12_500,
    width: 1920,
    height: 1080,
    frame_rate: 30,
    video_codec: "h264",
    audio_codec: "aac",
    container: "mov,mp4",
    rotation_degrees: 0,
  },
  importStatus: "ready",
} as const;

describe("App", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    openMock.mockReset();
    convertFileSrcMock.mockClear();
    listenMock.mockClear();
    invokeMock.mockImplementation((command: string) => {
      if (command === "list_projects") return Promise.resolve([]);
      if (command === "log_preview_event") return Promise.resolve(undefined);
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });
  });

  it("keeps the automatic workflow on one workspace", () => {
    render(<App />);

    expect(screen.getByRole("heading", { name: "Chọn nguồn video" })).toBeInTheDocument();
    expect(screen.getByText("Toàn bộ quy trình chạy trong một màn hình")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Quy trình tự động" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /2 Transcript/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Bắt đầu xử lý/ })).toBeDisabled();
  });

  it("imports, probes and previews a selected local video before enabling processing", async () => {
    openMock.mockResolvedValue("D:\\video\\video thử nghiệm.mp4");
    invokeMock.mockImplementation((command: string) => {
      if (command === "list_projects") return Promise.resolve([]);
      if (command === "create_project") {
        return Promise.resolve({ id: importedSource.projectId, name: "video thử nghiệm", status: "draft" });
      }
      if (command === "import_local_media") return Promise.resolve(importedSource);
      if (command === "log_preview_event") return Promise.resolve(undefined);
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });
    const { container } = render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Chọn video" }));

    const video = await waitFor(() => {
      const element = container.querySelector("video");
      expect(element).not.toBeNull();
      return element as HTMLVideoElement;
    });
    expect(convertFileSrcMock).toHaveBeenCalledWith(importedSource.absolutePath);
    expect(video.closest(".video-viewport")).toHaveStyle({ aspectRatio: "1920 / 1080" });
    expect(screen.getByRole("button", { name: /Bắt đầu xử lý/ })).toBeDisabled();

    fireEvent.loadedMetadata(video);
    expect(screen.getByRole("button", { name: /Bắt đầu xử lý/ })).toBeEnabled();
    expect(screen.getByText("1920 × 1080")).toBeInTheDocument();
    expect(screen.getByText("1.0 MB")).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("import_local_media", {
      projectId: importedSource.projectId,
      sourcePath: "D:\\video\\video thử nghiệm.mp4",
    });
  });

  it("does not report ready when ffprobe tools are unavailable", async () => {
    openMock.mockResolvedValue("D:\\video\\sample.mp4");
    invokeMock.mockImplementation((command: string) => {
      if (command === "list_projects") return Promise.resolve([]);
      if (command === "create_project") {
        return Promise.resolve({ id: importedSource.projectId, name: "sample", status: "draft" });
      }
      if (command === "import_local_media") {
        return Promise.reject(Object.assign(new Error("media tools unavailable"), {
          code: "MEDIA_TOOLS_UNAVAILABLE",
        }));
      }
      return Promise.resolve(undefined);
    });
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Chọn video" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Chưa cấu hình FFmpeg/ffprobe");
    expect(screen.getByRole("button", { name: /Bắt đầu xử lý/ })).toBeDisabled();
    expect(screen.queryByText("Nguồn video đã sẵn sàng")).not.toBeInTheDocument();
  });

  it("extracts a Douyin URL from Chinese share text without marking it imported", () => {
    render(<App />);

    fireEvent.click(screen.getByRole("tab", { name: "Từ liên kết" }));
    fireEvent.change(screen.getByLabelText("Liên kết hoặc nội dung chia sẻ"), {
      target: { value: "复制此链接 https://v.douyin.com/bBnHr9vPnYA/ 打开Dou音搜索，直接观看视频！" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Dùng liên kết" }));

    expect(screen.getByRole("status")).toHaveTextContent("Douyin");
    expect(screen.getByLabelText("Liên kết hoặc nội dung chia sẻ")).toHaveValue(
      "https://v.douyin.com/bBnHr9vPnYA/",
    );
    expect(screen.getByRole("button", { name: /Bắt đầu xử lý/ })).toBeDisabled();
    expect(screen.getByRole("alert")).toHaveTextContent("chưa có bộ tải video được phê duyệt");
  });

  it("rejects unsupported or unsafe video links", () => {
    render(<App />);
    fireEvent.click(screen.getByRole("tab", { name: "Từ liên kết" }));
    fireEvent.change(screen.getByLabelText("Liên kết hoặc nội dung chia sẻ"), {
      target: { value: "http://example.com/video" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Dùng liên kết" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Liên kết không hợp lệ");
    expect(screen.getByRole("button", { name: /Bắt đầu xử lý/ })).toBeDisabled();
  });
});
