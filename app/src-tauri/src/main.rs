// Pas de console derrière la fenêtre en version publiée.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod fs;
mod jobs;
mod launch;
mod recent;
mod settings;
#[cfg(windows)]
mod win;

use tauri::{LogicalSize, Manager};

fn main() {
    let launch = launch::Launch::from_args(std::env::args_os().skip(1));
    let task = launch.is_task();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(jobs::Jobs::default())
        .manage(recent::Recent::default())
        .manage(launch)
        .setup(move |app| {
            std::thread::spawn(commands::clean_scratch);
            // La fenêtre naît cachée : l'interface l'affiche une fois prête, sans flash.
            if task {
                let window = app.get_webview_window("main").expect("fenêtre principale déclarée dans tauri.conf.json");
                window.set_size(LogicalSize::new(460.0, 176.0))?;
                window.set_min_size(None::<LogicalSize<f64>>)?;
                window.set_resizable(false)?;
                window.set_maximizable(false)?;
                window.center()?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::launch_info,
            commands::formats,
            commands::open_archive,
            commands::extract,
            commands::create,
            commands::cancel,
            commands::suggest_archive_path,
            commands::open_entry,
            commands::reveal,
            commands::open_path,
            commands::system_accent,
            commands::update,
            commands::file_icons,
            commands::drag_out,
            commands::test_archive,
            commands::notify,
            recent::recent_list,
            recent::recent_add,
            recent::recent_remove,
            fs::list_dir,
            fs::places,
            fs::path_icons,
            fs::drag_paths,
            fs::path_kind,
            settings::settings_load,
            settings::settings_save,
            settings::set_context_menu_hidden,
            settings::file_types_status,
            settings::register_file_types,
            settings::open_default_apps,
            settings::open_url,
        ])
        .run(tauri::generate_context!())
        .expect("échec du démarrage de Lynk");
}
