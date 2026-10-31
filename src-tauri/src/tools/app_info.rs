use serde::Serialize;

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
