use std::{
    ffi::OsString,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    domain::{
        Artifact, ArtifactKind, ArtifactVerification, ComposerConfig, CoreError, MediaMetadata,
        PreviewPreset, RenderQualityReport, Segment, StageName, SubtitleMode,
    },
    jobs::{ClaimedJob, PersistentQueue},
    media::{FfprobeAdapter, MediaToolError},
    persistence::SegmentRepository,
    processes::{ApprovedTool, SupervisedProcess, ToolError, ToolInvocation},
};

use super::{ArtifactRegistry, ProjectLayout, ProjectRelativePath};

#[derive(Debug, Error)]
pub enum ComposerError {
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error("composer filesystem operation failed")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Tool(#[from] ToolError),
    #[error(transparent)]
    Media(#[from] MediaToolError),
    #[error("composer output failed quality control")]
    QualityControl,
    #[error("composer tool did not produce output")]
    MissingOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposerExecutionRequest {
    pub project_id: Uuid,
    pub source_artifact_id: Uuid,
    pub mixed_audio_artifact_id: Uuid,
    pub config: ComposerConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderPlan {
    pub arguments: Vec<String>,
    pub filter_graph: String,
    pub output_width: u32,
    pub output_height: u32,
    pub expected_duration_ms: u64,
    pub identity: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposerExecutionResult {
    pub render: Artifact,
    pub subtitle: Option<Artifact>,
    pub quality_report: RenderQualityReport,
    pub quality_artifact: Artifact,
}

#[derive(Clone)]
pub struct ComposerPipelineService {
    layout: ProjectLayout,
    artifacts: ArtifactRegistry,
    segments: SegmentRepository,
    ffmpeg: ApprovedTool,
    ffprobe: FfprobeAdapter,
    supervisor: SupervisedProcess,
}

impl ComposerPipelineService {
    pub fn new(
        layout: ProjectLayout,
        artifacts: ArtifactRegistry,
        segments: SegmentRepository,
        ffmpeg: ApprovedTool,
        ffprobe: FfprobeAdapter,
        supervisor: SupervisedProcess,
    ) -> Self {
        Self {
            layout,
            artifacts,
            segments,
            ffmpeg,
            ffprobe,
            supervisor,
        }
    }

    pub async fn execute(
        &self,
        request: &ComposerExecutionRequest,
        cancellation: CancellationToken,
    ) -> Result<ComposerExecutionResult, ComposerError> {
        if request.config.project_id != request.project_id {
            return Err(CoreError::InvalidInput("composer project").into());
        }
        let (source_artifact, source) = self.verified_artifact(
            request.project_id,
            request.source_artifact_id,
            ArtifactKind::SourceVideo,
        )?;
        let (audio_artifact, _audio) = self.verified_artifact(
            request.project_id,
            request.mixed_audio_artifact_id,
            ArtifactKind::MixedAudio,
        )?;
        let source_metadata = self
            .ffprobe
            .probe(&source, cancellation.child_token())
            .await?;
        request.config.validate_for_source(&source_metadata)?;
        let root = self.layout.project_root(request.project_id)?;
        let segments = self.segments.list_by_project(request.project_id)?;

        let subtitle = if request.config.subtitle_mode == SubtitleMode::None {
            None
        } else {
            Some(self.write_subtitle(request, &segments, source_metadata.duration_ms)?)
        };
        let text_paths = self.write_text_overlays(request)?;
        let image_artifacts = request
            .config
            .image_overlays
            .iter()
            .map(|overlay| {
                self.verified_artifact(
                    request.project_id,
                    overlay.artifact_id,
                    ArtifactKind::OverlayImage,
                )
                .map(|(artifact, _)| artifact)
            })
            .collect::<Result<Vec<_>, _>>()?;

        let render_id = Uuid::new_v4();
        let render_relative = ProjectRelativePath::parse(format!("renders/{render_id}.mp4"))?;
        let render_path = self
            .layout
            .prepare_output(request.project_id, &render_relative)?;
        let temporary_relative =
            ProjectRelativePath::parse(format!("renders/.{render_id}.partial.mp4"))?;
        let temporary_path = self
            .layout
            .prepare_output(request.project_id, &temporary_relative)?;
        let identity = render_identity(
            &request.config,
            &source_artifact,
            &audio_artifact,
            &image_artifacts,
            &segments,
        )?;
        let plan = build_render_plan(
            &request.config,
            &source_metadata,
            &source_artifact.relative_path,
            &audio_artifact.relative_path,
            subtitle.as_ref().map(|value| value.relative_path.as_str()),
            &text_paths,
            &image_artifacts
                .iter()
                .map(|value| value.relative_path.clone())
                .collect::<Vec<_>>(),
            temporary_relative.as_str(),
            identity,
        )?;
        let invocation =
            ToolInvocation::new(plan.arguments.iter().map(OsString::from)).in_directory(&root);
        let process_result = self
            .supervisor
            .run(&self.ffmpeg, &invocation, cancellation.child_token())
            .await;
        if let Err(error) = process_result {
            let _ = fs::remove_file(&temporary_path);
            self.remove_generated(&root, &text_paths);
            return Err(error.into());
        }
        if let Err(error) = verify_output(&temporary_path) {
            let _ = fs::remove_file(&temporary_path);
            self.remove_generated(&root, &text_paths);
            return Err(error);
        }
        if let Err(error) = fs::rename(&temporary_path, &render_path) {
            let _ = fs::remove_file(&temporary_path);
            self.remove_generated(&root, &text_paths);
            return Err(error.into());
        }
        self.remove_generated(&root, &text_paths);

        let output_metadata = match self
            .ffprobe
            .probe(&render_path, cancellation.child_token())
            .await
        {
            Ok(metadata) => metadata,
            Err(error) => {
                let _ = fs::remove_file(&render_path);
                return Err(error.into());
            }
        };
        let report = RenderQualityReport {
            duration_ms: output_metadata.duration_ms,
            expected_duration_ms: plan.expected_duration_ms,
            width: output_metadata.width,
            height: output_metadata.height,
            has_video: !output_metadata.video_codec.is_empty(),
            has_audio: output_metadata.audio_codec.is_some(),
            subtitle_mode: request.config.subtitle_mode,
            plan_hash: plan.identity.clone(),
        };
        if !report.passes()
            || output_metadata.width != plan.output_width
            || output_metadata.height != plan.output_height
        {
            let _ = fs::remove_file(&render_path);
            return Err(ComposerError::QualityControl);
        }
        let render = register_or_remove(
            &self.artifacts,
            request.project_id,
            ArtifactKind::Render,
            &render_relative,
            StageName::Render,
            &json_map(json!({"planHash": plan.identity, "preset": request.config.preview_preset})),
            &render_path,
        )?;
        let quality_artifact =
            match self.write_quality_report(request.project_id, render.id, &report) {
                Ok(artifact) => artifact,
                Err(error) => {
                    let _ = self.artifacts.unregister(render.id);
                    let _ = fs::remove_file(&render_path);
                    return Err(error);
                }
            };
        Ok(ComposerExecutionResult {
            render,
            subtitle,
            quality_report: report,
            quality_artifact,
        })
    }

    pub async fn execute_claimed(
        &self,
        queue: &PersistentQueue,
        claimed: ClaimedJob,
        request: &ComposerExecutionRequest,
    ) -> Result<Option<ComposerExecutionResult>, CoreError> {
        let outcome = self.execute(request, claimed.cancellation.clone()).await;
        if claimed.cancellation.is_cancelled() {
            if let Ok(result) = &outcome {
                self.remove_result(result);
            }
            queue.acknowledge_interruption(claimed.job.id)?;
            return Ok(None);
        }
        match outcome {
            Ok(result) => {
                queue.complete(
                    claimed.job.id,
                    &[result.render.id, result.quality_artifact.id],
                )?;
                Ok(Some(result))
            }
            Err(_) => {
                queue.fail(
                    claimed.job.id,
                    "COMPOSER_FAILED",
                    "Không thể hoàn tất kết xuất video.",
                )?;
                Ok(None)
            }
        }
    }

    fn verified_artifact(
        &self,
        project_id: Uuid,
        artifact_id: Uuid,
        kind: ArtifactKind,
    ) -> Result<(Artifact, PathBuf), ComposerError> {
        let artifact = self.artifacts.get(artifact_id)?;
        if artifact.project_id != project_id || artifact.kind != kind {
            return Err(CoreError::InvalidInput("composer artifact").into());
        }
        if self.artifacts.verify(artifact_id)? != ArtifactVerification::Verified {
            return Err(CoreError::ArtifactIntegrity.into());
        }
        let relative = ProjectRelativePath::parse(&artifact.relative_path)?;
        Ok((
            artifact,
            self.layout.resolve_existing(project_id, &relative)?,
        ))
    }

    fn write_subtitle(
        &self,
        request: &ComposerExecutionRequest,
        segments: &[Segment],
        source_duration_ms: u64,
    ) -> Result<Artifact, ComposerError> {
        let id = Uuid::new_v4();
        let relative = ProjectRelativePath::parse(format!("subtitles/{id}.srt"))?;
        let path = self.layout.prepare_output(request.project_id, &relative)?;
        let body = build_srt(segments, &request.config, source_duration_ms);
        if body.is_empty() {
            return Err(CoreError::InvalidInput("subtitle cues").into());
        }
        write_new_file(&path, body.as_bytes())?;
        register_or_remove(
            &self.artifacts,
            request.project_id,
            ArtifactKind::Subtitle,
            &relative,
            StageName::ComposeVideo,
            &Map::new(),
            &path,
        )
    }

    fn write_text_overlays(
        &self,
        request: &ComposerExecutionRequest,
    ) -> Result<Vec<String>, ComposerError> {
        let mut paths = Vec::new();
        for overlay in &request.config.text_overlays {
            let relative =
                ProjectRelativePath::parse(format!("metadata/text-{}.txt", Uuid::new_v4()))?;
            let path = self.layout.prepare_output(request.project_id, &relative)?;
            if let Err(error) = write_new_file(&path, overlay.text.as_bytes()) {
                let root = self.layout.project_root(request.project_id)?;
                self.remove_generated(&root, &paths);
                return Err(error.into());
            }
            paths.push(relative.as_str().to_owned());
        }
        Ok(paths)
    }

    fn write_quality_report(
        &self,
        project_id: Uuid,
        render_id: Uuid,
        report: &RenderQualityReport,
    ) -> Result<Artifact, ComposerError> {
        let relative = ProjectRelativePath::parse(format!("metadata/render-qc-{render_id}.json"))?;
        let path = self.layout.prepare_output(project_id, &relative)?;
        let bytes = serde_json::to_vec_pretty(report)
            .map_err(|_| CoreError::InvalidInput("render quality report"))?;
        write_new_file(&path, &bytes)?;
        register_or_remove(
            &self.artifacts,
            project_id,
            ArtifactKind::Metadata,
            &relative,
            StageName::QualityCheck,
            &Map::new(),
            &path,
        )
    }

    fn remove_generated(&self, root: &Path, paths: &[String]) {
        for relative in paths {
            let _ =
                fs::remove_file(root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR)));
        }
    }

    fn remove_result(&self, result: &ComposerExecutionResult) {
        for artifact in [
            Some(&result.render),
            result.subtitle.as_ref(),
            Some(&result.quality_artifact),
        ]
        .into_iter()
        .flatten()
        {
            if let Ok(relative) = ProjectRelativePath::parse(&artifact.relative_path) {
                if let Ok(path) = self.layout.prepare_output(artifact.project_id, &relative) {
                    let _ = fs::remove_file(path);
                }
            }
            let _ = self.artifacts.unregister(artifact.id);
        }
    }
}

#[derive(Clone)]
pub struct ComposerExportService {
    layout: ProjectLayout,
    artifacts: ArtifactRegistry,
}

#[derive(Clone)]
pub struct ComposerAssetService {
    layout: ProjectLayout,
    artifacts: ArtifactRegistry,
    max_bytes: u64,
}

impl ComposerAssetService {
    pub fn new(
        layout: ProjectLayout,
        artifacts: ArtifactRegistry,
        max_bytes: u64,
    ) -> Result<Self, CoreError> {
        if max_bytes == 0 || max_bytes > 128 * 1024 * 1024 {
            return Err(CoreError::InvalidInput("overlay size limit"));
        }
        Ok(Self {
            layout,
            artifacts,
            max_bytes,
        })
    }

    pub fn import_overlay(
        &self,
        project_id: Uuid,
        source: &Path,
    ) -> Result<Artifact, ComposerError> {
        if !source.is_absolute() {
            return Err(CoreError::UnsafePath.into());
        }
        let source_metadata = fs::symlink_metadata(source)?;
        if source_metadata.file_type().is_symlink()
            || !source_metadata.is_file()
            || source_metadata.len() == 0
            || source_metadata.len() > self.max_bytes
        {
            return Err(CoreError::InvalidInput("overlay image").into());
        }
        let extension = source
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .ok_or(CoreError::InvalidInput("overlay image type"))?;
        if !matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp") {
            return Err(CoreError::InvalidInput("overlay image type").into());
        }
        let relative = ProjectRelativePath::parse(format!(
            "metadata/overlay-{}.{}",
            Uuid::new_v4(),
            extension
        ))?;
        let destination = self.layout.prepare_output(project_id, &relative)?;
        let input = fs::File::open(source)?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)?;
        let copied = std::io::copy(&mut input.take(self.max_bytes + 1), &mut output)?;
        if copied == 0 || copied > self.max_bytes {
            drop(output);
            let _ = fs::remove_file(&destination);
            return Err(CoreError::InvalidInput("overlay image size").into());
        }
        output.sync_all()?;
        register_or_remove(
            &self.artifacts,
            project_id,
            ArtifactKind::OverlayImage,
            &relative,
            StageName::ComposeVideo,
            &Map::new(),
            &destination,
        )
    }
}

impl ComposerExportService {
    pub fn new(layout: ProjectLayout, artifacts: ArtifactRegistry) -> Self {
        Self { layout, artifacts }
    }

    pub fn export(
        &self,
        project_id: Uuid,
        artifact_id: Uuid,
        destination: &Path,
    ) -> Result<PathBuf, ComposerError> {
        let artifact = self.artifacts.get(artifact_id)?;
        let expected_extension = match artifact.kind {
            ArtifactKind::Subtitle => "srt",
            ArtifactKind::MixedAudio => "wav",
            _ => return Err(CoreError::InvalidInput("export artifact kind").into()),
        };
        if artifact.project_id != project_id
            || destination
                .extension()
                .and_then(|value| value.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref()
                != Some(expected_extension)
            || !destination.is_absolute()
            || destination.exists()
        {
            return Err(CoreError::UnsafePath.into());
        }
        if self.artifacts.verify(artifact_id)? != ArtifactVerification::Verified {
            return Err(CoreError::ArtifactIntegrity.into());
        }
        let source = self.layout.resolve_existing(
            project_id,
            &ProjectRelativePath::parse(&artifact.relative_path)?,
        )?;
        let parent = destination
            .parent()
            .ok_or(CoreError::UnsafePath)?
            .canonicalize()?;
        if fs::symlink_metadata(&parent)?.file_type().is_symlink() || !parent.is_dir() {
            return Err(CoreError::UnsafePath.into());
        }
        let target = parent.join(destination.file_name().ok_or(CoreError::UnsafePath)?);
        let mut input = fs::File::open(source)?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        if let Err(error) = std::io::copy(&mut input, &mut output).and_then(|_| output.sync_all()) {
            drop(output);
            let _ = fs::remove_file(&target);
            return Err(error.into());
        }
        Ok(target)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_render_plan(
    config: &ComposerConfig,
    source: &MediaMetadata,
    source_path: &str,
    audio_path: &str,
    subtitle_path: Option<&str>,
    text_paths: &[String],
    image_paths: &[String],
    output_path: &str,
    identity: String,
) -> Result<RenderPlan, ComposerError> {
    config.validate_for_source(source)?;
    if identity.len() != 64
        || !identity
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(CoreError::InvalidInput("composer identity").into());
    }
    if text_paths.len() != config.text_overlays.len()
        || image_paths.len() != config.image_overlays.len()
    {
        return Err(CoreError::InvalidInput("composer plan inputs").into());
    }
    for path in [source_path, audio_path, output_path]
        .into_iter()
        .chain(subtitle_path)
        .chain(text_paths.iter().map(String::as_str))
        .chain(image_paths.iter().map(String::as_str))
    {
        ProjectRelativePath::parse(path)?;
    }
    let end = config.trim_end_ms.unwrap_or(source.duration_ms);
    let start_seconds = seconds(config.trim_start_ms);
    let end_seconds = seconds(end);
    let (width, height) = config.output_dimensions(source);
    let mut video = vec![
        format!("trim=start={start_seconds}:end={end_seconds}"),
        format!("setpts=(PTS-STARTPTS)/{}", decimal(config.speed)),
    ];
    if let Some(crop) = &config.crop {
        video.push(format!(
            "crop={}:{}:{}:{}",
            crop.width, crop.height, crop.x, crop.y
        ));
    }
    match config.flip {
        crate::domain::FlipMode::Horizontal => video.push("hflip".into()),
        crate::domain::FlipMode::Vertical => video.push("vflip".into()),
        crate::domain::FlipMode::Both => {
            video.push("hflip".into());
            video.push("vflip".into());
        }
        crate::domain::FlipMode::None => {}
    }
    if config.blur_radius > 0.0 {
        video.push(format!("gblur=sigma={}", decimal(config.blur_radius)));
    }
    video.push(format!(
        "scale={width}:{height}:force_original_aspect_ratio=decrease"
    ));
    video.push(format!(
        "pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:color={}",
        ffmpeg_color(&config.padding_color)
    ));
    for cover in &config.cover_regions {
        video.push(format!(
            "drawbox=x={}:y={}:w={}:h={}:color={}@{}:t=fill:enable='between(t,{},{})'",
            cover.region.x,
            cover.region.y,
            cover.region.width,
            cover.region.height,
            ffmpeg_color(&cover.color),
            decimal(cover.region.opacity),
            adjusted_seconds(cover.region.start_ms, config),
            adjusted_seconds(cover.region.end_ms, config)
        ));
    }
    for (overlay, path) in config.text_overlays.iter().zip(text_paths) {
        video.push(format!("drawtext=textfile='{path}':x={}:y={}:fontsize={}:fontcolor={}:enable='between(t,{},{})'",
            overlay.x, overlay.y, overlay.font_size, ffmpeg_color(&overlay.color),
            adjusted_seconds(overlay.start_ms, config), adjusted_seconds(overlay.end_ms, config)));
    }
    let mut graph = format!("[0:v]{}[base]", video.join(","));
    let mut last = "base".to_owned();
    for (index, blur) in config.blur_regions.iter().enumerate() {
        let main = format!("blurmain{index}");
        let crop = format!("blurcrop{index}");
        let layer = format!("blurlayer{index}");
        let next = format!("blurvideo{index}");
        graph.push_str(&format!(";[{last}]split=2[{main}][{crop}]"));
        graph.push_str(&format!(
            ";[{crop}]crop={}:{}:{}:{},gblur=sigma={}[{layer}]",
            blur.region.width,
            blur.region.height,
            blur.region.x,
            blur.region.y,
            decimal(blur.radius)
        ));
        graph.push_str(&format!(
            ";[{main}][{layer}]overlay={}:{}:enable='between(t,{},{})'[{next}]",
            blur.region.x,
            blur.region.y,
            adjusted_seconds(blur.region.start_ms, config),
            adjusted_seconds(blur.region.end_ms, config)
        ));
        last = next;
    }
    for (index, overlay) in config.image_overlays.iter().enumerate() {
        let input = index + 2;
        let layer = format!("layer{index}");
        let next = format!("video{index}");
        graph.push_str(&format!(
            ";[{input}:v]scale={}:{},format=rgba,colorchannelmixer=aa={}[{layer}]",
            overlay.region.width,
            overlay.region.height,
            decimal(overlay.region.opacity)
        ));
        graph.push_str(&format!(
            ";[{last}][{layer}]overlay={}:{}:enable='between(t,{},{})'[{next}]",
            overlay.region.x,
            overlay.region.y,
            adjusted_seconds(overlay.region.start_ms, config),
            adjusted_seconds(overlay.region.end_ms, config)
        ));
        last = next;
    }
    if config.subtitle_mode == SubtitleMode::Burned {
        let subtitle = subtitle_path.ok_or(CoreError::InvalidInput("burned subtitle"))?;
        graph.push_str(&format!(
            ";[{last}]subtitles=filename='{subtitle}':force_style='FontName=Arial,FontSize=14,PrimaryColour=&H00FFFFFF,OutlineColour=&H00000000,BorderStyle=1,Outline=1,Shadow=0,Alignment=2,MarginV=12'[subtitled]"
        ));
        last = "subtitled".into();
    }
    graph.push_str(&format!(";[{last}]null[vout]"));
    graph.push_str(&format!(
        ";[1:a]atrim=start={start_seconds}:end={end_seconds},asetpts=PTS-STARTPTS"
    ));
    for factor in atempo_factors(config.speed) {
        graph.push_str(&format!(",atempo={}", decimal(factor)));
    }
    graph.push_str("[aout]");

    let mut arguments = vec![
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-n",
        "-i",
        source_path,
        "-i",
        audio_path,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    for path in image_paths {
        arguments.extend(["-i".into(), path.clone()]);
    }
    let subtitle_input = if config.subtitle_mode == SubtitleMode::Soft {
        let path = subtitle_path.ok_or(CoreError::InvalidInput("soft subtitle"))?;
        arguments.extend(["-i".into(), path.into()]);
        Some(2 + image_paths.len())
    } else {
        None
    };
    arguments.extend([
        "-filter_complex".into(),
        graph.clone(),
        "-map".into(),
        "[vout]".into(),
        "-map".into(),
        "[aout]".into(),
    ]);
    if let Some(index) = subtitle_input {
        arguments.extend(["-map".into(), format!("{index}:s:0")]);
    }
    let bitrate = match config.preview_preset {
        PreviewPreset::Draft => "4M",
        PreviewPreset::Final => "8M",
    };
    arguments.extend(
        [
            "-c:v",
            "libopenh264",
            "-b:v",
            bitrate,
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-movflags",
            "+faststart",
        ]
        .into_iter()
        .map(str::to_owned),
    );
    if subtitle_input.is_some() {
        arguments.extend(["-c:s".into(), "mov_text".into()]);
    }
    arguments.push(output_path.to_owned());
    Ok(RenderPlan {
        arguments,
        filter_graph: graph,
        output_width: width,
        output_height: height,
        expected_duration_ms: config.expected_duration_ms(source.duration_ms),
        identity,
    })
}

pub fn build_srt(segments: &[Segment], config: &ComposerConfig, source_duration_ms: u64) -> String {
    let end = config.trim_end_ms.unwrap_or(source_duration_ms);
    let mut cues = Vec::new();
    for segment in segments
        .iter()
        .filter(|value| value.enabled && !value.translated_text.trim().is_empty())
    {
        let cue_start = segment.start_ms.max(config.trim_start_ms);
        let cue_end = segment.end_ms.min(end);
        if cue_end <= cue_start {
            continue;
        }
        let start =
            ((cue_start - config.trim_start_ms) as f64 / config.speed as f64).round() as u64;
        let finish = ((cue_end - config.trim_start_ms) as f64 / config.speed as f64).round() as u64;
        let text = segment
            .translated_text
            .replace('\r', "")
            .replace("-->", "→");
        cues.push(format!(
            "{}\n{} --> {}\n{}\n",
            cues.len() + 1,
            srt_time(start),
            srt_time(finish),
            text
        ));
    }
    if cues.is_empty() {
        String::new()
    } else {
        format!("{}\n", cues.join("\n"))
    }
}

fn render_identity(
    config: &ComposerConfig,
    source: &Artifact,
    audio: &Artifact,
    images: &[Artifact],
    segments: &[Segment],
) -> Result<String, ComposerError> {
    let value = json!({
        "schemaVersion": 1, "config": config, "source": source.sha256, "audio": audio.sha256,
        "images": images.iter().map(|value| &value.sha256).collect::<Vec<_>>(),
        "subtitles": segments.iter().filter(|value| value.enabled).map(|value| json!([value.id, value.start_ms, value.end_ms, value.translation_hash])).collect::<Vec<_>>()
    });
    let bytes =
        serde_json::to_vec(&value).map_err(|_| CoreError::InvalidInput("composer identity"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn write_new_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn register_or_remove(
    registry: &ArtifactRegistry,
    project_id: Uuid,
    kind: ArtifactKind,
    relative: &ProjectRelativePath,
    stage: StageName,
    metadata: &Map<String, Value>,
    path: &Path,
) -> Result<Artifact, ComposerError> {
    match registry.register_existing(project_id, kind, relative.as_str(), stage, metadata) {
        Ok(value) => Ok(value),
        Err(error) => {
            let _ = fs::remove_file(path);
            Err(error.into())
        }
    }
}

fn verify_output(path: &Path) -> Result<(), ComposerError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ComposerError::MissingOutput)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
        return Err(ComposerError::MissingOutput);
    }
    Ok(())
}

fn seconds(value: u64) -> String {
    format!("{:.3}", value as f64 / 1000.0)
}
fn adjusted_seconds(value: u64, config: &ComposerConfig) -> String {
    seconds(
        ((value.saturating_sub(config.trim_start_ms)) as f64 / config.speed as f64).round() as u64,
    )
}
fn decimal(value: f32) -> String {
    format!("{value:.3}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}
fn ffmpeg_color(value: &str) -> String {
    format!("0x{}", &value[1..])
}
fn atempo_factors(speed: f32) -> Vec<f32> {
    if speed < 0.5 {
        vec![0.5, speed / 0.5]
    } else if speed > 2.0 {
        vec![2.0, speed / 2.0]
    } else {
        vec![speed]
    }
}
fn srt_time(value: u64) -> String {
    let hours = value / 3_600_000;
    let minutes = value / 60_000 % 60;
    let seconds = value / 1_000 % 60;
    let millis = value % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}
fn json_map(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}
