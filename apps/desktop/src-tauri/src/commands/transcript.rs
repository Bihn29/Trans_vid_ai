use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::domain::{CoreError, Segment, SegmentUpdate, SegmentWarning};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct UpdateSegmentPayload {
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
    pub source_text: Option<String>,
    pub translated_text: Option<String>,
    pub speaker_id: Option<String>,
    pub clear_speaker: Option<bool>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct SplitResult {
    pub first: Segment,
    pub second: Segment,
}

fn map_error(error: CoreError) -> String {
    format!("{}: {}", error.code(), error)
}

#[tauri::command]
pub fn get_transcript(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<Segment>, String> {
    let id = Uuid::parse_str(&project_id).map_err(|_| "invalid project_id".to_string())?;
    state.transcript.get_transcript(id).map_err(map_error)
}

#[tauri::command]
pub fn update_segment(
    project_id: String,
    segment_id: String,
    payload: UpdateSegmentPayload,
    state: State<'_, AppState>,
) -> Result<Segment, String> {
    let project_id = Uuid::parse_str(&project_id).map_err(|_| "invalid project_id".to_string())?;
    let seg_id = Uuid::parse_str(&segment_id).map_err(|_| "invalid segment_id".to_string())?;

    let speaker_id = match (payload.speaker_id, payload.clear_speaker.unwrap_or(false)) {
        (Some(_), true) => return Err("invalid speaker update".to_string()),
        (Some(value), false) => Some(Some(
            Uuid::parse_str(&value).map_err(|_| "invalid speaker_id".to_string())?,
        )),
        (None, true) => Some(None),
        (None, false) => None,
    };
    let update = SegmentUpdate {
        start_ms: payload.start_ms,
        end_ms: payload.end_ms,
        source_text: payload.source_text,
        translated_text: payload.translated_text,
        speaker_id,
        enabled: payload.enabled,
        ..Default::default()
    };

    state
        .transcript
        .update_segment(project_id, seg_id, &update)
        .map_err(map_error)
}

#[tauri::command]
pub fn split_segment(
    project_id: String,
    segment_id: String,
    split_ms: u64,
    state: State<'_, AppState>,
) -> Result<SplitResult, String> {
    let project_id = Uuid::parse_str(&project_id).map_err(|_| "invalid project_id".to_string())?;
    let seg_id = Uuid::parse_str(&segment_id).map_err(|_| "invalid segment_id".to_string())?;

    let (first, second) = state
        .transcript
        .split_segment(project_id, seg_id, split_ms)
        .map_err(map_error)?;

    Ok(SplitResult { first, second })
}

#[tauri::command]
pub fn merge_segments(
    project_id: String,
    segment_id_a: String,
    segment_id_b: String,
    state: State<'_, AppState>,
) -> Result<Segment, String> {
    let project_id = Uuid::parse_str(&project_id).map_err(|_| "invalid project_id".to_string())?;
    let id_a = Uuid::parse_str(&segment_id_a).map_err(|_| "invalid segment_id_a".to_string())?;
    let id_b = Uuid::parse_str(&segment_id_b).map_err(|_| "invalid segment_id_b".to_string())?;

    state
        .transcript
        .merge_segments(project_id, id_a, id_b)
        .map_err(map_error)
}

#[tauri::command]
pub fn approve_transcript(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<Segment>, String> {
    let id = Uuid::parse_str(&project_id).map_err(|_| "invalid project_id".to_string())?;
    let approved = state.transcript.approve_transcript(id).map_err(map_error)?;
    state.queue.complete_review(id).map_err(map_error)?;
    Ok(approved)
}

#[tauri::command]
pub fn check_transcript_quality(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<SegmentWarning>, String> {
    let id = Uuid::parse_str(&project_id).map_err(|_| "invalid project_id".to_string())?;
    state.transcript.check_quality(id).map_err(map_error)
}
