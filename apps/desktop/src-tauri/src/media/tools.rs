use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use serde_json::{json, Map, Value};
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    domain::{Artifact, ArtifactKind, ArtifactVerification, CoreError, MediaMetadata, StageName},
    infrastructure::{ArtifactRegistry, ProjectLayout, ProjectRelativePath},
    processes::{ApprovedTool, SupervisedProcess, ToolError, ToolInvocation},
};

#[derive(Debug, Error)]
pub enum MediaToolError {
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error("media filesystem operation failed")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Tool(#[from] ToolError),
    #[error("media metadata is invalid")]
    InvalidMetadata,
    #[error("media tool did not produce its required output")]
    MissingOutput,
}

#[derive(Debug, Clone)]
pub struct FfprobeAdapter {
    tool: ApprovedTool,
    supervisor: SupervisedProcess,
}

impl FfprobeAdapter {
    pub fn new(tool: ApprovedTool, supervisor: SupervisedProcess) -> Self {
        Self { tool, supervisor }
    }

    pub async fn probe(
        &self,
        input: &Path,
        cancellation: CancellationToken,
    ) -> Result<MediaMetadata, MediaToolError> {
        let input = validate_existing_file(input)?;
        let invocation = ToolInvocation::new([
            OsString::from("-v"),
            OsString::from("error"),
            OsString::from("-print_format"),
            OsString::from("json"),
            OsString::from("-show_format"),
            OsString::from("-show_streams"),
            OsString::from("--"),
            input.into_os_string(),
        ]);
        let output = self
            .supervisor
            .run(&self.tool, &invocation, cancellation)
            .await?;
        parse_probe_output(&output.stdout)
    }
}

#[derive(Debug, Clone)]
pub struct FfmpegAdapter {
    tool: ApprovedTool,
    supervisor: SupervisedProcess,
}

impl FfmpegAdapter {
    pub fn new(tool: ApprovedTool, supervisor: SupervisedProcess) -> Self {
        Self { tool, supervisor }
    }

    pub async fn create_proxy(
        &self,
        input: &Path,
        output: &Path,
        cancellation: CancellationToken,
    ) -> Result<(), MediaToolError> {
        let input = validate_existing_file(input)?;
        let output = validate_new_output(output)?;
        let invocation = ToolInvocation::new([
            OsString::from("-nostdin"),
            OsString::from("-hide_banner"),
            OsString::from("-loglevel"),
            OsString::from("error"),
            OsString::from("-n"),
            OsString::from("-i"),
            input.into_os_string(),
            OsString::from("-map"),
            OsString::from("0:v:0"),
            OsString::from("-map"),
            OsString::from("0:a?"),
            OsString::from("-vf"),
            OsString::from("scale=w=min(1280\\,iw):h=-2"),
            OsString::from("-c:v"),
            OsString::from("libx264"),
            OsString::from("-preset"),
            OsString::from("veryfast"),
            OsString::from("-crf"),
            OsString::from("28"),
            OsString::from("-c:a"),
            OsString::from("aac"),
            OsString::from("-b:a"),
            OsString::from("128k"),
            OsString::from("-movflags"),
            OsString::from("+faststart"),
            output.clone().into_os_string(),
        ]);
        self.supervisor
            .run(&self.tool, &invocation, cancellation)
            .await?;
        verify_new_output(&output)
    }

    pub async fn extract_normalized_audio(
        &self,
        input: &Path,
        output: &Path,
        cancellation: CancellationToken,
    ) -> Result<(), MediaToolError> {
        let input = validate_existing_file(input)?;
        let output = validate_new_output(output)?;
        let invocation = ToolInvocation::new([
            OsString::from("-nostdin"),
            OsString::from("-hide_banner"),
            OsString::from("-loglevel"),
            OsString::from("error"),
            OsString::from("-n"),
            OsString::from("-i"),
            input.into_os_string(),
            OsString::from("-map"),
            OsString::from("0:a:0"),
            OsString::from("-vn"),
            OsString::from("-ac"),
            OsString::from("1"),
            OsString::from("-ar"),
            OsString::from("16000"),
            OsString::from("-c:a"),
            OsString::from("pcm_s16le"),
            OsString::from("-f"),
            OsString::from("wav"),
            output.clone().into_os_string(),
        ]);
        self.supervisor
            .run(&self.tool, &invocation, cancellation)
            .await?;
        verify_new_output(&output)
    }
}

#[derive(Clone)]
pub struct MediaToolService {
    layout: ProjectLayout,
    artifacts: ArtifactRegistry,
    ffprobe: FfprobeAdapter,
    ffmpeg: FfmpegAdapter,
}

impl MediaToolService {
    pub fn new(
        layout: ProjectLayout,
        artifacts: ArtifactRegistry,
        ffprobe: FfprobeAdapter,
        ffmpeg: FfmpegAdapter,
    ) -> Self {
        Self {
            layout,
            artifacts,
            ffprobe,
            ffmpeg,
        }
    }

    pub async fn probe_source(
        &self,
        project_id: Uuid,
        source_artifact_id: Uuid,
        cancellation: CancellationToken,
    ) -> Result<(MediaMetadata, Artifact), MediaToolError> {
        let (_, source) = self.verified_source(project_id, source_artifact_id)?;
        let metadata = self.ffprobe.probe(&source, cancellation).await?;
        let relative = ProjectRelativePath::parse(format!(
            "metadata/probe-{source_artifact_id}-{}.json",
            Uuid::new_v4()
        ))?;
        let destination = self.layout.prepare_output(project_id, &relative)?;
        let temporary = destination.with_extension(format!("json.{}.partial", Uuid::new_v4()));
        let encoded = serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "source_artifact_id": source_artifact_id,
            "metadata": metadata,
        }))
        .map_err(|_| MediaToolError::InvalidMetadata)?;
        let write_result =
            fs::write(&temporary, encoded).and_then(|_| fs::rename(&temporary, &destination));
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temporary);
            return Err(error.into());
        }
        let artifact = self.artifacts.register_existing(
            project_id,
            ArtifactKind::Metadata,
            relative.as_str(),
            StageName::Probe,
            &Map::new(),
        );
        let artifact = match artifact {
            Ok(artifact) => artifact,
            Err(error) => {
                let _ = fs::remove_file(&destination);
                return Err(error.into());
            }
        };
        Ok((metadata, artifact))
    }

    pub async fn create_proxy(
        &self,
        project_id: Uuid,
        source_artifact_id: Uuid,
        cancellation: CancellationToken,
    ) -> Result<Artifact, MediaToolError> {
        let (_, source) = self.verified_source(project_id, source_artifact_id)?;
        let id = Uuid::new_v4();
        let relative = ProjectRelativePath::parse(format!("proxy/{id}.mp4"))?;
        let destination = self.layout.prepare_output(project_id, &relative)?;
        let temporary = destination.with_file_name(format!(".{id}.partial.mp4"));
        let result = self
            .ffmpeg
            .create_proxy(&source, &temporary, cancellation)
            .await
            .and_then(|_| fs::rename(&temporary, &destination).map_err(MediaToolError::from));
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        match self.artifacts.register_existing(
            project_id,
            ArtifactKind::ProxyVideo,
            relative.as_str(),
            StageName::Normalize,
            &Map::new(),
        ) {
            Ok(artifact) => Ok(artifact),
            Err(error) => {
                let _ = fs::remove_file(&destination);
                Err(error.into())
            }
        }
    }

    pub async fn extract_normalized_audio(
        &self,
        project_id: Uuid,
        source_artifact_id: Uuid,
        cancellation: CancellationToken,
    ) -> Result<Artifact, MediaToolError> {
        let (_, source) = self.verified_source(project_id, source_artifact_id)?;
        let id = Uuid::new_v4();
        let relative = ProjectRelativePath::parse(format!("audio/original/{id}.wav"))?;
        let destination = self.layout.prepare_output(project_id, &relative)?;
        let temporary = destination.with_file_name(format!(".{id}.partial.wav"));
        let result = self
            .ffmpeg
            .extract_normalized_audio(&source, &temporary, cancellation)
            .await
            .and_then(|_| fs::rename(&temporary, &destination).map_err(MediaToolError::from));
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        match self.artifacts.register_existing(
            project_id,
            ArtifactKind::OriginalAudio,
            relative.as_str(),
            StageName::ExtractAudio,
            &Map::new(),
        ) {
            Ok(artifact) => Ok(artifact),
            Err(error) => {
                let _ = fs::remove_file(&destination);
                Err(error.into())
            }
        }
    }

    fn verified_source(
        &self,
        project_id: Uuid,
        artifact_id: Uuid,
    ) -> Result<(Artifact, PathBuf), MediaToolError> {
        let artifact = self.artifacts.get(artifact_id)?;
        if artifact.project_id != project_id || artifact.kind != ArtifactKind::SourceVideo {
            return Err(CoreError::InvalidInput("source artifact").into());
        }
        if self.artifacts.verify(artifact_id)? != ArtifactVerification::Verified {
            return Err(CoreError::ArtifactIntegrity.into());
        }
        let relative = ProjectRelativePath::parse(&artifact.relative_path)?;
        let path = self.layout.resolve_existing(project_id, &relative)?;
        Ok((artifact, path))
    }
}

fn validate_existing_file(path: &Path) -> Result<PathBuf, MediaToolError> {
    if !path.is_absolute() {
        return Err(CoreError::UnsafePath.into());
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CoreError::UnsafePath.into());
    }
    Ok(path.canonicalize()?)
}

fn validate_new_output(path: &Path) -> Result<PathBuf, MediaToolError> {
    if !path.is_absolute() || path.exists() {
        return Err(CoreError::UnsafePath.into());
    }
    let parent = path.parent().ok_or(CoreError::UnsafePath)?.canonicalize()?;
    if !parent.is_dir() {
        return Err(CoreError::UnsafePath.into());
    }
    let name = path.file_name().ok_or(CoreError::UnsafePath)?;
    Ok(parent.join(name))
}

fn verify_new_output(path: &Path) -> Result<(), MediaToolError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| MediaToolError::MissingOutput)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
        return Err(MediaToolError::MissingOutput);
    }
    Ok(())
}

fn parse_probe_output(output: &[u8]) -> Result<MediaMetadata, MediaToolError> {
    let value: Value =
        serde_json::from_slice(output).map_err(|_| MediaToolError::InvalidMetadata)?;
    let streams = value
        .get("streams")
        .and_then(Value::as_array)
        .ok_or(MediaToolError::InvalidMetadata)?;
    let video = streams
        .iter()
        .find(|stream| stream.get("codec_type").and_then(Value::as_str) == Some("video"))
        .ok_or(MediaToolError::InvalidMetadata)?;
    let audio = streams
        .iter()
        .find(|stream| stream.get("codec_type").and_then(Value::as_str) == Some("audio"));
    let duration_seconds = value
        .pointer("/format/duration")
        .and_then(Value::as_str)
        .and_then(|duration| duration.parse::<f64>().ok())
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .ok_or(MediaToolError::InvalidMetadata)?;
    let metadata = MediaMetadata {
        duration_ms: (duration_seconds * 1000.0).round() as u64,
        width: u32::try_from(video.get("width").and_then(Value::as_u64).unwrap_or(0))
            .map_err(|_| MediaToolError::InvalidMetadata)?,
        height: u32::try_from(video.get("height").and_then(Value::as_u64).unwrap_or(0))
            .map_err(|_| MediaToolError::InvalidMetadata)?,
        frame_rate: parse_frame_rate(
            video
                .get("avg_frame_rate")
                .and_then(Value::as_str)
                .ok_or(MediaToolError::InvalidMetadata)?,
        )?,
        video_codec: video
            .get("codec_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        audio_codec: audio
            .and_then(|stream| stream.get("codec_name"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        container: value
            .pointer("/format/format_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        rotation_degrees: video
            .get("side_data_list")
            .and_then(Value::as_array)
            .and_then(|entries| {
                entries
                    .iter()
                    .find_map(|entry| entry.get("rotation").and_then(Value::as_i64))
            })
            .or_else(|| {
                video
                    .pointer("/tags/rotate")
                    .and_then(Value::as_str)
                    .and_then(|rotation| rotation.parse::<i64>().ok())
            })
            .map(i32::try_from)
            .transpose()
            .map_err(|_| MediaToolError::InvalidMetadata)?
            .unwrap_or(0),
    };
    if !metadata.validate() {
        return Err(MediaToolError::InvalidMetadata);
    }
    Ok(metadata)
}

fn parse_frame_rate(value: &str) -> Result<f64, MediaToolError> {
    let (numerator, denominator) = value
        .split_once('/')
        .ok_or(MediaToolError::InvalidMetadata)?;
    let numerator = numerator
        .parse::<f64>()
        .map_err(|_| MediaToolError::InvalidMetadata)?;
    let denominator = denominator
        .parse::<f64>()
        .map_err(|_| MediaToolError::InvalidMetadata)?;
    if denominator <= 0.0 {
        return Err(MediaToolError::InvalidMetadata);
    }
    Ok(numerator / denominator)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bounded_ffprobe_metadata() {
        let metadata = parse_probe_output(
            br#"{"streams":[{"codec_type":"video","codec_name":"h264","width":1920,"height":1080,"avg_frame_rate":"30000/1001"},{"codec_type":"audio","codec_name":"aac"}],"format":{"duration":"12.345","format_name":"mov,mp4"}}"#,
        )
        .expect("metadata");
        assert_eq!(metadata.duration_ms, 12_345);
        assert_eq!(metadata.audio_codec.as_deref(), Some("aac"));
        assert!((metadata.frame_rate - 29.970).abs() < 0.001);
    }

    #[test]
    fn rejects_incomplete_or_extreme_ffprobe_metadata() {
        for fixture in [
            br#"{}"#.as_slice(),
            br#"{"streams":[],"format":{"duration":"1"}}"#.as_slice(),
            br#"{"streams":[{"codec_type":"video","codec_name":"h264","width":99999,"height":1080,"avg_frame_rate":"30/1"}],"format":{"duration":"1","format_name":"mp4"}}"#.as_slice(),
        ] {
            assert!(parse_probe_output(fixture).is_err());
        }
    }
}
