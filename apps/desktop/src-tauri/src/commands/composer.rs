use std::path::PathBuf;

use tauri::State;
use uuid::Uuid;

use crate::{
    domain::{ComposerConfig, CoreError},
    jobs::InvalidationChange,
    state::AppState,
};

fn map(error: CoreError) -> String {
    format!("{}: {}", error.code(), error)
}

#[tauri::command]
pub fn get_composer_config(
    project_id: Uuid,
    state: State<'_, AppState>,
) -> Result<ComposerConfig, String> {
    state.composer.get_config(project_id).map_err(map)
}

#[tauri::command]
pub fn save_composer_config(
    config: ComposerConfig,
    state: State<'_, AppState>,
) -> Result<ComposerConfig, String> {
    config.validate().map_err(map)?;
    state
        .invalidation
        .invalidate(config.project_id, &InvalidationChange::Composition)
        .map_err(map)?;
    state.composer.save_config(&config).map_err(map)
}

#[tauri::command]
pub fn export_composer_artifact(
    project_id: Uuid,
    artifact_id: Uuid,
    destination: PathBuf,
    state: State<'_, AppState>,
) -> Result<PathBuf, String> {
    state
        .composer_export
        .export(project_id, artifact_id, &destination)
        .map_err(|error| format!("COMPOSER_EXPORT_FAILED: {error}"))
}

#[tauri::command]
pub fn import_composer_overlay(
    project_id: Uuid,
    source: PathBuf,
    state: State<'_, AppState>,
) -> Result<crate::domain::Artifact, String> {
    state
        .composer_assets
        .import_overlay(project_id, &source)
        .map_err(|error| format!("COMPOSER_IMPORT_FAILED: {error}"))
}
