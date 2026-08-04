use std::collections::BTreeMap;

use serde::Deserialize;
use tauri::State;

use crate::{
    domain::CoreError,
    hardening::{PrivacySettings, RecoverySummary},
    security::{CredentialReference, SecretString},
    state::AppState,
};

fn map(error: CoreError) -> String {
    format!("{}: {}", error.code(), error)
}

fn reference(service: &str, account: &str) -> Result<CredentialReference, String> {
    if !matches!(service, "vietdub.translation" | "vietdub.tts") {
        return Err("INVALID_INPUT: credential service".into());
    }
    CredentialReference::new(service, account).map_err(map)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveCredentialRequest {
    pub service: String,
    pub account: String,
    pub secret: String,
}

#[tauri::command]
pub fn save_provider_credential(
    request: SaveCredentialRequest,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let reference = reference(&request.service, &request.account)?;
    let secret = SecretString::new(request.secret).map_err(map)?;
    state.credentials.put(&reference, &secret).map_err(map)?;
    state
        .privacy
        .write_event(
            "CREDENTIAL_SAVED",
            &BTreeMap::from([("service".into(), request.service)]),
        )
        .map_err(map)
}

#[tauri::command]
pub fn delete_provider_credential(
    service: String,
    account: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let reference = reference(&service, &account)?;
    state.credentials.delete(&reference).map_err(map)?;
    state
        .privacy
        .write_event(
            "CREDENTIAL_DELETED",
            &BTreeMap::from([("service".into(), service)]),
        )
        .map_err(map)
}

#[tauri::command]
pub fn provider_credential_available(
    service: String,
    account: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let reference = reference(&service, &account)?;
    Ok(state.credentials.get(&reference).is_ok())
}

#[tauri::command]
pub fn get_privacy_settings(state: State<'_, AppState>) -> Result<PrivacySettings, String> {
    state.privacy.settings().map_err(map)
}

#[tauri::command]
pub fn save_privacy_settings(
    settings: PrivacySettings,
    state: State<'_, AppState>,
) -> Result<PrivacySettings, String> {
    state.privacy.save_settings(settings).map_err(map)
}

#[tauri::command]
pub fn get_recovery_summary(state: State<'_, AppState>) -> RecoverySummary {
    state.runtime_session.summary()
}
