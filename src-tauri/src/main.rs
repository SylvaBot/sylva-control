// Never open a console window on Windows, in debug or release builds.
#![windows_subsystem = "windows"]

mod commands;
mod secrets;
mod state;
mod updater;

fn main() {
    updater::clean_up_old_build();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(state::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::restore_session,
            commands::get_feed,
            commands::get_stats,
            commands::get_viewers,
            commands::get_channel_info,
            commands::get_ad_schedule,
            commands::search_categories,
            commands::update_channel,
            commands::alert_action,
            commands::get_alert_state,
            commands::reload_overlays,
            commands::replay_activity,
            commands::sylva_sign_in,
            commands::sylva_cancel_sign_in,
            commands::sylva_sign_out,
            commands::twitch_sign_in,
            commands::twitch_cancel_sign_in,
            commands::twitch_sign_out,
            commands::get_audience,
            commands::open_external,
            updater::check_for_update,
            updater::apply_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Cortex");
}
