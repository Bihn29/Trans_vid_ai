use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::State;
use uuid::Uuid;

use crate::{
    domain::{CoreError, NewProject, Project, ProjectStatus, ProjectUpdate, WorkflowMode},
    state::AppState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectRequest {
    pub name: String,
    pub workflow_mode: WorkflowMode,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProjectRequest {
    pub name: Option<String>,
    pub status: Option<ProjectStatus>,
    pub config_snapshot: Option<Map<String, Value>>,
}

#[derive(Debug, Serialize)]
pub struct CommandError {
    pub code: &'static str,
}

impl From<CoreError> for CommandError {
    fn from(error: CoreError) -> Self {
        Self { code: error.code() }
    }
}

#[tauri::command]
pub fn create_project(
    state: State<'_, AppState>,
    request: CreateProjectRequest,
) -> Result<Project, CommandError> {
    state
        .projects
        .create(&NewProject::chinese_to_vietnamese(
            request.name,
            request.workflow_mode,
        ))
        .map_err(Into::into)
}

#[tauri::command]
pub fn get_project(state: State<'_, AppState>, id: Uuid) -> Result<Project, CommandError> {
    state.projects.get(id).map_err(Into::into)
}

#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> Result<Vec<Project>, CommandError> {
    state.projects.list().map_err(Into::into)
}

#[tauri::command]
pub fn update_project(
    state: State<'_, AppState>,
    id: Uuid,
    request: UpdateProjectRequest,
) -> Result<Project, CommandError> {
    state
        .projects
        .update(
            id,
            &ProjectUpdate {
                name: request.name,
                status: request.status,
                source_asset_id: None,
                config_snapshot: request.config_snapshot,
            },
        )
        .map_err(Into::into)
}

#[tauri::command]
pub fn delete_project(state: State<'_, AppState>, id: Uuid) -> Result<(), CommandError> {
    state.projects.delete(id).map_err(Into::into)
}
