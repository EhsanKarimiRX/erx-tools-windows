//! File-level pipeline: inspect → pick output name → convert → write.
//!
//! This is the unit 2C will fan out across worker threads; keeping it here
//! (no Tauri types) keeps the whole flow unit-testable.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use super::convert::convert_stream;
use super::core::{ConvertOptions, Inspection, inspect_path};
use super::naming::choose_output_path;

/// What one successful conversion produced.
#[derive(Debug)]
pub struct ConvertOutcome {
    pub output_path: PathBuf,
    pub output_size: u64,
}

/// Converts a single file end to end. The original is never touched and the
/// output path never collides (`name.newext`, `name (1).newext`, …).
///
/// Error strings are short, user-friendly English reasons (no full paths —
/// at most the file name).
pub fn convert_one_file(source: &Path, options: &ConvertOptions) -> Result<ConvertOutcome, String> {
    if !source.is_absolute() {
        return Err("file path is not absolute".into());
    }
    let info = match inspect_path(source) {
        Inspection::Image(info) => info,
        Inspection::NotSupported { detected } => {
            return Err(match detected {
                Some(name) => format!("{name} is not supported in v1"),
                None => "not a recognized image".to_string(),
            });
        }
        Inspection::Invalid { reason } => return Err(reason),
    };

    let same_format = info.format.matches_output(options.format);
    // The stem is sanitized, so `join` can never escape the output folder.
    let output_path =
        choose_output_path(source, same_format, options, |candidate| candidate.exists())?;

    let file = match File::open(source) {
        Ok(file) => file,
        Err(_) => return Err("cannot open the file".into()),
    };
    let data = convert_stream(BufReader::new(file), info.animated, options)?;
    if std::fs::write(&output_path, &data).is_err() {
        let name = output_path.file_name().map_or_else(
            || "output".to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        return Err(format!("could not write the output file \"{name}\""));
    }

    Ok(ConvertOutcome {
        output_path,
        output_size: data.len() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::image_convert::core::{OutputFormat, RgbColor};
    use image::GenericImageView;
    use std::io::Write;

    /// Fresh per-test directory under the system temp dir.
    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("erx-convert-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        let mut file = File::create(&path).unwrap();
        file.write_all(bytes).unwrap();
        path
    }

    /// A 3×2 PNG with a red top row and a green bottom row.
    fn small_png() -> Vec<u8> {
        let image = image::DynamicImage::ImageRgba8(image::ImageBuffer::from_fn(3, 2, |_, y| {
            if y == 0 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 255, 0, 255])
            }
        }));
        let mut out = std::io::Cursor::new(Vec::new());
        image.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn jpeg_options() -> ConvertOptions {
        ConvertOptions {
            format: OutputFormat::Jpeg,
            jpeg_quality: 85,
            background: RgbColor(255, 255, 255),
            output_folder: None,
        }
    }

    #[test]
    fn converts_into_the_source_folder_without_touching_the_original() {
        let dir = temp_dir("same-folder");
        let source = write_file(&dir, "photo.png", &small_png());
        let original_bytes = std::fs::read(&source).unwrap();

        let outcome = convert_one_file(&source, &jpeg_options()).unwrap();
        assert_eq!(outcome.output_path, dir.join("photo.jpg"));
        assert!(outcome.output_size > 0);
        // Original untouched, output exists and decodes.
        assert_eq!(std::fs::read(&source).unwrap(), original_bytes);
        let decoded = image::open(&outcome.output_path).unwrap();
        assert_eq!(decoded.dimensions(), (3, 2));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn never_overwrites_and_counts_upwards() {
        let dir = temp_dir("collisions");
        let source = write_file(&dir, "photo.png", &small_png());
        write_file(&dir, "photo.jpg", b"pre-existing output");

        let outcome = convert_one_file(&source, &jpeg_options()).unwrap();
        assert_eq!(outcome.output_path, dir.join("photo (1).jpg"));
        // The pre-existing file is not the conversion result.
        assert_eq!(
            std::fs::read(dir.join("photo.jpg")).unwrap(),
            b"pre-existing output"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn same_format_recompression_uses_converted_suffix() {
        let dir = temp_dir("same-format");
        let source = write_file(&dir, "photo.png", &small_png());

        let options = ConvertOptions {
            format: OutputFormat::Png,
            ..jpeg_options()
        };
        let outcome = convert_one_file(&source, &options).unwrap();
        assert_eq!(outcome.output_path, dir.join("photo-converted.png"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn custom_folder_receives_the_output() {
        let dir = temp_dir("custom-folder");
        let out_dir = temp_dir("custom-folder-out");
        let source = write_file(&dir, "عکس تست.png", &small_png());

        let options = ConvertOptions {
            output_folder: Some(out_dir.clone()),
            ..jpeg_options()
        };
        let outcome = convert_one_file(&source, &options).unwrap();
        // Persian file name survives; nothing lands next to the original.
        assert_eq!(outcome.output_path, out_dir.join("عکس تست.jpg"));
        assert!(outcome.output_path.exists());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1); // only the source
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&out_dir).unwrap();
    }

    #[test]
    fn unsupported_and_missing_files_fail_with_reasons() {
        let dir = temp_dir("errors");
        let text = write_file(&dir, "notes.txt", b"not an image");
        assert!(
            convert_one_file(&text, &jpeg_options())
                .unwrap_err()
                .contains("not a recognized image")
        );

        let missing = dir.join("missing.png");
        assert!(
            convert_one_file(&missing, &jpeg_options())
                .unwrap_err()
                .contains("not found")
        );

        let relative = PathBuf::from("relative.png");
        assert!(
            convert_one_file(&relative, &jpeg_options())
                .unwrap_err()
                .contains("not absolute")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
