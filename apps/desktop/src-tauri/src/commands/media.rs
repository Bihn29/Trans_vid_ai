use std::{collections::BTreeMap, path::PathBuf};

use serde::Serialize;
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    domain::{
        Artifact, ArtifactKind, ArtifactVerification, CoreError, MediaMetadata, ProjectStatus,
        ProjectUpdate,
    },
    media::MediaToolError,
    state::AppState,
};

use super::projects::CommandError;

const SOURCE_CONFIG_KEY: &str = "source";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceMediaResponse {
    pub project_id: Uuid,
    pub artifact: Artifact,
    pub probe_artifact_id: Uuid,
    pub original_name: String,
    pub absolute_path: String,
    pub metadata: MediaMetadata,
    pub import_status: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderMediaResponse {
    pub artifact: Artifact,
    pub absolute_path: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceStateEvent {
    project_id: Uuid,
    status: &'static str,
    artifact_id: Option<Uuid>,
    error_code: Option<&'static str>,
}

#[tauri::command]
pub async fn import_local_media(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: Uuid,
    source_path: String,
) -> Result<SourceMediaResponse, CommandError> {
    emit_source_state(&app, project_id, "importing", None, None);
    state
        .privacy
        .write_event(
            "MEDIA_IMPORT_REQUESTED",
            &BTreeMap::from([("projectId".into(), project_id.to_string())]),
        )
        .map_err(CommandError::from)?;

    let source = PathBuf::from(&source_path);
    let original_name = source
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
        .ok_or(CoreError::InvalidInput("source filename"))?;
    let artifact = state
        .media_import
        .import_local(project_id, &source)
        .map_err(CommandError::from)?;
    emit_source_state(&app, project_id, "probing", Some(artifact.id), None);
    state
        .privacy
        .write_event(
            "MEDIA_SOURCE_IMPORTED",
            &BTreeMap::from([
                ("projectId".into(), project_id.to_string()),
                ("artifactId".into(), artifact.id.to_string()),
                ("relativePath".into(), artifact.relative_path.clone()),
            ]),
        )
        .map_err(CommandError::from)?;

    let Some(media_tools) = state.media_tools.clone() else {
        persist_source_state(
            &state,
            project_id,
            &artifact,
            &original_name,
            "failed",
            None,
            None,
            Some("MEDIA_TOOLS_UNAVAILABLE"),
        )?;
        emit_source_state(
            &app,
            project_id,
            "failed",
            Some(artifact.id),
            Some("MEDIA_TOOLS_UNAVAILABLE"),
        );
        return Err(CoreError::MediaToolsUnavailable.into());
    };

    state
        .privacy
        .write_event(
            "FFPROBE_STARTED",
            &BTreeMap::from([
                ("projectId".into(), project_id.to_string()),
                ("artifactId".into(), artifact.id.to_string()),
            ]),
        )
        .map_err(CommandError::from)?;
    let probe_result = media_tools
        .probe_source(project_id, artifact.id, CancellationToken::new())
        .await;
    let (metadata, probe_artifact) = match probe_result {
        Ok(result) => result,
        Err(error) => {
            persist_source_state(
                &state,
                project_id,
                &artifact,
                &original_name,
                "failed",
                None,
                None,
                Some(media_tool_error_code(&error)),
            )?;
            state
                .privacy
                .write_event(
                    "FFPROBE_FAILED",
                    &BTreeMap::from([
                        ("projectId".into(), project_id.to_string()),
                        ("artifactId".into(), artifact.id.to_string()),
                        ("errorCode".into(), media_tool_error_code(&error).into()),
                    ]),
                )
                .map_err(CommandError::from)?;
            emit_source_state(
                &app,
                project_id,
                "failed",
                Some(artifact.id),
                Some(media_tool_error_code(&error)),
            );
            return Err(CommandError {
                code: media_tool_error_code(&error),
            });
        }
    };
    persist_source_state(
        &state,
        project_id,
        &artifact,
        &original_name,
        "ready",
        Some(&metadata),
        Some(probe_artifact.id),
        None,
    )?;
    state
        .privacy
        .write_event(
            "FFPROBE_COMPLETED",
            &BTreeMap::from([
                ("projectId".into(), project_id.to_string()),
                ("artifactId".into(), artifact.id.to_string()),
                ("durationMs".into(), metadata.duration_ms.to_string()),
                (
                    "resolution".into(),
                    format!("{}x{}", metadata.width, metadata.height),
                ),
            ]),
        )
        .map_err(CommandError::from)?;
    emit_source_state(&app, project_id, "ready", Some(artifact.id), None);

    source_response(
        &state,
        project_id,
        artifact,
        probe_artifact.id,
        original_name,
        metadata,
    )
}

fn emit_source_state(
    app: &AppHandle,
    project_id: Uuid,
    status: &'static str,
    artifact_id: Option<Uuid>,
    error_code: Option<&'static str>,
) {
    let _ = app.emit(
        "source-state",
        SourceStateEvent {
            project_id,
            status,
            artifact_id,
            error_code,
        },
    );
}

#[tauri::command]
pub fn get_source_media(
    state: State<'_, AppState>,
    project_id: Uuid,
) -> Result<SourceMediaResponse, CommandError> {
    let project = state.projects.get(project_id)?;
    let artifact_id = project
        .source_asset_id
        .ok_or(CoreError::NotFound("source artifact"))?;
    let artifact = state.artifacts.get(artifact_id)?;
    if state.artifacts.verify(artifact_id)? != ArtifactVerification::Verified {
        return Err(CoreError::ArtifactIntegrity.into());
    }
    let source = project
        .config_snapshot
        .get(SOURCE_CONFIG_KEY)
        .and_then(Value::as_object)
        .ok_or(CoreError::InvalidInput("source metadata"))?;
    if source.get("import_status").and_then(Value::as_str) != Some("ready") {
        return Err(CoreError::InvalidTransition.into());
    }
    let metadata = serde_json::from_value(
        source
            .get("metadata")
            .cloned()
            .ok_or(CoreError::InvalidInput("source metadata"))?,
    )
    .map_err(|_| CoreError::InvalidInput("source metadata"))?;
    let probe_artifact_id = source
        .get("probe_artifact_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or(CoreError::InvalidInput("probe artifact"))?;
    let original_name = source
        .get("original_name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(CoreError::InvalidInput("source filename"))?;
    source_response(
        &state,
        project_id,
        artifact,
        probe_artifact_id,
        original_name,
        metadata,
    )
}

#[tauri::command]
pub fn get_latest_render_media(
    state: State<'_, AppState>,
    project_id: Uuid,
) -> Result<RenderMediaResponse, CommandError> {
    let artifact = state
        .artifacts
        .list_for_project(project_id)?
        .into_iter()
        .rev()
        .find(|artifact| artifact.kind == ArtifactKind::Render)
        .ok_or(CoreError::NotFound("render artifact"))?;
    if state.artifacts.verify(artifact.id)? != ArtifactVerification::Verified {
        return Err(CoreError::ArtifactIntegrity.into());
    }
    let relative = crate::infrastructure::ProjectRelativePath::parse(&artifact.relative_path)?;
    let absolute_path = state
        .projects
        .layout()
        .resolve_existing(project_id, &relative)?
        .to_str()
        .map(str::to_owned)
        .ok_or(CoreError::InvalidInput("render media path"))?;
    Ok(RenderMediaResponse {
        artifact,
        absolute_path,
    })
}

#[tauri::command]
pub fn log_preview_event(
    state: State<'_, AppState>,
    project_id: Uuid,
    artifact_id: Uuid,
    event: String,
    protocol: String,
    media_error_code: Option<u16>,
) -> Result<(), CommandError> {
    if !matches!(
        event.as_str(),
        "url_generated" | "metadata_loaded" | "can_play" | "play" | "error"
    ) || !matches!(protocol.as_str(), "asset:" | "http:" | "https:")
        || media_error_code.is_some_and(|code| code > 4)
    {
        return Err(CoreError::InvalidInput("preview event").into());
    }
    state.privacy.write_event(
        "VIDEO_PREVIEW_EVENT",
        &BTreeMap::from([
            ("projectId".into(), project_id.to_string()),
            ("artifactId".into(), artifact_id.to_string()),
            ("event".into(), event),
            ("protocol".into(), protocol),
            (
                "mediaErrorCode".into(),
                media_error_code.map_or_else(|| "none".into(), |code| code.to_string()),
            ),
        ]),
    )?;
    Ok(())
}

fn source_response(
    state: &AppState,
    project_id: Uuid,
    artifact: Artifact,
    probe_artifact_id: Uuid,
    original_name: String,
    metadata: MediaMetadata,
) -> Result<SourceMediaResponse, CommandError> {
    let relative = crate::infrastructure::ProjectRelativePath::parse(&artifact.relative_path)?;
    let absolute_path = state
        .projects
        .layout()
        .resolve_existing(project_id, &relative)?
        .to_str()
        .map(str::to_owned)
        .ok_or(CoreError::InvalidInput("project media path"))?;
    Ok(SourceMediaResponse {
        project_id,
        artifact,
        probe_artifact_id,
        original_name,
        absolute_path,
        metadata,
        import_status: "ready",
    })
}

#[allow(clippy::too_many_arguments)]
fn persist_source_state(
    state: &AppState,
    project_id: Uuid,
    artifact: &Artifact,
    original_name: &str,
    import_status: &str,
    metadata: Option<&MediaMetadata>,
    probe_artifact_id: Option<Uuid>,
    error_code: Option<&str>,
) -> Result<(), CommandError> {
    let project = state.projects.get(project_id)?;
    let mut config = project.config_snapshot;
    let mut source = Map::new();
    source.insert("schema_version".into(), Value::from(1));
    source.insert("artifact_id".into(), Value::String(artifact.id.to_string()));
    source.insert(
        "relative_path".into(),
        Value::String(artifact.relative_path.clone()),
    );
    source.insert("original_name".into(), Value::String(original_name.into()));
    source.insert("import_status".into(), Value::String(import_status.into()));
    source.insert("size_bytes".into(), Value::from(artifact.size_bytes));
    if let Some(metadata) = metadata {
        source.insert("metadata".into(), json!(metadata));
    }
    if let Some(probe_artifact_id) = probe_artifact_id {
        source.insert(
            "probe_artifact_id".into(),
            Value::String(probe_artifact_id.to_string()),
        );
    }
    if let Some(error_code) = error_code {
        source.insert("error_code".into(), Value::String(error_code.into()));
    }
    config.insert(SOURCE_CONFIG_KEY.into(), Value::Object(source));
    state.projects.update(
        project_id,
        &ProjectUpdate {
            status: Some(if import_status == "ready" {
                ProjectStatus::Active
            } else {
                ProjectStatus::Failed
            }),
            config_snapshot: Some(config),
            ..ProjectUpdate::default()
        },
    )?;
    Ok(())
}

fn media_tool_error_code(error: &MediaToolError) -> &'static str {
    match error {
        MediaToolError::InvalidMetadata => "FFPROBE_INVALID_MEDIA",
        MediaToolError::MissingOutput => "MEDIA_TOOL_MISSING_OUTPUT",
        MediaToolError::Tool(_) => "FFPROBE_FAILED",
        MediaToolError::Io(_) => "FILESYSTEM_ERROR",
        MediaToolError::Core(error) => error.code(),
    }
}
