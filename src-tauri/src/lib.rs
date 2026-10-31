mod tools;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            // Central command registry: every command the frontend can call
            // is listed here exactly once. Adding a tool backend means adding
            // its module under `tools/` and one line here per command.
            // See docs/ADDING_A_TOOL.md.
            tools::app_info::get_app_info,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
