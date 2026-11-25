// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod secrets;
mod state;
mod updater;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(state::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::restore_session,
            commands::se_sign_in,
            commands::se_sign_out,
            commands::get_feed,
            commands::get_stats,
            commands::get_viewers,
            commands::alert_action,
            commands::reload_overlays,
            commands::replay_activity,
            commands::twitch_sign_in,
            commands::twitch_cancel_sign_in,
            commands::twitch_sign_out,
            commands::get_audience,
            commands::open_external,
            updater::check_for_update,
            updater::apply_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Pine");
}
