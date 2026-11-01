mod tools;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Native file dialogs for tools that pick files/folders. Used from
        // Rust commands only, so no webview permission is granted for it.
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            // Central command registry: every command the frontend can call
            // is listed here exactly once. Adding a tool backend means adding
            // its module under `tools/` and one line here per command.
            // See docs/ADDING_A_TOOL.md.
            tools::app_info::get_app_info,
            tools::image_convert::commands::inspect_files,
            tools::image_convert::commands::pick_image_files,
            tools::image_convert::commands::pick_output_folder,
            tools::image_convert::commands::probe_conversion,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
