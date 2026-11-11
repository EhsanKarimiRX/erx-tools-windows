use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::Path;

use serde::Serialize;
use tauri::Manager;

use crate::tools::update_check::commands::open_url_in_browser;

/// Basic information about the app, exposed to the frontend.
#[derive(Serialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

/// Returns the app name and version. Single source of truth is
/// `tauri.conf.json` (productName + version), read through the package info.
#[tauri::command]
pub fn get_app_info(app: tauri::AppHandle) -> AppInfo {
    let package_info = app.package_info();
    AppInfo {
        name: package_info.name.clone(),
        version: package_info.version.to_string(),
    }
}

/// The GitHub page the About button opens. Hardcoded on purpose: the frontend
/// only asks the app to open "its" page and can never choose the URL.
const GITHUB_REPO_URL: &str = "https://github.com/EhsanKarimiRX/erx-tools-windows";

/// Opens the project's GitHub page in the default browser, through the same
/// ShellExecuteW opener the update button uses.
#[tauri::command]
pub fn open_github_page() -> Result<(), String> {
    if open_url_in_browser(GITHUB_REPO_URL) {
        Ok(())
    } else {
        Err("Couldn't open your browser.".to_string())
    }
}

/// Wallpaper embedded in the app for the About-dialog Easter egg.
const WALLPAPER_BYTES: &[u8] = include_bytes!("../../assets/erx-wallpaper.jpg");
const WALLPAPER_FILE_STEM: &str = "ERX-Tools-Wallpaper";

/// Saves the embedded wallpaper into the user's Downloads folder and returns
/// the file name it was saved under. Never overwrites an existing file.
#[tauri::command]
pub fn save_wallpaper_to_downloads(app: tauri::AppHandle) -> Result<String, String> {
    let dir = app
        .path()
        .download_dir()
        .map_err(|_| "Couldn't find your Downloads folder.".to_string())?;
    save_wallpaper_into(&dir, WALLPAPER_BYTES)
}

fn save_wallpaper_into(dir: &Path, bytes: &[u8]) -> Result<String, String> {
    const SAVE_ERROR: &str = "Couldn't save the wallpaper to your Downloads folder.";
    if !dir.is_dir() {
        return Err("Couldn't find your Downloads folder.".to_string());
    }
    for attempt in 0..1000u32 {
        let name = if attempt == 0 {
            format!("{WALLPAPER_FILE_STEM}.jpg")
        } else {
            format!("{WALLPAPER_FILE_STEM} ({attempt}).jpg")
        };
        let path = dir.join(&name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if file.write_all(bytes).is_err() {
                    drop(file);
                    let _ = fs::remove_file(&path);
                    return Err(SAVE_ERROR.to_string());
                }
                return Ok(name);
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(SAVE_ERROR.to_string()),
        }
    }
    Err(SAVE_ERROR.to_string())
}
