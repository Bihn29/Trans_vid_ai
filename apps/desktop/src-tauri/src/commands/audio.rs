use crate::{
    domain::{AudioMixSettings, CoreError, SeparationEngineDescriptor},
    jobs::InvalidationChange,
    state::AppState,
};
use tauri::State;
use uuid::Uuid;

fn map(error: CoreError) -> String {
    format!("{}: {}", error.code(), error)
}

fn project_id(value: &str) -> Result<Uuid, String> {
    Uuid::parse_str(value).map_err(|_| "invalid project_id".into())
}

#[tauri::command]
pub fn list_separation_engines() -> Vec<SeparationEngineDescriptor> {
    vec![SeparationEngineDescriptor {
        engine_id: "energy-mask-v1".into(),
        display_name: "VietDub Energy Mask".into(),
        version: "1.0.0".into(),
        license: "UNLICENSED".into(),
        install_mode: "bundled_source".into(),
        requires_consent: false,
        sends_data_off_device: false,
        approved: true,
    }]
}

#[tauri::command]
pub fn get_audio_mix_settings(
    project_id_value: String,
    state: State<'_, AppState>,
) -> Result<AudioMixSettings, String> {
    state
        .audio
        .get_settings(project_id(&project_id_value)?)
        .map_err(map)
}

#[tauri::command]
pub fn save_audio_mix_settings(
    settings: AudioMixSettings,
    state: State<'_, AppState>,
) -> Result<AudioMixSettings, String> {
    state
        .invalidation
        .invalidate(settings.project_id, &InvalidationChange::AudioMix)
        .map_err(map)?;
    state.audio.save_settings(&settings).map_err(map)
}
