//! Output file naming rules.
//!
//! Never overwrite an original: `name.newext` → `name (1).newext` → …, and
//! `name-converted.ext` for same-format re-compression. Names are sanitized
//! so output can never escape the chosen folder or contain characters
//! Windows rejects.

use std::path::{Path, PathBuf};

use super::core::ConvertOptions;

/// Windows reserved device names, invalid as (stem of a) file name.
const RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Cap for the output stem so long source names cannot produce paths beyond
/// what Windows accepts.
const MAX_STEM_CHARS: usize = 200;

/// Makes a source file stem safe as an output name part: replaces
/// Windows-forbidden characters and control characters, trims trailing
/// dots/spaces, caps the length and avoids reserved device names.
/// Persian/Unicode text is preserved untouched.
pub fn sanitize_stem(stem: &str) -> String {
    let cleaned: String = stem
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    let capped: String = if cleaned.chars().count() > MAX_STEM_CHARS {
        cleaned.chars().take(MAX_STEM_CHARS).collect()
    } else {
        cleaned
    };
    let trimmed = capped.trim_end_matches(['.', ' ']);
    let stem = if trimmed.is_empty() { "image" } else { trimmed };
    if RESERVED_NAMES
        .iter()
        .any(|name| stem.eq_ignore_ascii_case(name))
    {
        return format!("image-{stem}");
    }
    stem.to_string()
}

/// Picks the output path for one converted file.
///
/// `same_format` marks a re-compression (source format == output format) and
/// switches the base name to `name-converted`. `exists` is injected so tests
/// can simulate collisions without a real file system.
pub fn choose_output_path(
    source: &Path,
    same_format: bool,
    options: &ConvertOptions,
    exists: impl Fn(&Path) -> bool,
) -> Result<PathBuf, String> {
    let folder = match &options.output_folder {
        Some(folder) => folder.clone(),
        None => match source.parent() {
            // Source paths are absolute; "." is only a defensive fallback.
            Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
            _ => PathBuf::from("."),
        },
    };
    let raw_stem = source
        .file_stem()
        .map_or_else(|| "image".to_string(), |s| s.to_string_lossy().into_owned());
    let stem = sanitize_stem(&raw_stem);
    let base = if same_format {
        format!("{stem}-converted")
    } else {
        stem
    };

    let mut candidate = folder.join(format!("{base}.{}", options.format.extension()));
    let mut counter = 1;
    while exists(&candidate) {
        candidate = folder.join(format!("{base} ({counter}).{}", options.format.extension()));
        counter += 1;
    }
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::image_convert::core::{OutputFormat, RgbColor};

    fn options(folder: Option<&str>) -> ConvertOptions {
        ConvertOptions {
            format: OutputFormat::Png,
            jpeg_quality: 85,
            background: RgbColor(255, 255, 255),
            output_folder: folder.map(PathBuf::from),
        }
    }

    fn never(_path: &Path) -> bool {
        false
    }

    #[test]
    fn picks_sibling_name_with_new_extension() {
        let path = choose_output_path(Path::new("C:/pics/photo.jpg"), false, &options(None), never)
            .unwrap();
        assert_eq!(path, PathBuf::from("C:/pics/photo.png"));
    }

    #[test]
    fn same_format_appends_converted() {
        let path = choose_output_path(Path::new("C:/pics/photo.jpg"), true, &options(None), never)
            .unwrap();
        assert_eq!(path, PathBuf::from("C:/pics/photo-converted.png"));
    }

    #[test]
    fn collisions_count_upwards() {
        let taken = [
            PathBuf::from("C:/pics/photo.png"),
            PathBuf::from("C:/pics/photo (1).png"),
        ];
        let path = choose_output_path(Path::new("C:/pics/photo.jpg"), false, &options(None), |p| {
            taken.iter().any(|candidate| candidate.as_path() == p)
        })
        .unwrap();
        assert_eq!(path, PathBuf::from("C:/pics/photo (2).png"));
    }

    #[test]
    fn same_format_collisions_count_too() {
        let path = choose_output_path(Path::new("C:/pics/a.tif"), true, &options(None), |p| {
            p.to_str().unwrap().ends_with("a-converted.png")
        })
        .unwrap();
        assert_eq!(path, PathBuf::from("C:/pics/a-converted (1).png"));
    }

    #[test]
    fn custom_folder_wins_over_source_folder() {
        let path = choose_output_path(
            Path::new("C:/pics/photo.jpg"),
            false,
            &options(Some("D:/out")),
            never,
        )
        .unwrap();
        assert_eq!(path, PathBuf::from("D:/out/photo.png"));
    }

    #[test]
    fn sanitizes_forbidden_characters() {
        assert_eq!(sanitize_stem("img<1>:\"x?"), "img_1___x_");
        assert_eq!(sanitize_stem("trailing... "), "trailing");
        assert_eq!(sanitize_stem("عکس تست"), "عکس تست");
        assert_eq!(sanitize_stem(""), "image");
        assert_eq!(sanitize_stem("CON"), "image-CON");
        assert_eq!(sanitize_stem("com4"), "image-com4");
    }

    #[test]
    fn very_long_stems_are_capped() {
        let long = "a".repeat(500);
        assert_eq!(sanitize_stem(&long).chars().count(), MAX_STEM_CHARS);
    }
}
