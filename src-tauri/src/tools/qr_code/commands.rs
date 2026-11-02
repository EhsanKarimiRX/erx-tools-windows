//! Tauri command layer for the QR Code tool. Thin by design: shape the
//! DTOs, run the pure `core` pipeline, open native dialogs from Rust so
//! the webview needs no permissions beyond `core:default`.

use serde::{Deserialize, Serialize};
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
    /// Exact PNG side length after snapping to whole pixels per module.
    pub png_pixels: u32,
    pub png_pixels_per_module: u32,
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
        png_pixels_per_module: built.png_pixels_per_module,
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

/// Builds the QR SVG and lets the user pick a save location in the native
/// Save As dialog, then writes the file. Returns the written path, or
/// `None` when the dialog was cancelled. The dialog's own overwrite
/// confirmation is the only overwrite protection (never silent).
#[tauri::command]
pub fn save_qr_svg(app: AppHandle, options: QrOptionsDto) -> Result<Option<String>, ToolError> {
    let request = to_build_request(options);
    let built = core::build(&request).map_err(ToolError::Internal)?;

    let target = app
        .dialog()
        .file()
        .add_filter("SVG image", &["svg"])
        .set_file_name("qr-code.svg")
        .blocking_pick_file();
    let Some(path) = target else {
        return Ok(None); // cancelled — nothing written
    };
    let path = path
        .into_path()
        .map_err(|_| ToolError::Internal("The chosen save location is not valid.".into()))?;

    std::fs::write(&path, built.svg.as_bytes()).map_err(|_| {
        ToolError::Internal("Windows refused to write the SVG file. Check the folder.".into())
    })?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Builds the QR code, rasterizes it at the snapped size and lets the user
/// pick a save location in the native Save As dialog, then writes the PNG.
/// Returns the written path, or `None` when the dialog was cancelled.
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
    )
    .map_err(ToolError::Internal)?;

    let target = app
        .dialog()
        .file()
        .add_filter("PNG image", &["png"])
        .set_file_name("qr-code.png")
        .blocking_pick_file();
    let Some(path) = target else {
        return Ok(None); // cancelled — nothing written
    };
    let path = path
        .into_path()
        .map_err(|_| ToolError::Internal("The chosen save location is not valid.".into()))?;

    std::fs::write(&path, png).map_err(|_| {
        ToolError::Internal("Windows refused to write the PNG file. Check the folder.".into())
    })?;
    Ok(Some(path.to_string_lossy().into_owned()))
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
                    "pngSize": 1024
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
                    "pngSize": 1024
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
                    "pngSize": 1024
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
                    "pngSize": 1024
                }"##,
            )
            .unwrap(),
        )
        .expect("builds");
        assert!(svg.svg.starts_with("<svg"));
    }
}
