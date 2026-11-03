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
}

/// Builds the folder icon SVG for the live preview. The returned string is
/// generated entirely from validated colors and our own geometry.
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
    Ok(FolderIconPreview {
        svg: built.svg,
        warnings: built.warnings,
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
