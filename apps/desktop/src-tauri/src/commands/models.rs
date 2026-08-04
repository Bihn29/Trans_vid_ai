use tauri::State;

use std::path::PathBuf;

use crate::domain::{ApprovedModelManifest, CoreError, ModelConsent, ModelInstallationReport};
use crate::state::AppState;

fn map_error(error: CoreError) -> String {
    format!("{}: {}", error.code(), error)
}

#[tauri::command]
pub fn list_model_manifests(state: State<'_, AppState>) -> Vec<ApprovedModelManifest> {
    state.model_manager.list()
}

#[tauri::command]
pub fn grant_model_consent(
    model_id: String,
    state: State<'_, AppState>,
) -> Result<ModelConsent, String> {
    let approved = state.model_manager.get(&model_id).map_err(map_error)?;
    if !approved.approved_for_local_use {
        return Err("INVALID_INPUT: model is not approved for local use".into());
    }
    let manifest = approved.consent_manifest();

    state
        .model_consents
        .insert_consent(&manifest)
        .map_err(map_error)
}

#[tauri::command]
pub fn verify_model_installation(
    model_id: String,
    installation_root: PathBuf,
    state: State<'_, AppState>,
) -> Result<ModelInstallationReport, String> {
    state
        .model_manager
        .verify_installation(&model_id, &installation_root)
        .map_err(map_error)
}

#[tauri::command]
pub fn check_model_consent(model_id: String, state: State<'_, AppState>) -> Result<bool, String> {
    state
        .model_consents
        .has_consent(&model_id)
        .map_err(map_error)
}
