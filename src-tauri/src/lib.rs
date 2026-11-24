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
        .manage(tools::update_check::commands::UpdateState::default())
        .manage(tools::vault::commands::VaultState::default())
        .manage(tools::notes::commands::NotesState::default())
        .manage(tools::notes::window::NotesWindowState::default())
        .manage(tools::notes::clipboard::NotesClipboardState::default())
        .setup(|app| {
            // Awake startup: resolve the state folder, run the app-start
            // reconcile (R4) and start the 5-second background thread.
            tools::system_manager::awake::commands::init(&app.handle().clone());
            // Secure Notes: only the first running instance may use Secure
            // Notes; the others get "unavailable". The guard also removes
            // profile folders left by an earlier run.
            let handle = app.handle().clone();
            if tools::notes::window::init(&handle) {
                tools::notes::commands::init(&handle);
                tools::notes::clipboard::init(&handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Awake R7: closing the app while Awake is on must ask the user
            // first. This handler also looks after the Secure Notes window
            // below; nothing else chains here today.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
                && tools::system_manager::awake::commands::is_on_for_close_check(
                    window.app_handle(),
                )
            {
                api.prevent_close();
                let _ = window.app_handle().emit("awake://close-requested", ());
            }
            if let tauri::WindowEvent::Destroyed = event {
                tools::notes::window::on_destroyed(window.app_handle(), window.label());
            }
        })
        .invoke_handler(tauri::generate_handler![
            // Central command registry: every command the frontend can call
            // is listed here exactly once. Adding a tool backend means adding
            // its module under `tools/` and one line here per command.
            // See docs/ADDING_A_TOOL.md.
            tools::app_info::get_app_info,
            tools::app_info::open_github_page,
            tools::app_info::save_wallpaper_to_downloads,
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
            tools::favicons::commands::favicons_pick_source,
            tools::favicons::commands::favicons_inspect_source,
            tools::favicons::commands::favicons_preview,
            tools::favicons::commands::favicons_save,
            tools::favicons::commands::favicons_reveal_folder,
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
            tools::system_manager::quick_launch::commands::quick_launch_info,
            tools::system_manager::quick_launch::commands::quick_launch_open,
            tools::system_manager::system_info::commands::system_info_get,
            tools::metadata_remover::commands::mr_pick_files,
            tools::metadata_remover::commands::mr_pick_output_folder,
            tools::metadata_remover::commands::mr_scan_files,
            tools::metadata_remover::commands::mr_clean_files,
            tools::metadata_remover::commands::mr_cancel,
            tools::metadata_remover::commands::mr_reveal_files,
            // Secure Notes: words flows, note operations, lock and save.
            tools::notes::commands::notes_status,
            tools::notes::commands::notes_generate_words,
            tools::notes::commands::notes_refresh_word,
            tools::notes::commands::notes_confirm_words_saved,
            tools::notes::commands::notes_retype_words,
            tools::notes::commands::notes_clear_words,
            tools::notes::commands::notes_lock,
            tools::notes::commands::notes_activity,
            tools::notes::commands::notes_save_state,
            tools::notes::commands::notes_save_now,
            tools::notes::commands::notes_get_settings,
            tools::notes::commands::notes_set_settings,
            tools::notes::commands::notes_list,
            tools::notes::commands::notes_get_note,
            tools::notes::commands::notes_create_note,
            tools::notes::commands::notes_update_note,
            tools::notes::commands::notes_delete_note,
            tools::notes::commands::notes_finish_create,
            tools::notes::commands::notes_unlock,
            tools::notes::commands::notes_change_passphrase,
            // Secure Notes window: open the hardened window and close it.
            tools::notes::window::notes_open_window,
            tools::notes::window::notes_close_window,
            // Secure Notes clipboard: Rust writes the selected text.
            tools::notes::clipboard::notes_copy_text,
            // Update check: the app's only network access (R3), run solely
            // when the user clicks the update button on the home screen.
            tools::update_check::commands::check_for_updates,
            tools::update_check::commands::open_update_page,
            // Quantum Resistance Vault: passphrase session, dialogs,
            // planning, encrypt / decrypt with progress, cancel.
            tools::vault::commands::vault_generate_passphrase,
            tools::vault::commands::vault_refresh_word,
            tools::vault::commands::vault_confirm_saved,
            tools::vault::commands::vault_clear_passphrase,
            tools::vault::commands::vault_pick_source_file,
            tools::vault::commands::vault_pick_vault_file,
            tools::vault::commands::vault_pick_folder,
            tools::vault::commands::vault_prepare_encrypt,
            tools::vault::commands::vault_prepare_decrypt,
            tools::vault::commands::vault_encrypt_file,
            tools::vault::commands::vault_decrypt_file,
            tools::vault::commands::vault_cancel,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // Secure Notes: save and lock on exit (the timer thread ends with
            // the process; this is the last chance to write the notes).
            if let tauri::RunEvent::Exit = event {
                tools::notes::commands::on_exit(app_handle);
                // First save and lock the notes, then remove the WebView2
                // profile folder (order matters).
                tools::notes::window::on_exit(app_handle);
                // Remove our text from the clipboard if it is still ours.
                tools::notes::clipboard::on_exit(app_handle);
            }
        });
}
