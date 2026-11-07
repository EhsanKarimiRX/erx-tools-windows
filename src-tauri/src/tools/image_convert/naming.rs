//! Output file naming rules.
//!
//! Every output gets an "ERX" suffix before the extension: `photo.jpg` →
//! `photoERX.png`, ALWAYS — even when nothing collides, and even when the
//! stem already ends in "ERX" (it may belong to another project and mean
//! something to its owner, so it is never stripped or detected). When the
//! name exists it counts up: `photoERX (1).png`, `photoERX (2).png`, …
//! Names are sanitized so output can never escape the chosen folder or
//! contain characters Windows rejects.

use std::path::{Path, PathBuf};

use super::core::ConvertOptions;

/// Suffix appended to every output stem, before the extension.
const OUTPUT_SUFFIX: &str = "ERX";

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

/// Picks the output path for one converted file: `stemERX.ext`, or the first
/// free `stemERX (n).ext`. `exists` is injected so tests can simulate
/// collisions without a real file system. This check is only a fast
/// pre-check; the write itself claims the name with `create_new` (see
/// `pipeline`), so nothing existing can be overwritten even under races.
pub fn choose_output_path(
    source: &Path,
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
    let base = format!("{stem}{OUTPUT_SUFFIX}");

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

    fn jpeg_options(folder: Option<&str>) -> ConvertOptions {
        ConvertOptions {
            format: OutputFormat::Jpeg,
            ..options(folder)
        }
    }

    fn never(_path: &Path) -> bool {
        false
    }

    #[test]
    fn always_appends_the_erx_suffix() {
        let path =
            choose_output_path(Path::new("C:/pics/photo.jpg"), &options(None), never).unwrap();
        assert_eq!(path, PathBuf::from("C:/pics/photoERX.png"));
    }

    #[test]
    fn same_format_recompression_also_gets_the_suffix() {
        let path =
            choose_output_path(Path::new("C:/pics/photo.jpg"), &jpeg_options(None), never).unwrap();
        assert_eq!(path, PathBuf::from("C:/pics/photoERX.jpg"));
    }

    #[test]
    fn collisions_count_upwards() {
        let taken = [
            PathBuf::from("C:/pics/photoERX.png"),
            PathBuf::from("C:/pics/photoERX (1).png"),
        ];
        let path = choose_output_path(Path::new("C:/pics/photo.jpg"), &options(None), |p| {
            taken.iter().any(|candidate| candidate.as_path() == p)
        })
        .unwrap();
        assert_eq!(path, PathBuf::from("C:/pics/photoERX (2).png"));
    }

    #[test]
    fn same_format_collisions_count_upwards() {
        let path = choose_output_path(Path::new("C:/pics/photo.jpg"), &jpeg_options(None), |p| {
            p.to_str().unwrap().ends_with("photoERX.jpg")
        })
        .unwrap();
        assert_eq!(path, PathBuf::from("C:/pics/photoERX (1).jpg"));
    }

    #[test]
    fn custom_folder_wins_over_source_folder() {
        let path = choose_output_path(
            Path::new("C:/pics/photo.jpg"),
            &options(Some("D:/out")),
            never,
        )
        .unwrap();
        assert_eq!(path, PathBuf::from("D:/out/photoERX.png"));
    }

    /// "ERX" at the end of a source name may belong to another project, so it
    /// is never stripped or detected: the suffix simply stacks.
    #[test]
    fn stem_already_ending_in_erx_gets_another_erx() {
        let path =
            choose_output_path(Path::new("C:/pics/photoERX.jpg"), &options(None), never).unwrap();
        assert_eq!(path, PathBuf::from("C:/pics/photoERXERX.png"));

        let path = choose_output_path(Path::new("C:/pics/ERX.jpg"), &options(None), never).unwrap();
        assert_eq!(path, PathBuf::from("C:/pics/ERXERX.png"));
    }

    /// Sanitizing happens first, the suffix is appended after it — so it is
    /// never trimmed or truncated, even for dirty or capped stems.
    #[test]
    fn sanitized_stem_still_ends_with_the_suffix() {
        let path = choose_output_path(Path::new("C:/pics/img<1>:\"x?.png"), &options(None), never)
            .unwrap();
        assert!(path.to_str().unwrap().ends_with("img_1___x_ERX.png"));

        let long = format!("{}.png", "a".repeat(500));
        let path = choose_output_path(Path::new(&long), &options(None), never).unwrap();
        assert!(path.to_str().unwrap().ends_with("ERX.png"));
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        assert_eq!(stem.chars().count(), MAX_STEM_CHARS + OUTPUT_SUFFIX.len());
    }

    /// With the always-append rule the output can only collide with the
    /// source itself when the ERX-named file already exists; the count-up
    /// handles that like any other collision.
    #[test]
    fn output_colliding_with_the_source_name_counts_up() {
        let taken = [
            PathBuf::from("C:/pics/photoERX.jpg"), // the source file itself
            PathBuf::from("C:/pics/photoERXERX.jpg"), // the natural first output name
        ];
        let path = choose_output_path(
            Path::new("C:/pics/photoERX.jpg"),
            &jpeg_options(None),
            |p| taken.iter().any(|candidate| candidate.as_path() == p),
        )
        .unwrap();
        assert_eq!(path, PathBuf::from("C:/pics/photoERXERX (1).jpg"));
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
