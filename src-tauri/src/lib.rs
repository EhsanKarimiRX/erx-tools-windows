mod tools;

use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Native file dialogs for tools that pick files/folders. Used from
        // Rust commands only, so no webview permission is granted for it.
        .plugin(tauri_plugin_dialog::init())
        // "Open output folder" support (reveal in Explorer). Also used from
        // Rust commands only; the webview gets no opener permission.
        .plugin(tauri_plugin_opener::init())
        // System Manager / Awake: best-effort native notifications when
        // Awake switches itself off. Rust-side only; the webview gets no
        // notification permission.
        .plugin(tauri_plugin_notification::init())
        .manage(tools::image_convert::commands::BatchState::default())
        .manage(tools::folder_icon::commands::BatchState::default())
        .manage(tools::system_manager::awake::commands::AwakeRuntime::default())
        .setup(|app| {
            // Awake startup: resolve the state folder, run the app-start
            // reconcile (R4) and start the 5-second background thread.
            tools::system_manager::awake::commands::init(&app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            // Awake R7: closing the app while Awake is on must ask the user
            // first. This is the app's only window-event handler (nothing to
            // chain with today).
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
                && tools::system_manager::awake::commands::is_on_for_close_check(
                    window.app_handle(),
                )
            {
                api.prevent_close();
                let _ = window.app_handle().emit("awake://close-requested", ());
            }
        })
        .invoke_handler(tauri::generate_handler![
            // Central command registry: every command the frontend can call
            // is listed here exactly once. Adding a tool backend means adding
            // its module under `tools/` and one line here per command.
            // See docs/ADDING_A_TOOL.md.
            tools::app_info::get_app_info,
            tools::image_convert::commands::inspect_files,
            tools::image_convert::commands::pick_image_files,
            tools::image_convert::commands::pick_output_folder,
            tools::image_convert::commands::convert_files,
            tools::image_convert::commands::cancel_conversion,
            tools::image_convert::commands::reveal_outputs,
            tools::qr_code::commands::generate_qr,
            tools::qr_code::commands::save_qr_svg,
            tools::qr_code::commands::save_qr_png,
            tools::qr_code::commands::pick_logo_file,
            tools::qr_code::commands::load_logo,
            tools::folder_icon::commands::folder_icon_preview_svg,
            tools::folder_icon::commands::folder_icon_pick_folders,
            tools::folder_icon::commands::folder_icon_inspect,
            tools::folder_icon::commands::folder_icon_apply_batch,
            tools::folder_icon::commands::folder_icon_restore_batch,
            tools::folder_icon::commands::folder_icon_cancel_batch,
            tools::system_manager::awake::commands::awake_get_status,
            tools::system_manager::awake::commands::awake_enable,
            tools::system_manager::awake::commands::awake_disable,
            tools::system_manager::awake::commands::awake_set_options,
            tools::system_manager::awake::commands::awake_exit,
            tools::system_manager::desktop_icons::commands::desktop_icons_get,
            tools::system_manager::desktop_icons::commands::desktop_icons_set,
            tools::system_manager::organize::commands::organize_pick_folder,
            tools::system_manager::organize::commands::organize_scan,
            tools::system_manager::organize::commands::organize_run,
            tools::system_manager::organize::commands::organize_undo_info,
            tools::system_manager::organize::commands::organize_undo,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
