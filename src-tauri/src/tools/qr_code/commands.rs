//! Tauri command layer for the QR Code tool. Thin by design: shape the
//! DTOs, run the pure `core` pipeline, open native dialogs from Rust so
//! the webview needs no permissions beyond `core:default`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

use super::core::{self, BuildRequest, ContentInput};
use crate::tools::ToolError;

/// Content as sent by the UI, tagged by tab kind.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ContentDto {
    Text {
        value: String,
    },
    Website {
        url: String,
    },
    Wifi {
        ssid: String,
        password: String,
        security: String,
        hidden: bool,
    },
}

/// All generate/save options as sent by the UI (unvalidated; Rust decides).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrOptionsDto {
    pub content: ContentDto,
    pub style: String,
    pub foreground: String,
    pub background: String,
    pub transparent: bool,
    pub error_correction: String,
    pub margin: i32,
    pub png_size: i32,
    pub logo: Option<LogoDto>,
}

/// The prepared logo as the UI holds it (came from `load_logo`).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogoDto {
    pub png_base64: String,
    pub fraction: f64,
}

/// Build result for the live preview.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QrSvg {
    pub svg: String,
    pub size_modules: usize,
    pub total_modules: usize,
    pub level_used: String,
    pub char_count: usize,
    pub max_chars: usize,
    /// Side of the saved PNG (the requested export size, used exactly).
    pub png_pixels: u32,
    pub warnings: Vec<String>,
}

fn to_build_request(options: QrOptionsDto) -> BuildRequest {
    let content = match options.content {
        ContentDto::Text { value } => ContentInput::Text(value),
        ContentDto::Website { url } => ContentInput::Website(url),
        ContentDto::Wifi {
            ssid,
            password,
            security,
            hidden,
        } => ContentInput::Wifi {
            ssid,
            password,
            security,
            hidden,
        },
    };
    BuildRequest {
        content,
        style: options.style,
        foreground: options.foreground,
        background: options.background,
        transparent: options.transparent,
        error_correction: options.error_correction,
        margin: options.margin,
        png_size: options.png_size,
        logo: options.logo.map(|logo| core::LogoInput {
            png_base64: logo.png_base64,
            fraction: logo.fraction,
        }),
    }
}

fn to_qr_svg(built: core::BuiltQr) -> QrSvg {
    QrSvg {
        svg: built.svg,
        size_modules: built.size_modules,
        total_modules: built.total_modules,
        level_used: built.level_used.label().to_string(),
        char_count: built.char_count,
        max_chars: built.max_chars,
        png_pixels: built.png_pixels,
        warnings: built.warnings,
    }
}

/// Builds the QR SVG for the live preview. The returned string is the exact
/// document used for the SVG export, so preview and file always match.
#[tauri::command]
pub fn generate_qr(options: QrOptionsDto) -> Result<QrSvg, ToolError> {
    let request = to_build_request(options);
    core::build(&request)
        .map(to_qr_svg)
        .map_err(ToolError::Internal)
}

/// The extension rule for saved files: the final file name always ends
/// with the expected extension. A name that already has it (any casing)
/// is kept exactly as typed; anything else gets the extension APPENDED —
/// like Windows' own save dialogs — so "report" becomes "report.png" and
/// a mistyped "report.jpg" becomes "report.jpg.png" instead of silently
/// dropping what the user typed. Trailing dots/spaces are trimmed first
/// (Windows ignores them in file names anyway). The check only looks at
/// the final path component, so dots in folder names ("D:\my.folder\a")
/// are never mistaken for an extension.
fn with_extension(path: &Path, extension: &str) -> PathBuf {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return path.to_path_buf();
    };
    let trimmed = name.trim_end_matches(['.', ' ']);
    let wanted = format!(".{extension}");
    let fixed = if trimmed.to_ascii_lowercase().ends_with(&wanted) {
        trimmed.to_string()
    } else {
        format!("{trimmed}{wanted}")
    };
    path.with_file_name(fixed)
}

/// Builds the QR SVG and lets the user choose a location in the native
/// SAVE dialog (confirm button "Save", with the standard overwrite
/// prompt), then writes the file. The chosen name always ends up with the
/// .svg extension. Returns the written path, or `None` when the dialog
/// was cancelled (nothing written, no message).
#[tauri::command]
pub fn save_qr_svg(app: AppHandle, options: QrOptionsDto) -> Result<Option<String>, ToolError> {
    let request = to_build_request(options);
    let built = core::build(&request).map_err(ToolError::Internal)?;

    let target = app
        .dialog()
        .file()
        .add_filter("SVG image", &["svg"])
        .set_file_name("qr-code.svg")
        .blocking_save_file();
    let Some(target) = target else {
        return Ok(None); // cancelled — nothing written
    };
    let path = with_extension(
        &target
            .into_path()
            .map_err(|_| ToolError::Internal("The chosen save location is not valid.".into()))?,
        "svg",
    );
    if path.as_os_str().is_empty() {
        return Ok(None); // nothing chosen — never write
    }

    std::fs::write(&path, built.svg.as_bytes()).map_err(|_| {
        ToolError::Internal("Windows refused to write the SVG file. Check the folder.".into())
    })?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Builds the QR code, rasterizes it at the exact export size and lets
/// the user choose a location in the native SAVE dialog (confirm button
/// "Save", with the standard overwrite prompt), then writes the PNG. The
/// chosen name always ends up with the .png extension. Returns the
/// written path, or `None` when the dialog was cancelled.
#[tauri::command]
pub fn save_qr_png(app: AppHandle, options: QrOptionsDto) -> Result<Option<String>, ToolError> {
    let request = to_build_request(options);
    let built = core::build(&request).map_err(ToolError::Internal)?;

    let png = super::raster::render_png(
        &built.shapes,
        built.total_modules,
        built.foreground,
        built.background,
        built.png_pixels,
        built.plate.as_ref(),
        built.logo.as_ref(),
    )
    .map_err(ToolError::Internal)?;

    let target = app
        .dialog()
        .file()
        .add_filter("PNG image", &["png"])
        .set_file_name("qr-code.png")
        .blocking_save_file();
    let Some(target) = target else {
        return Ok(None); // cancelled — nothing written
    };
    let path = with_extension(
        &target
            .into_path()
            .map_err(|_| ToolError::Internal("The chosen save location is not valid.".into()))?,
        "png",
    );
    if path.as_os_str().is_empty() {
        return Ok(None); // nothing chosen — never write
    }

    std::fs::write(&path, png).map_err(|_| {
        ToolError::Internal("Windows refused to write the PNG file. Check the folder.".into())
    })?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Opens the native single-file dialog for a logo image (Rust side, so
/// the webview needs no dialog permission). Returns the absolute path or
/// null on cancel.
#[tauri::command]
pub fn pick_logo_file(app: AppHandle) -> Option<String> {
    let picked = app
        .dialog()
        .file()
        .add_filter("Logo images", &["png", "jpg", "jpeg", "webp"])
        .add_filter("All files", &["*"])
        .blocking_pick_file();
    picked
        .and_then(|file| file.into_path().ok())
        .map(|path| path.to_string_lossy().into_owned())
}

/// Loads and prepares a logo from the given path: safe decode, downscale
/// into a square transparent canvas, re-encode as PNG. The frontend only
/// ever holds this prepared PNG — never the original file.
#[tauri::command]
pub fn load_logo(path: String) -> Result<LogoPrepared, ToolError> {
    super::logo::load_from_path(std::path::Path::new(&path))
        .map(|data| LogoPrepared {
            edge: data.edge,
            png_base64: data.png_base64,
        })
        .map_err(ToolError::Internal)
}

/// The prepared logo returned to the UI.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogoPrepared {
    pub edge: u32,
    pub png_base64: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact JSON shape the frontend sends must deserialize into our
    /// DTOs (tagged enum + camelCase), so a rename on either side fails
    /// here instead of silently at runtime.
    #[test]
    fn frontend_json_shapes_deserialize() {
        let options: QrOptionsDto = serde_json::from_str(
            r##"{
                "content": { "kind": "text", "value": "hello" },
                "style": "round",
                "foreground": "#000000",
                "background": "#ffffff",
                "transparent": false,
                "errorCorrection": "Q",
                "margin": 4,
                    "pngSize": 1024,
                    "logo": null
            }"##,
        )
        .expect("text shape must parse");
        assert!(matches!(
            options.content,
            ContentDto::Text { ref value } if value == "hello"
        ));
        assert_eq!(options.error_correction, "Q");

        let options: QrOptionsDto = serde_json::from_str(
            r##"{
                "content": {
                    "kind": "wifi",
                    "ssid": "net",
                    "password": "pw",
                    "security": "wpa",
                    "hidden": true
                },
                "style": "sharp",
                "foreground": "#000000",
                "background": "#ffffff",
                "transparent": false,
                "errorCorrection": "H",
                "margin": 4,
                    "pngSize": 1024,
                    "logo": null
            }"##,
        )
        .expect("wifi shape must parse");
        assert!(matches!(
            options.content,
            ContentDto::Wifi { ref ssid, hidden: true, .. } if ssid == "net"
        ));

        let options: QrOptionsDto = serde_json::from_str(
            r##"{
                "content": { "kind": "website", "url": "example.com" },
                "style": "sharp",
                "foreground": "#000000",
                "background": "#ffffff",
                "transparent": false,
                "errorCorrection": "M",
                "margin": 4,
                    "pngSize": 1024,
                    "logo": null
            }"##,
        )
        .expect("website shape must parse");
        assert!(matches!(options.content, ContentDto::Website { .. }));

        // The full pipeline runs through the same DTO path.
        let svg = generate_qr(
            serde_json::from_str(
                r##"{
                    "content": { "kind": "text", "value": "bridge" },
                    "style": "sharp",
                    "foreground": "#000000",
                    "background": "#ffffff",
                    "transparent": false,
                    "errorCorrection": "Q",
                    "margin": 4,
                    "pngSize": 1024,
                    "logo": null
                }"##,
            )
            .unwrap(),
        )
        .expect("builds");
        assert!(svg.svg.starts_with("<svg"));
    }

    /// The save-dialog name fix: the final file always ends with the
    /// expected extension, nothing typed is silently replaced.
    #[test]
    fn save_paths_always_get_the_right_extension() {
        let png = |p: &str| with_extension(Path::new(p), "png");
        let svg = |p: &str| with_extension(Path::new(p), "svg");

        // No extension: appended.
        assert_eq!(png("a"), PathBuf::from("a.png"));
        assert_eq!(svg("b"), PathBuf::from("b.svg"));
        // Right extension in any casing: kept exactly as typed.
        assert_eq!(png("a.PNG"), PathBuf::from("a.PNG"));
        assert_eq!(png("qr-code.png"), PathBuf::from("qr-code.png"));
        assert_eq!(svg("B.SVG"), PathBuf::from("B.SVG"));
        // A different extension is NOT replaced: append instead.
        assert_eq!(png("a.jpg"), PathBuf::from("a.jpg.png"));
        assert_eq!(svg("a.png"), PathBuf::from("a.png.svg"));
        // Dots in folder names never count as the file's extension.
        assert_eq!(png(r"C:\my.folder\a"), PathBuf::from(r"C:\my.folder\a.png"));
        // Trailing dots/spaces are trimmed like Windows does.
        assert_eq!(png("a."), PathBuf::from("a.png"));
        assert_eq!(png("a . "), PathBuf::from("a.png"));
    }
}
