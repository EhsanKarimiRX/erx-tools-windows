mod tools;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Native file dialogs for tools that pick files/folders. Used from
        // Rust commands only, so no webview permission is granted for it.
        .plugin(tauri_plugin_dialog::init())
        // "Open output folder" support (reveal in Explorer). Also used from
        // Rust commands only; the webview gets no opener permission.
        .plugin(tauri_plugin_opener::init())
        .manage(tools::image_convert::commands::BatchState::default())
        .manage(tools::folder_icon::commands::BatchState::default())
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
            tools::folder_icon::commands::folder_icon_refresh_icons,
            #[cfg(debug_assertions)]
            tools::folder_icon::commands::folder_icon_debug_probe,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
