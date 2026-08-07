import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState } from "react";

import { Button } from "../components/Button";
import { t } from "../lib/i18n";

const sourceOptions = [
  ["subtitle", t("autoSubtitle")],
  ["translate", t("translateBeforeVoice")],
  ["music", t("keepBackgroundMusic")],
] as const;

const pipelineSteps = [
  t("pipelineImport"),
  t("pipelineTranscribe"),
  t("pipelineTranslate"),
  t("pipelineVoice"),
  t("pipelineExport"),
] as const;

type SourceMode = "file" | "link";
type ImportState = "idle" | "importing" | "probing" | "ready" | "failed";
type PlayerState = "idle" | "loading" | "metadata" | "canplay" | "failed";

interface ProjectResponse {
  id: string;
  name: string;
  status: string;
  source_asset_id?: string | null;
}

interface ArtifactResponse {
  id: string;
  relative_path: string;
  size_bytes: number;
}

interface MediaMetadata {
  duration_ms: number;
  width: number;
  height: number;
  frame_rate: number;
  video_codec: string;
  audio_codec?: string | null;
  container: string;
  rotation_degrees: number;
}

interface SourceMediaResponse {
  projectId: string;
  artifact: ArtifactResponse;
  probeArtifactId: string;
  originalName: string;
  absolutePath: string;
  metadata: MediaMetadata;
  importStatus: "ready";
}

interface RenderMediaResponse {
  artifact: ArtifactResponse;
  absolutePath: string;
}

interface SourceStateEvent {
  projectId: string;
  status: ImportState;
  artifactId?: string | null;
  errorCode?: string | null;
}

interface JobResponse {
  id: string;
  project_id: string;
  job_type: string;
  status: "queued" | "running" | "paused" | "completed" | "failed" | "cancelled";
  progress: number;
  started_at?: string | null;
  error_code?: string | null;
  safe_error_message?: string | null;
}

interface TranscriptSegment {
  id: string;
  start_ms: number;
  end_ms: number;
  source_text: string;
  translated_text?: string | null;
}

interface LocalSource {
  kind: "file";
  name: string;
  projectId: string;
  media?: SourceMediaResponse;
}

interface RemoteSource {
  kind: "link";
  platform: "Bilibili" | "Douyin";
  url: string;
}

type VideoSource = LocalSource | RemoteSource;

const supportedRemoteHosts: Record<string, RemoteSource["platform"]> = {
  "b23.tv": "Bilibili",
  "bilibili.com": "Bilibili",
  "www.bilibili.com": "Bilibili",
  "douyin.com": "Douyin",
  "v.douyin.com": "Douyin",
  "www.douyin.com": "Douyin",
};

function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? t("untitledProject");
}

function projectNameFromPath(path: string): string {
  return basename(path).replace(/\.[^.]+$/, "").slice(0, 120) || t("untitledProject");
}

function directoryFromPath(path: string): string {
  const separator = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  if (separator < 0) return path;
  if (separator === 2 && /^[a-z]:/iu.test(path)) return path.slice(0, 3);
  return path.slice(0, separator);
}

function extractSharedUrl(raw: string): string | null {
  const match = raw.match(/https:\/\/[^\s<>"']+/iu);
  return match?.[0].replace(/[),.!?;:，。！？；：]+$/u, "") ?? null;
}

function parseRemoteSource(raw: string): RemoteSource | null {
  const extracted = extractSharedUrl(raw);
  if (!extracted || extracted.length > 2_048) return null;
  try {
    const url = new URL(extracted);
    const platform = supportedRemoteHosts[url.hostname.toLowerCase()];
    if (
      url.protocol !== "https:" ||
      url.hash !== "" ||
      url.port !== "" ||
      url.username !== "" ||
      url.password !== "" ||
      !platform
    ) {
      return null;
    }
    return { kind: "link", platform, url: url.toString() };
  } catch {
    return null;
  }
}

function commandErrorCode(error: unknown): string {
  if (typeof error === "object" && error !== null && "code" in error) {
    const code = (error as { code?: unknown }).code;
    return typeof code === "string" ? code : "UNKNOWN";
  }
  if (typeof error === "string") {
    const match = error.match(/[A-Z][A-Z0-9_]{2,}/u);
    return match?.[0] ?? "UNKNOWN";
  }
  return "UNKNOWN";
}

function importErrorMessage(code: string): string {
  if (code === "MEDIA_TOOLS_UNAVAILABLE") return t("mediaToolsRequired");
  if (code === "FFPROBE_INVALID_MEDIA") return t("invalidMediaMetadata");
  if (code === "FFPROBE_FAILED") return t("ffprobeFailed");
  if (code === "UNSUPPORTED_MEDIA") return t("unsupportedVideo");
  if (code === "SOURCE_TOO_LARGE") return t("videoTooLarge");
  return t("videoImportFailed");
}

function formatDuration(durationMs: number): string {
  const totalSeconds = Math.round(durationMs / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  return hours > 0
    ? `${hours}:${minutes.toString().padStart(2, "0")}:${seconds.toString().padStart(2, "0")}`
    : `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

function formatBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

export function App() {
  const [sourceMode, setSourceMode] = useState<SourceMode>("file");
  const [source, setSource] = useState<VideoSource | null>(null);
  const [importState, setImportState] = useState<ImportState>("idle");
  const [playerState, setPlayerState] = useState<PlayerState>("idle");
  const [linkInput, setLinkInput] = useState("");
  const [sourceError, setSourceError] = useState("");
  const [pipelineError, setPipelineError] = useState("");
  const [jobs, setJobs] = useState<JobResponse[]>([]);
  const [transcript, setTranscript] = useState<TranscriptSegment[]>([]);
  const [renderMedia, setRenderMedia] = useState<RenderMediaResponse | null>(null);
  const [fileDialogOpen, setFileDialogOpen] = useState(false);
  const videoRef = useRef<HTMLVideoElement>(null);
  const fileDialogPendingRef = useRef(false);

  const media = source?.kind === "file" ? source.media : undefined;
  const videoAspectRatio = useMemo(() => {
    if (!media) return "16 / 9";
    const quarterTurn = Math.abs(media.metadata.rotation_degrees) % 180 === 90;
    const width = quarterTurn ? media.metadata.height : media.metadata.width;
    const height = quarterTurn ? media.metadata.width : media.metadata.height;
    return `${width} / ${height}`;
  }, [media]);
  const videoUrl = useMemo(() => {
    const path = renderMedia?.absolutePath ?? media?.absolutePath;
    return path ? convertFileSrc(path) : "";
  }, [media, renderMedia]);
  const playerReady = playerState === "metadata" || playerState === "canplay";
  const sourceReady = source?.kind === "file" && importState === "ready" && playerReady;
  const sourceName = source?.kind === "file" ? source.name : source?.platform ?? t("untitledProject");
  const sourceDescription = source?.kind === "file" ? source.name : source?.url ?? t("notSelected");
  const latestJob = jobs.at(-1);
  const isProcessing = latestJob?.status === "queued" || latestJob?.status === "running";
  const hasTranslation = transcript.some((segment) => Boolean(segment.translated_text?.trim()));
  const voiceComplete = jobs.some((job) => job.job_type === "SYNTHESIZE" && job.status === "completed");
  const renderComplete = jobs.some((job) => job.job_type === "RENDER" && job.status === "completed");

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void listen<SourceStateEvent>("source-state", (event) => {
      if (!active) return;
      setSource((current) => {
        if (current?.kind !== "file" || current.projectId !== event.payload.projectId) return current;
        setImportState(event.payload.status);
        if (event.payload.status === "failed" && event.payload.errorCode) {
          setSourceError(importErrorMessage(event.payload.errorCode));
        }
        return current;
      });
    }).then((dispose) => {
      if (active) unlisten = dispose;
      else dispose();
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    let active = true;
    void invoke<ProjectResponse[]>("list_projects")
      .then(async (projects) => {
        const latest = projects.find(
          (project) => project.source_asset_id && project.status === "active",
        );
        if (!latest) return;
        const restored = await invoke<SourceMediaResponse>("get_source_media", {
          projectId: latest.id,
        });
        if (!active) return;
        setSource({
          kind: "file",
          name: restored.originalName,
          projectId: restored.projectId,
          media: restored,
        });
        setImportState("ready");
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (!media || !videoUrl) {
      setPlayerState("idle");
      return;
    }
    setPlayerState("loading");
    setSourceError("");
    let protocol = "asset:";
    try {
      protocol = new URL(videoUrl).protocol;
    } catch {
      // convertFileSrc always returns a URL; keep the safe protocol label if parsing fails.
    }
    void invoke("log_preview_event", {
      projectId: media.projectId,
      artifactId: media.artifact.id,
      event: "url_generated",
      protocol,
      mediaErrorCode: null,
    }).catch(() => undefined);
    const timeoutId = window.setTimeout(() => {
      setPlayerState((current) => {
        if (current === "loading") {
          setSourceError(t("videoLoadTimeout"));
          return "failed";
        }
        return current;
      });
    }, 15_000);
    return () => window.clearTimeout(timeoutId);
  }, [media, videoUrl]);

  useEffect(() => {
    if (source?.kind !== "file") {
      setJobs([]);
      setTranscript([]);
      return;
    }
    let active = true;
    const refresh = () => {
      void invoke<JobResponse[]>("list_project_jobs", { projectId: source.projectId })
        .then((nextJobs) => {
          if (!active || !Array.isArray(nextJobs)) return;
          setJobs(nextJobs);
          const asrCompleted = nextJobs.some(
            (job) => job.job_type === "TRANSCRIBE" && job.status === "completed",
          );
          if (asrCompleted) {
            void invoke<TranscriptSegment[]>("get_transcript", { projectId: source.projectId })
              .then((segments) => {
                if (active) setTranscript(segments);
              })
              .catch(() => undefined);
          }
          if (nextJobs.some((job) => job.job_type === "RENDER" && job.status === "completed")) {
            void invoke<RenderMediaResponse>("get_latest_render_media", {
              projectId: source.projectId,
            })
              .then((render) => {
                if (active) setRenderMedia(render);
              })
              .catch(() => undefined);
          }
        })
        .catch(() => undefined);
    };
    refresh();
    const intervalId = window.setInterval(refresh, 500);
    return () => {
      active = false;
      window.clearInterval(intervalId);
    };
  }, [source]);

  const resetWorkspace = () => {
    setSourceMode("file");
    setSource(null);
    setImportState("idle");
    setPlayerState("idle");
    setLinkInput("");
    setSourceError("");
    setPipelineError("");
    setJobs([]);
    setTranscript([]);
    setRenderMedia(null);
  };

  const selectSourceMode = (mode: SourceMode) => {
    setSourceMode(mode);
    setSource(null);
    setImportState("idle");
    setPlayerState("idle");
    setSourceError("");
    setPipelineError("");
    setRenderMedia(null);
    if (mode === "file") setLinkInput("");
  };

  const pickLocalVideo = async () => {
    if (fileDialogPendingRef.current) return;
    fileDialogPendingRef.current = true;
    setFileDialogOpen(true);
    setSourceError("");
    setPipelineError("");
    try {
      const rememberedDirectory = window.localStorage.getItem("vietdub.lastVideoDirectory");
      const selected = await open({
        directory: false,
        multiple: false,
        defaultPath: rememberedDirectory ?? "D:\\",
        filters: [{ name: t("videoFiles"), extensions: ["mp4", "mov", "mkv", "webm"] }],
      });
      if (typeof selected !== "string") return;
      window.localStorage.setItem("vietdub.lastVideoDirectory", directoryFromPath(selected));
      setImportState("importing");
      setPlayerState("idle");
      const project = await invoke<ProjectResponse>("create_project", {
        request: { name: projectNameFromPath(selected), workflowMode: "dubbed" },
      });
      setSource({ kind: "file", name: basename(selected), projectId: project.id });
      const imported = await invoke<SourceMediaResponse>("import_local_media", {
        projectId: project.id,
        sourcePath: selected,
      });
      setSource({
        kind: "file",
        name: imported.originalName,
        projectId: project.id,
        media: imported,
      });
      setImportState("ready");
    } catch (error) {
      setImportState("failed");
      setPlayerState("failed");
      setSourceError(importErrorMessage(commandErrorCode(error)));
    } finally {
      fileDialogPendingRef.current = false;
      setFileDialogOpen(false);
    }
  };

  const acceptRemoteSource = () => {
    const parsed = parseRemoteSource(linkInput);
    if (!parsed) {
      setSource(null);
      setImportState("failed");
      setSourceError(t("invalidVideoLink"));
      return;
    }
    setSource(parsed);
    setLinkInput(parsed.url);
    setImportState("idle");
    setSourceError("");
    setPipelineError(t("remoteDownloaderRequired"));
  };

  const logPlayerEvent = (event: "metadata_loaded" | "can_play" | "play" | "error", errorCode?: number) => {
    if (!media || !videoUrl) return;
    let protocol = "asset:";
    try {
      protocol = new URL(videoUrl).protocol;
    } catch {
      // Keep the safe default protocol label.
    }
    void invoke("log_preview_event", {
      projectId: media.projectId,
      artifactId: media.artifact.id,
      event,
      protocol,
      mediaErrorCode: errorCode ?? null,
    }).catch(() => undefined);
  };

  const handlePlayerError = () => {
    const code = videoRef.current?.error?.code;
    setPlayerState("failed");
    setSourceError(code === MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED ? t("unsupportedCodec") : t("videoPreviewFailed"));
    logPlayerEvent("error", code);
  };

  const startProcessing = async () => {
    if (!sourceReady || source?.kind !== "file") return;
    setPipelineError("");
    try {
      const command = hasTranslation ? "start_dub_render_job" : "start_transcript_job";
      const job = await invoke<JobResponse>(command, {
        projectId: source.projectId,
      });
      setJobs((current) => [...current.filter((item) => item.id !== job.id), job]);
    } catch (error) {
      const code = commandErrorCode(error);
      setPipelineError(
        code === "MEDIA_TOOLS_UNAVAILABLE" ? t("mediaToolsRequired") : t("jobStartFailed"),
      );
    }
  };

  const cancelProcessing = async () => {
    if (!latestJob || !isProcessing) return;
    try {
      const cancelled = await invoke<JobResponse>("cancel_job", { jobId: latestJob.id });
      setJobs((current) => current.map((job) => job.id === cancelled.id ? cancelled : job));
    } catch {
      setPipelineError(t("jobCancelFailed"));
    }
  };

  const seekToSegment = (segment: TranscriptSegment) => {
    if (!videoRef.current) return;
    videoRef.current.currentTime = segment.start_ms / 1000;
    void videoRef.current.play().catch(() => undefined);
  };

  const footerStatus = importState === "importing"
    ? t("copyingVideoIntoProject")
    : importState === "probing"
      ? t("probingVideo")
      : playerState === "loading"
        ? t("loadingVideoPreview")
        : sourceReady
          ? t("sourceImportedReady")
          : importState === "failed"
            ? t("sourceFailed")
            : t("waitingForVideo");

  return (
    <div className="app-shell">
      <header className="app-header">
        <div className="brand"><div className="brand-mark" aria-hidden="true">V</div><div><strong>{t("appName")}</strong><small>{t("workspace")}</small></div></div>
        <div className="header-title"><span className="header-title__icon" aria-hidden="true">▦</span><span>{t("workspace")}</span></div>
        <div className="header-actions"><span className="privacy-badge"><span className="status-dot" aria-hidden="true" />{t("localOnly")}</span><button className="icon-button" aria-label={t("settings")} title={t("settings")} type="button">⚙</button></div>
      </header>

      <main className="workspace">
        <section className="project-bar" aria-label={t("currentProject")}>
          <div className="project-summary"><span className="project-summary__label">{t("currentProject")}</span><strong>{sourceName}</strong></div>
          <div className="project-bar__message"><span aria-hidden="true">●</span>{t("singleScreenWorkflow")}</div>
          <Button className="button--compact" onClick={resetWorkspace}>＋ {t("newProject")}</Button>
        </section>

        <div className="workspace-grid">
          <aside className="quick-panel">
            <section className="side-card" aria-labelledby="quick-options-heading">
              <h2 id="quick-options-heading"><span aria-hidden="true">☷</span>{t("quickOptions")}</h2>
              <div className="option-list">{sourceOptions.map(([key, label], index) => <label className="check-row" key={key}><input defaultChecked={index !== 1} type="checkbox" /><span>{label}</span></label>)}</div>
            </section>
            <section className="side-card source-card" aria-labelledby="source-info-heading">
              <h2 id="source-info-heading"><span aria-hidden="true">▣</span>{t("sourceInfo")}</h2>
              <dl>
                <div><dt>{t("video")}</dt><dd title={sourceDescription}>{sourceDescription}</dd></div>
                {media ? <><div><dt>{t("duration")}</dt><dd>{formatDuration(media.metadata.duration_ms)}</dd></div><div><dt>{t("resolution")}</dt><dd>{media.metadata.width} × {media.metadata.height}</dd></div><div><dt>{t("fileSize")}</dt><dd>{formatBytes(media.artifact.size_bytes)}</dd></div><div><dt>{t("audio")}</dt><dd>{media.metadata.audio_codec ?? t("noAudio")}</dd></div></> : null}
                <div><dt>{t("language")}</dt><dd>{t("chineseToVietnamese")}</dd></div>
                <div><dt>{t("processing")}</dt><dd className={importState === "failed" ? "error-value" : "local-value"}>{footerStatus}</dd></div>
              </dl>
            </section>
            <section className="side-card pipeline-card" aria-labelledby="pipeline-heading">
              <h2 id="pipeline-heading"><span aria-hidden="true">↻</span>{t("automaticPipeline")}</h2>
              <ol>{pipelineSteps.map((step, index) => {
                const done = (index === 0 && sourceReady)
                  || (index === 1 && transcript.length > 0)
                  || (index === 2 && hasTranslation)
                  || (index === 3 && voiceComplete)
                  || (index === 4 && renderComplete);
                return <li className={done ? "pipeline-item pipeline-item--done" : "pipeline-item"} key={step}><span>{done ? "✓" : index + 1}</span>{step}</li>;
              })}</ol>
            </section>
          </aside>

          <section className="stage-panel source-stage" aria-live="polite">
            <div className="stage-heading"><div><span className="eyebrow">{t("videoSource")}</span><h1>{t("chooseVideoSource")}</h1></div><span className="format-note">MP4 · MOV · MKV · WEBM</span></div>
            <div className="source-mode-switch" role="tablist" aria-label={t("videoSourceType")}>
              <button aria-selected={sourceMode === "file"} className={sourceMode === "file" ? "source-mode source-mode--active" : "source-mode"} onClick={() => selectSourceMode("file")} role="tab" type="button"><span aria-hidden="true">▣</span> {t("fromComputer")}</button>
              <button aria-selected={sourceMode === "link"} className={sourceMode === "link" ? "source-mode source-mode--active" : "source-mode"} onClick={() => selectSourceMode("link")} role="tab" type="button"><span aria-hidden="true">↗</span> {t("fromLink")}</button>
            </div>

            {sourceMode === "file" ? (
              media && videoUrl ? (
                <div className="media-workspace">
                  <div className="video-frame">
                    <div className="video-viewport" style={{ aspectRatio: videoAspectRatio }}>
                      <video
                        key={videoUrl}
                        ref={videoRef}
                        src={videoUrl}
                        controls
                        preload="metadata"
                        style={{ width: "100%", height: "100%", objectFit: "contain" }}
                        onLoadedMetadata={() => { setPlayerState("metadata"); logPlayerEvent("metadata_loaded"); }}
                        onCanPlay={() => { setPlayerState("canplay"); logPlayerEvent("can_play"); }}
                        onPlay={() => logPlayerEvent("play")}
                        onError={handlePlayerError}
                      />
                    </div>
                    {playerState === "loading" ? <div className="video-loading" role="status">{t("loadingVideoPreview")}</div> : null}
                  </div>

                  <div className="media-summary"><div><h2>{media.originalName}</h2><p>{formatDuration(media.metadata.duration_ms)} · {media.metadata.width}×{media.metadata.height} · {media.metadata.video_codec.toUpperCase()} · {formatBytes(media.artifact.size_bytes)}</p></div><Button onClick={() => void pickLocalVideo()}>{t("changeVideo")}</Button></div>
                  <section className="transcript-empty">
                    <strong>{t("transcript")}</strong>
                    {isProcessing && latestJob ? <div className="job-status" role="status"><div><span>{latestJob.job_type.replaceAll("_", " ")}</span><strong>{Math.round(latestJob.progress)}%</strong></div><progress max="100" value={latestJob.progress} /><p>{t("processingNow")}</p></div> : null}
                    {transcript.length > 0 ? (
                      <div className="transcript-segments">{transcript.map((segment) => <button key={segment.id} onClick={() => seekToSegment(segment)} type="button"><span className="segment-time">{formatDuration(segment.start_ms)}</span><span className="segment-copy"><span lang="zh">{segment.source_text}</span>{segment.translated_text ? <span className="segment-translation" lang="vi">{segment.translated_text}</span> : null}</span></button>)}</div>
                    ) : latestJob ? (
                      <div className="job-status" role="status">
                        <div><span>{latestJob.job_type.replaceAll("_", " ")}</span><strong>{Math.round(latestJob.progress)}%</strong></div>
                        <progress max="100" value={latestJob.progress} />
                        <p className={latestJob.status === "failed" ? "job-error" : undefined}>{latestJob.safe_error_message ?? `${t("jobStatus")}: ${latestJob.status}`}</p>
                        {latestJob.error_code === "ASR_MODEL_UNAVAILABLE" ? <p className="model-required">{t("installAsrModel")}</p> : null}
                      </div>
                    ) : <p>{sourceReady ? t("readyForTranscription") : t("waitingForPlayer")}</p>}
                  </section>
                </div>
              ) : (
                <div className={source?.kind === "file" ? "drop-zone drop-zone--selected" : "drop-zone"}>
                  <div className="video-icon" aria-hidden="true">▶</div>
                  <h2>{source?.kind === "file" ? source.name : t("selectVideoFromComputer")}</h2>
                  <p>{importState === "importing" ? t("copyingVideoIntoProject") : importState === "probing" ? t("probingVideo") : source?.kind === "file" ? t("sourceNotReady") : t("nativePickerDescription")}</p>
                  <Button disabled={fileDialogOpen || importState === "importing" || importState === "probing"} onClick={() => void pickLocalVideo()}>{fileDialogOpen ? t("openingFilePicker") : importState === "importing" || importState === "probing" ? t("importingVideo") : source?.kind === "file" ? t("changeVideo") : t("selectVideo")}</Button>
                </div>
              )
            ) : (
              <div className={source?.kind === "link" ? "link-source link-source--accepted" : "link-source"}>
                <div className="link-source__intro"><div className="video-icon" aria-hidden="true">↗</div><div><h2>{t("pasteVideoLink")}</h2><p>{t("pasteShareTextDescription")}</p></div></div>
                <div className="platform-list" aria-label={t("supportedPlatforms")}><span className="platform-chip">抖 Douyin</span><span className="platform-chip">B Bilibili</span></div>
                <form className="link-form" noValidate onSubmit={(event) => { event.preventDefault(); acceptRemoteSource(); }}><label htmlFor="remote-video-url">{t("videoLinkOrShareText")}</label><div className="link-input-row"><input id="remote-video-url" inputMode="url" onChange={(event) => { setLinkInput(event.target.value); setSource(null); setImportState("idle"); setSourceError(""); setPipelineError(""); }} placeholder={t("shareTextPlaceholder")} type="text" value={linkInput} /><Button type="submit">{t("useThisLink")}</Button></div><p className="link-note">{t("extractLinkAutomatically")}</p></form>
                {source?.kind === "link" ? <div className="accepted-link" role="status"><span aria-hidden="true">!</span><div><strong>{source.platform}</strong><small>{t("linkAcceptedNeedsDownloader")}</small></div></div> : null}
              </div>
            )}
            {sourceError ? <p className="workspace-error" role="alert">{sourceError}</p> : null}
          </section>
        </div>

        <footer className="action-bar">
          <div className="status-copy"><span className={sourceReady ? "status-dot" : importState === "failed" ? "status-dot status-dot--error" : "status-dot status-dot--idle"} aria-hidden="true" /><span>{footerStatus}</span></div>
          {pipelineError ? <p className="pipeline-error" role="alert">{pipelineError}</p> : null}
          <div className="action-buttons">
            {isProcessing ? <button className="cancel-button" onClick={() => void cancelProcessing()} type="button">{t("cancelProcessing")}</button> : null}
            <button className="secondary-button" disabled={!sourceReady || isProcessing || hasTranslation} onClick={() => void startProcessing()} type="button">{t("extractSubtitles")}</button>
            <Button disabled={!sourceReady || isProcessing} onClick={() => void startProcessing()}>{renderComplete ? `${t("renderAgain")} →` : hasTranslation ? `${t("createVoiceAndExport")} →` : `${isProcessing ? t("processingNow") : t("startProcessing")} →`}</Button>
          </div>
        </footer>
      </main>
    </div>
  );
}
