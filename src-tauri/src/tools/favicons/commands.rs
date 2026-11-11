//! Tauri command layer for Favicons. Thin by design: DTOs and data URLs
//! here, the real work in the pure `package`/`render`/`source` modules.
//! Native dialogs run on the Rust side so the webview needs no permission
//! beyond `core:default`. All commands are `#[tauri::command(async)]` so
//! heavy image work never runs on the UI thread.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

use super::package::{FaviconsOptions, Package, build_package, encode_png, write_package};
use super::render::{make_square, resize_square};
use super::source::{LoadedSource, SourceKind, load_source};
use crate::tools::ToolError;

// ---------------------------------------------------------------------------
// DTOs (all camelCase on the wire)
// ---------------------------------------------------------------------------

/// Options as sent by the UI (unvalidated; the core validates colors and
/// padding).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaviconsOptionsDto {
    pub site_name: String,
    pub padding_percent: f32,
    pub app_padding_percent: f32,
    pub trim_transparent: bool,
    pub background_color: String,
    pub theme_color: String,
    pub include_maskable: bool,
}

/// The loaded source as shown in the source card.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfoDto {
    pub path: String,
    pub file_name: String,
    /// "raster" | "svg"
    pub kind: String,
    pub format_label: String,
    pub width: u32,
    pub height: u32,
    pub warnings: Vec<String>,
    /// PNG data URL of the thumbnail (longest side max 256 px).
    pub preview_data_url: String,
}

/// One generated PNG in the live preview grid.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IconPreviewDto {
    pub file_name: String,
    pub width: u32,
    pub height: u32,
    pub size_bytes: u64,
    pub data_url: String,
}

/// The full live preview: every PNG icon plus snippet and warnings.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewDto {
    pub icons: Vec<IconPreviewDto>,
    pub html_snippet: String,
    pub warnings: Vec<String>,
}

/// One written file, reported after saving.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedFileDto {
    pub name: String,
    pub size_bytes: u64,
}

/// The save result: created folder, all files, snippet, warnings.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveResultDto {
    pub folder: String,
    pub files: Vec<SavedFileDto>,
    pub html_snippet: String,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Pure helpers (no Tauri types)
// ---------------------------------------------------------------------------

/// Plain field copy from the DTO into the validated core options.
fn options_from_dto(dto: FaviconsOptionsDto) -> FaviconsOptions {
    FaviconsOptions {
        site_name: dto.site_name,
        padding_percent: dto.padding_percent,
        app_padding_percent: dto.app_padding_percent,
        trim_transparent: dto.trim_transparent,
        background_color: dto.background_color,
        theme_color: dto.theme_color,
        include_maskable: dto.include_maskable,
    }
}

/// Standard base64 PNG data URL.
fn png_data_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", STANDARD.encode(png))
}

/// Thumbnail of the master for the source card: squared, scaled so the
/// longest side is at most 256 px (never upscaled), as a PNG data URL.
fn make_thumbnail_data_url(master: &RgbaImage) -> Result<String, String> {
    let side = master.width().max(master.height());
    let size = 256.min(side);
    let thumbnail = resize_square(&make_square(master), size);
    let png = encode_png(&thumbnail)?;
    Ok(png_data_url(&png))
}

/// Builds the source-card DTO from a loaded source.
fn build_source_info(
    path: &std::path::Path,
    loaded: &LoadedSource,
) -> Result<SourceInfoDto, String> {
    let file_name = path.file_name().map_or_else(
        || path.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    Ok(SourceInfoDto {
        path: path.to_string_lossy().into_owned(),
        file_name,
        kind: match loaded.kind {
            SourceKind::Raster => "raster".to_string(),
            SourceKind::Svg => "svg".to_string(),
        },
        format_label: loaded.format_label.clone(),
        width: loaded.original_width,
        height: loaded.original_height,
        warnings: loaded.warnings.clone(),
        preview_data_url: make_thumbnail_data_url(&loaded.master)?,
    })
}

/// Builds the preview DTO: every generated PNG (width and height set) with
/// its data URL; .ico/.svg/.webmanifest/.html are not part of the icons.
fn build_preview(package: &Package) -> Result<PreviewDto, String> {
    let icons = package
        .files
        .iter()
        .filter_map(|file| {
            let (width, height) = match (file.width, file.height) {
                (Some(width), Some(height)) => (width, height),
                _ => return None,
            };
            Some(IconPreviewDto {
                file_name: file.name.clone(),
                width,
                height,
                size_bytes: file.bytes.len() as u64,
                data_url: png_data_url(&file.bytes),
            })
        })
        .collect();
    Ok(PreviewDto {
        icons,
        html_snippet: package.html_snippet.clone(),
        warnings: package.warnings.clone(),
    })
}

/// One SavedFileDto per package file (including .ico/.svg/manifest/html).
fn saved_files(package: &Package) -> Vec<SavedFileDto> {
    package
        .files
        .iter()
        .map(|file| SavedFileDto {
            name: file.name.clone(),
            size_bytes: file.bytes.len() as u64,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Opens the native single-file dialog for the logo image (Rust side, so
/// the webview needs no dialog permission). Returns the absolute path or
/// None on cancel.
#[tauri::command(async)]
pub fn favicons_pick_source(app: AppHandle) -> Option<String> {
    let picked = app
        .dialog()
        .file()
        .set_title("Choose your logo")
        .add_filter(
            "Images",
            &[
                "png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff", "ico", "svg",
            ],
        )
        .blocking_pick_file();
    picked
        .and_then(|file| file.into_path().ok())
        .map(|path| path.to_string_lossy().into_owned())
}

/// Loads the source and returns the source-card info (including the
/// thumbnail data URL and warnings).
#[tauri::command(async)]
pub fn favicons_inspect_source(path: String) -> Result<SourceInfoDto, ToolError> {
    let loaded = load_source(std::path::Path::new(&path)).map_err(ToolError::Internal)?;
    build_source_info(std::path::Path::new(&path), &loaded).map_err(ToolError::Internal)
}

/// Loads the source, builds the whole package and returns the preview DTO.
#[tauri::command(async)]
pub fn favicons_preview(
    path: String,
    options: FaviconsOptionsDto,
) -> Result<PreviewDto, ToolError> {
    let source = load_source(std::path::Path::new(&path)).map_err(ToolError::Internal)?;
    let package =
        build_package(&source, &options_from_dto(options)).map_err(ToolError::Internal)?;
    build_preview(&package).map_err(ToolError::Internal)
}

/// Loads the source and builds the package FIRST (so errors appear before
/// any dialog), then asks for the output folder, writes the package and
/// reports what was created. Dialog cancelled or empty -> Ok(None).
#[tauri::command(async)]
pub fn favicons_save(
    app: AppHandle,
    path: String,
    options: FaviconsOptionsDto,
) -> Result<Option<SaveResultDto>, ToolError> {
    let source = load_source(std::path::Path::new(&path)).map_err(ToolError::Internal)?;
    let options = options_from_dto(options);
    let package = build_package(&source, &options).map_err(ToolError::Internal)?;

    let target = app
        .dialog()
        .file()
        .set_title("Choose where to save the favicon package")
        .blocking_pick_folder();
    let Some(target) = target else {
        return Ok(None); // cancelled — nothing written
    };
    let parent = target
        .into_path()
        .map_err(|_| ToolError::Internal("The chosen save location is not valid.".into()))?;
    if parent.as_os_str().is_empty() {
        return Ok(None); // nothing chosen — never write
    }

    let folder =
        write_package(&parent, &options.site_name, &package).map_err(ToolError::Internal)?;
    Ok(Some(SaveResultDto {
        folder: folder.to_string_lossy().into_owned(),
        files: saved_files(&package),
        html_snippet: package.html_snippet.clone(),
        warnings: package.warnings.clone(),
    }))
}

/// Shows the created package in Windows Explorer: the favicon.ico inside
/// it when present, the folder itself otherwise.
#[tauri::command(async)]
pub fn favicons_reveal_folder(folder: String) -> Result<(), ToolError> {
    let folder_path = std::path::Path::new(&folder);
    if !folder_path.is_dir() {
        return Err(ToolError::Internal("The folder no longer exists.".into()));
    }
    let target = folder_path.join("favicon.ico");
    let target = if target.is_file() {
        target
    } else {
        folder_path.to_path_buf()
    };
    if tauri_plugin_opener::reveal_item_in_dir(&target).is_err() {
        return Err(ToolError::Internal(
            "Windows refused to open the folder.".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    use image::RgbaImage;

    fn solid(width: u32, height: u32, rgba: [u8; 4]) -> RgbaImage {
        let mut img = RgbaImage::new(width, height);
        for p in img.pixels_mut() {
            *p = Rgba(rgba);
        }
        img
    }

    fn raster_source(width: u32, height: u32) -> LoadedSource {
        LoadedSource {
            kind: SourceKind::Raster,
            format_label: "PNG".to_string(),
            original_width: width,
            original_height: height,
            master: solid(width, height, [255, 0, 0, 255]),
            svg_bytes: None,
            warnings: Vec::new(),
        }
    }

    fn decode_data_url_png(url: &str) -> RgbaImage {
        let encoded = url
            .strip_prefix("data:image/png;base64,")
            .expect("data URL prefix");
        let bytes = STANDARD.decode(encoded).expect("base64");
        image::load_from_memory(&bytes).expect("decodes").to_rgba8()
    }

    #[test]
    fn options_dto_parses_and_copies_every_field() {
        let dto: FaviconsOptionsDto = serde_json::from_str(
            r##"{"siteName":"My Site","paddingPercent":12.5,"appPaddingPercent":7.5,"trimTransparent":false,"backgroundColor":"#112233","themeColor":"#abc","includeMaskable":false}"##,
        )
        .expect("parses");
        let options = options_from_dto(dto);
        assert_eq!(options.site_name, "My Site");
        assert!((options.padding_percent - 12.5).abs() < 1e-6);
        assert!((options.app_padding_percent - 7.5).abs() < 1e-6);
        assert!(!options.trim_transparent);
        assert_eq!(options.background_color, "#112233");
        assert_eq!(options.theme_color, "#abc");
        assert!(!options.include_maskable);
    }

    #[test]
    fn options_dto_requires_every_field() {
        let missing_site_name = r##"{"paddingPercent":0,"appPaddingPercent":0,"trimTransparent":true,"backgroundColor":"#ffffff","themeColor":"#ffffff","includeMaskable":true}"##;
        assert!(
            serde_json::from_str::<FaviconsOptionsDto>(missing_site_name).is_err(),
            "a missing field must fail to deserialize"
        );
    }

    #[test]
    fn options_dto_requires_app_padding_percent() {
        let missing_app_padding = r##"{"siteName":"x","paddingPercent":0,"trimTransparent":true,"backgroundColor":"#ffffff","themeColor":"#ffffff","includeMaskable":true}"##;
        assert!(
            serde_json::from_str::<FaviconsOptionsDto>(missing_app_padding).is_err(),
            "appPaddingPercent is required, no serde default"
        );
    }

    #[test]
    fn png_data_url_round_trips_the_bytes() {
        let bytes: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3];
        let url = png_data_url(&bytes);
        assert!(url.starts_with("data:image/png;base64,"));
        let decoded = STANDARD
            .decode(url.strip_prefix("data:image/png;base64,").expect("prefix"))
            .expect("decodes");
        assert_eq!(decoded, bytes);
    }

    #[test]
    fn thumbnail_is_capped_at_256_px_without_upscaling() {
        let url = make_thumbnail_data_url(&solid(600, 400, [255, 0, 0, 255])).expect("thumbnail");
        let decoded = decode_data_url_png(&url);
        assert_eq!((decoded.width(), decoded.height()), (256, 256));

        let url = make_thumbnail_data_url(&solid(100, 100, [255, 0, 0, 255])).expect("thumbnail");
        let decoded = decode_data_url_png(&url);
        assert_eq!((decoded.width(), decoded.height()), (100, 100));
    }

    #[test]
    fn source_info_json_uses_camel_case_keys() {
        let mut loaded = raster_source(64, 32);
        loaded.warnings = vec!["The source is small (64x32 px).".to_string()];
        let info =
            build_source_info(std::path::Path::new(r"C:\logos\my.png"), &loaded).expect("info");
        let value = serde_json::to_value(&info).expect("serializes");
        let object = value.as_object().expect("object");
        for key in [
            "path",
            "fileName",
            "kind",
            "formatLabel",
            "width",
            "height",
            "warnings",
            "previewDataUrl",
        ] {
            assert!(object.contains_key(key), "missing key {key}");
        }
        assert!(!object.contains_key("file_name"), "no snake_case keys");
        assert!(!object.contains_key("preview_data_url"));
        assert_eq!(object["kind"], "raster");
        assert_eq!(info.file_name, "my.png");
    }

    #[test]
    fn source_info_kind_is_svg_for_svg_sources() {
        let loaded = LoadedSource {
            kind: SourceKind::Svg,
            format_label: "SVG".to_string(),
            original_width: 100,
            original_height: 100,
            master: solid(100, 100, [255, 0, 0, 255]),
            svg_bytes: Some(b"<svg/>".to_vec()),
            warnings: Vec::new(),
        };
        let info = build_source_info(std::path::Path::new("logo.svg"), &loaded).expect("info");
        assert_eq!(info.kind, "svg");
    }

    #[test]
    fn preview_lists_the_png_icons_in_order() {
        let package =
            build_package(&raster_source(600, 400), &FaviconsOptions::default()).expect("builds");
        let preview = build_preview(&package).expect("preview");

        let names: Vec<&str> = preview.icons.iter().map(|i| i.file_name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "favicon-16x16.png",
                "favicon-32x32.png",
                "apple-touch-icon.png",
                "android-chrome-192x192.png",
                "android-chrome-512x512.png",
                "android-chrome-maskable-512x512.png",
            ]
        );
        let expected_sizes = [
            (16, 16),
            (32, 32),
            (180, 180),
            (192, 192),
            (512, 512),
            (512, 512),
        ];
        for (icon, (w, h)) in preview.icons.iter().zip(expected_sizes) {
            assert_eq!((icon.width, icon.height), (w, h), "{}", icon.file_name);
            let decoded = decode_data_url_png(&icon.data_url);
            assert_eq!(
                (decoded.width(), decoded.height()),
                (w, h),
                "{}",
                icon.file_name
            );
        }
        assert!(!preview.html_snippet.is_empty());

        let value = serde_json::to_value(&preview).expect("serializes");
        let first_icon = &value["icons"][0];
        assert!(first_icon.get("fileName").is_some());
        assert!(first_icon.get("sizeBytes").is_some());
        assert!(first_icon.get("dataUrl").is_some());
        assert!(value.get("htmlSnippet").is_some());
    }

    #[test]
    fn preview_skips_the_maskable_icon_when_disabled() {
        let mut options = FaviconsOptions::default();
        options.include_maskable = false;
        let package = build_package(&raster_source(500, 500), &options).expect("builds");
        let preview = build_preview(&package).expect("preview");
        assert_eq!(preview.icons.len(), 5);
        assert!(
            !preview
                .icons
                .iter()
                .any(|i| i.file_name.contains("maskable"))
        );
    }

    #[test]
    fn saved_files_cover_every_package_file() {
        let package =
            build_package(&raster_source(500, 500), &FaviconsOptions::default()).expect("builds");
        let saved = saved_files(&package);
        assert_eq!(saved.len(), package.files.len());
        for (entry, file) in saved.iter().zip(&package.files) {
            assert_eq!(entry.name, file.name);
            assert_eq!(entry.size_bytes, file.bytes.len() as u64);
        }
        let names: Vec<&str> = saved.iter().map(|s| s.name.as_str()).collect();
        for required in ["favicon.ico", "site.webmanifest", "favicon-snippet.html"] {
            assert!(names.contains(&required), "{required} missing");
        }
    }

    #[test]
    fn save_result_dto_uses_camel_case_keys() {
        let dto = SaveResultDto {
            folder: r"C:\out".to_string(),
            files: vec![SavedFileDto {
                name: "favicon.ico".to_string(),
                size_bytes: 5,
            }],
            html_snippet: "<link/>".to_string(),
            warnings: vec![],
        };
        let value = serde_json::to_value(&dto).expect("serializes");
        let object = value.as_object().expect("object");
        for key in ["folder", "files", "htmlSnippet", "warnings"] {
            assert!(object.contains_key(key), "missing key {key}");
        }
    }

    #[test]
    fn an_invalid_color_fails_in_build_package_before_any_preview() {
        let mut options = FaviconsOptions::default();
        options.background_color = "red".to_string();
        let result = build_package(&raster_source(500, 500), &options);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("color"));
    }
}
