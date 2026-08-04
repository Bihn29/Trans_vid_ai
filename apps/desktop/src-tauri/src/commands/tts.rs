use crate::{
    domain::{Artifact, CoreError, VoiceAssignment, VoiceDescriptor, VoiceScope},
    infrastructure::TtsExecutionRequest,
    jobs::InvalidationChange,
    security::CredentialReference,
    state::AppState,
};
use serde::Deserialize;
use tauri::State;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn map(error: CoreError) -> String {
    format!("{}: {}", error.code(), error)
}
fn uuid(value: &str, name: &str) -> Result<Uuid, String> {
    Uuid::parse_str(value).map_err(|_| format!("invalid {name}"))
}

#[tauri::command]
pub fn list_voices() -> Vec<VoiceDescriptor> {
    vec![
        VoiceDescriptor {
            provider_id: "openai-compatible".into(),
            voice_id: "alloy".into(),
            display_name: "Alloy".into(),
            language: "multilingual".into(),
            sends_data_off_device: true,
            approved: true,
        },
        VoiceDescriptor {
            provider_id: "openai-compatible".into(),
            voice_id: "nova".into(),
            display_name: "Nova".into(),
            language: "multilingual".into(),
            sends_data_off_device: true,
            approved: true,
        },
    ]
}
#[tauri::command]
pub fn list_voice_assignments(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<VoiceAssignment>, String> {
    state
        .tts
        .list_assignments(uuid(&project_id, "project_id")?)
        .map_err(map)
}
#[tauri::command]
pub fn set_voice_assignment(
    project_id: String,
    scope_type: String,
    scope_id: Option<String>,
    provider_id: String,
    voice_id: String,
    state: State<'_, AppState>,
) -> Result<VoiceAssignment, String> {
    let project = uuid(&project_id, "project_id")?;
    let scope = match (scope_type.as_str(), scope_id) {
        ("project", None) => VoiceScope::Project,
        ("speaker", Some(v)) => VoiceScope::Speaker(uuid(&v, "scope_id")?),
        ("segment", Some(v)) => VoiceScope::Segment(uuid(&v, "scope_id")?),
        _ => return Err("invalid voice scope".into()),
    };
    if !matches!(voice_id.as_str(), "alloy" | "nova") || provider_id != "openai-compatible" {
        return Err("voice is not approved".into());
    }
    let segments = state.transcript.get_transcript(project).map_err(map)?;
    let (segment_ids, speaker_id) = match &scope {
        VoiceScope::Project => (segments.iter().map(|segment| segment.id).collect(), None),
        VoiceScope::Speaker(id) => {
            let affected = segments
                .iter()
                .filter(|segment| segment.speaker_id == Some(*id))
                .map(|segment| segment.id)
                .collect::<Vec<_>>();
            if affected.is_empty() {
                return Err("speaker does not belong to project".into());
            }
            (affected, Some(*id))
        }
        VoiceScope::Segment(id) => {
            if !segments.iter().any(|segment| segment.id == *id) {
                return Err("segment does not belong to project".into());
            }
            (vec![*id], None)
        }
    };
    state
        .invalidation
        .invalidate(
            project,
            &InvalidationChange::VoiceAssignment {
                segment_ids,
                speaker_id,
            },
        )
        .map_err(map)?;
    state
        .tts
        .set_assignment(project, scope, &provider_id, &voice_id)
        .map_err(map)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VoicePreviewRequest {
    project_id: String,
    segment_id: String,
    endpoint: String,
    model: String,
    account: String,
    speed: f64,
    cloud_consent: bool,
}

#[tauri::command]
pub async fn preview_voice(
    request: VoicePreviewRequest,
    state: State<'_, AppState>,
) -> Result<Artifact, String> {
    let execution = TtsExecutionRequest {
        provider: list_voices()
            .into_iter()
            .next()
            .ok_or_else(|| "no approved voice provider".to_owned())?,
        endpoint: request.endpoint,
        model: request.model,
        credential: CredentialReference::new("vietdub.tts", &request.account).map_err(map)?,
        cloud_consent: request.cloud_consent,
        speed: request.speed,
        max_attempts: 2,
    };
    state
        .tts_pipeline
        .preview(
            uuid(&request.project_id, "project_id")?,
            uuid(&request.segment_id, "segment_id")?,
            &execution,
            CancellationToken::new(),
        )
        .await
        .map_err(map)
}
