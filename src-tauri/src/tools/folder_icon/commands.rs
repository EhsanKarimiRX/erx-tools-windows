//! Tauri command layer for the Folder Icon tool. Thin by design: shape the
//! DTOs, run the pure `core` pipeline. All validation happens in
//! `settings.rs`; the webview needs no permissions beyond `core:default`.

use serde::{Deserialize, Serialize};

use super::core;
use crate::tools::ToolError;

/// Raw settings as sent by the UI (unvalidated; Rust decides).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderIconSettingsDto {
    pub base_color: String,
    pub pattern: String,
    pub pattern_color: String,
    pub pattern_size: i32,
    pub pattern_opacity: i32,
}

/// The live preview: the SVG document plus non-blocking hints.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderIconPreview {
    pub svg: String,
    pub warnings: Vec<String>,
    /// Small previews rendered exactly like the .ico frames (PNG data
    /// URLs), so what the UI shows at 16/32/48 px IS the icon data.
    pub frames: Vec<FramePreview>,
}

/// One small preview frame.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FramePreview {
    pub size: u32,
    pub data_url: String,
}

/// Small preview frame sizes, matching the smallest .ico entries.
const SMALL_FRAME_SIZES: [u32; 3] = [16, 32, 48];

/// `data:image/png;base64,…` for a PNG buffer (the base64 crate is already
/// in the project via the QR tool's logo embedding).
fn png_data_url(png: &[u8]) -> String {
    format!(
        "data:image/png;base64,{}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, png)
    )
}

/// Builds the folder icon SVG for the live preview plus the small preview
/// frames rendered on the exact .ico path.
#[tauri::command]
pub fn folder_icon_preview_svg(
    settings: FolderIconSettingsDto,
) -> Result<FolderIconPreview, ToolError> {
    let settings = super::settings::IconSettings::from_parts(
        &settings.base_color,
        &settings.pattern,
        &settings.pattern_color,
        settings.pattern_size,
        settings.pattern_opacity,
    )
    .map_err(ToolError::Internal)?;
    let built = core::build(&settings);
    let frames = SMALL_FRAME_SIZES
        .iter()
        .map(|&size| {
            super::raster::render_frame_png(&built.params, size)
                .map(|png| FramePreview {
                    size,
                    data_url: png_data_url(&png),
                })
                .map_err(ToolError::Internal)
        })
        .collect::<Result<Vec<_>, ToolError>>()?;
    Ok(FolderIconPreview {
        svg: built.svg,
        warnings: built.warnings,
        frames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact JSON shape the frontend sends must deserialize into our
    /// DTO (camelCase), so a rename on either side fails here instead of
    /// silently at runtime.
    #[test]
    fn frontend_json_shape_deserializes() {
        let dto: FolderIconSettingsDto = serde_json::from_str(
            r##"{
                "baseColor": "#0078D4",
                "pattern": "checker",
                "patternColor": "#FFFFFF",
                "patternSize": 2,
                "patternOpacity": 70
            }"##,
        )
        .expect("shape must parse");
        assert_eq!(dto.base_color, "#0078D4");
        assert_eq!(dto.pattern, "checker");
        assert_eq!(dto.pattern_size, 2);

        // The full pipeline runs through the same DTO path.
        let preview = folder_icon_preview_svg(dto).expect("builds");
        assert!(preview.svg.starts_with("<svg"));
        assert!(preview.warnings.is_empty());
        // The small previews arrive as PNG data URLs for every size.
        assert_eq!(
            preview
                .frames
                .iter()
                .map(|frame| frame.size)
                .collect::<Vec<_>>(),
            vec![16, 32, 48]
        );
        assert!(
            preview
                .frames
                .iter()
                .all(|frame| frame.data_url.starts_with("data:image/png;base64,"))
        );
    }

    #[test]
    fn invalid_input_becomes_a_friendly_error() {
        let dto: FolderIconSettingsDto = serde_json::from_str(
            r##"{
                "baseColor": "blue",
                "pattern": "none",
                "patternColor": "#FFFFFF",
                "patternSize": 3,
                "patternOpacity": 50
            }"##,
        )
        .unwrap();
        let error = folder_icon_preview_svg(dto).unwrap_err();
        assert!(error.to_string().contains("#RRGGBB"));
    }

    #[test]
    fn low_contrast_pattern_warns_through_the_dto_path() {
        let dto: FolderIconSettingsDto = serde_json::from_str(
            r##"{
                "baseColor": "#0067C0",
                "pattern": "horizontal",
                "patternColor": "#0055AA",
                "patternSize": 1,
                "patternOpacity": 100
            }"##,
        )
        .unwrap();
        let preview = folder_icon_preview_svg(dto).expect("builds");
        assert!(preview.warnings.iter().any(|w| w.contains("hard to see")));
    }
}
