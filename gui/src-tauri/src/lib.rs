//! CamFox GUI — an AdsPower-style control panel over the Camoufox browser.

mod commands;
mod cookies;
mod forwarder;
mod launch;
mod settings;
mod sidecar;
mod store;
mod types;

use commands::AppState;
use launch::kill_tree;
use settings::Settings;
use tauri::{Manager, RunEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)
                .map_err(|e| format!("cannot create data dir {}: {e}", data_dir.display()))?;
            app.manage(AppState::new(data_dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_profiles,
            commands::list_groups,
            commands::save_profile,
            commands::delete_profile,
            commands::duplicate_profile,
            commands::prepare_profile,
            commands::launch_profile,
            commands::close_profile,
            commands::running_profiles,
            commands::open_automation,
            commands::close_automation,
            commands::check_proxy,
            commands::kernel_info,
            commands::kernel_fetch,
            commands::kernel_set,
            commands::kernel_remove,
            commands::export_profiles,
            commands::import_profiles,
            commands::export_cookies,
            commands::import_cookies,
            commands::get_settings,
            commands::save_settings,
            commands::data_dir,
            commands::write_text_file,
            commands::read_text_file,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let RunEvent::Exit = event {
                let settings = Settings::load(
                    &app_handle
                        .path()
                        .app_data_dir()
                        .unwrap_or_else(|_| std::path::PathBuf::from(".")),
                );
                if settings.close_browsers_on_exit {
                    close_all(app_handle);
                }
            }
        });
}

fn close_all(app_handle: &tauri::AppHandle) {
    if let Some(state) = app_handle.try_state::<AppState>() {
        let pids: Vec<u32> = state
            .running
            .lock()
            .unwrap()
            .values()
            .map(launch::Running::pid)
            .collect();
        for pid in pids {
            let _ = kill_tree(pid);
        }
    }
}
