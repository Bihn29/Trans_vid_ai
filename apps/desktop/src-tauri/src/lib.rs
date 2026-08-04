pub mod commands;
pub mod domain;
pub mod hardening;
pub mod infrastructure;
pub mod jobs;
pub mod media;
pub mod persistence;
pub mod processes;
pub mod security;
pub mod workers;

use state::AppState;
use tauri::Manager;

mod state;

#[tauri::command]
fn health_check() -> &'static str {
    "ok"
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = AppState::initialize(app.handle())
                .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            health_check,
            commands::projects::create_project,
            commands::projects::get_project,
            commands::projects::list_projects,
            commands::projects::update_project,
            commands::projects::delete_project,
            commands::media::import_local_media,
            commands::media::get_source_media,
            commands::media::get_latest_render_media,
            commands::media::log_preview_event,
            commands::jobs::start_transcript_job,
            commands::jobs::start_dub_render_job,
            commands::jobs::list_project_jobs,
            commands::jobs::cancel_job,
            commands::transcript::get_transcript,
            commands::transcript::update_segment,
            commands::transcript::split_segment,
            commands::transcript::merge_segments,
            commands::transcript::approve_transcript,
            commands::transcript::check_transcript_quality,
            commands::translation::list_translation_providers,
            commands::translation::list_translation_blocks,
            commands::translation::list_glossary,
            commands::translation::upsert_glossary,
            commands::translation::list_locked_names,
            commands::translation::add_locked_name,
            commands::translation::approve_translation,
            commands::tts::list_voices,
            commands::tts::list_voice_assignments,
            commands::tts::set_voice_assignment,
            commands::tts::preview_voice,
            commands::audio::list_separation_engines,
            commands::audio::get_audio_mix_settings,
            commands::audio::save_audio_mix_settings,
            commands::composer::get_composer_config,
            commands::composer::save_composer_config,
            commands::composer::export_composer_artifact,
            commands::composer::import_composer_overlay,
            commands::models::list_model_manifests,
            commands::models::grant_model_consent,
            commands::models::check_model_consent,
            commands::models::verify_model_installation,
            commands::hardening::save_provider_credential,
            commands::hardening::delete_provider_credential,
            commands::hardening::provider_credential_available,
            commands::hardening::get_privacy_settings,
            commands::hardening::save_privacy_settings,
            commands::hardening::get_recovery_summary,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run VietDub Studio");
}
