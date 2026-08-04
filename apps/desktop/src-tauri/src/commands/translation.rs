use serde::Deserialize;
use tauri::State;
use uuid::Uuid;

use crate::{
    domain::{
        CoreError, GlossaryEntry, LockedProperName, TranslationBlock, TranslationProviderDisclosure,
    },
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct GlossaryPayload {
    pub source_text: String,
    pub target_text: String,
    pub case_sensitive: bool,
}

fn parse_project_id(value: &str) -> Result<Uuid, String> {
    Uuid::parse_str(value).map_err(|_| "invalid project_id".to_string())
}

fn map_error(error: CoreError) -> String {
    format!("{}: {}", error.code(), error)
}

#[tauri::command]
pub fn list_translation_providers() -> Vec<TranslationProviderDisclosure> {
    vec![
        TranslationProviderDisclosure {
            provider_id: "local".into(),
            display_name: "Bộ máy cục bộ".into(),
            sends_data_off_device: false,
        },
        TranslationProviderDisclosure {
            provider_id: "openai-compatible".into(),
            display_name: "OpenAI-compatible".into(),
            sends_data_off_device: true,
        },
    ]
}

#[tauri::command]
pub fn list_translation_blocks(
    stage_run_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<TranslationBlock>, String> {
    let id = Uuid::parse_str(&stage_run_id).map_err(|_| "invalid stage_run_id".to_string())?;
    state.translations.list_for_stage(id).map_err(map_error)
}

#[tauri::command]
pub fn list_glossary(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<GlossaryEntry>, String> {
    state
        .translations
        .list_glossary(parse_project_id(&project_id)?)
        .map_err(map_error)
}

#[tauri::command]
pub fn upsert_glossary(
    project_id: String,
    payload: GlossaryPayload,
    state: State<'_, AppState>,
) -> Result<GlossaryEntry, String> {
    state
        .translations
        .upsert_glossary(
            parse_project_id(&project_id)?,
            &payload.source_text,
            &payload.target_text,
            payload.case_sensitive,
        )
        .map_err(map_error)
}

#[tauri::command]
pub fn list_locked_names(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<LockedProperName>, String> {
    state
        .translations
        .list_locked_names(parse_project_id(&project_id)?)
        .map_err(map_error)
}

#[tauri::command]
pub fn add_locked_name(
    project_id: String,
    value: String,
    state: State<'_, AppState>,
) -> Result<LockedProperName, String> {
    state
        .translations
        .add_locked_name(parse_project_id(&project_id)?, &value)
        .map_err(map_error)
}

#[tauri::command]
pub fn approve_translation(project_id: String, state: State<'_, AppState>) -> Result<(), String> {
    state
        .queue
        .complete_translation_review(parse_project_id(&project_id)?)
        .map_err(map_error)
}
