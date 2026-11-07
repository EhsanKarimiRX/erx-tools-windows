//! File-level pipeline: inspect → pick output name → convert → write.
//!
//! This is the unit 2C will fan out across worker threads; keeping it here
//! (no Tauri types) keeps the whole flow unit-testable.

use std::fs::File;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};

use super::convert::{convert_heic, convert_stream};
use super::core::{ConvertOptions, InputFormat, Inspection, inspect_path};
use super::naming::choose_output_path;

/// Upper bound for output-name attempts when `create_new` keeps reporting
/// the name as taken. The count-up pre-check normally lands on a free name
/// immediately; the bound only matters under heavy races.
const MAX_NAME_TRIES: usize = 10_000;

/// What one successful conversion produced.
#[derive(Debug)]
pub struct ConvertOutcome {
    pub output_path: PathBuf,
    pub output_size: u64,
}

/// Converts a single file end to end. The original is never touched and no
/// existing file is ever overwritten: the output claims its name with
/// `create_new` (`nameERX.ext`, `nameERX (1).ext`, …), which is safe even
/// when several workers pick names at the same moment.
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
        Inspection::Failed { reason } => return Err(reason),
        Inspection::Invalid { reason } => return Err(reason),
    };

    // The stem is sanitized, so `join` can never escape the output folder.
    // This first pick is only a fast pre-check; the write below is the
    // source of truth.
    let mut output_path = choose_output_path(source, options, |candidate| candidate.exists())?;

    // HEIC goes through its own path-based decoder (Windows WIC) instead of
    // the byte-stream converters; everything else decodes from the stream.
    let data = if info.format == InputFormat::Heic {
        convert_heic(source, options)?
    } else {
        let file = match File::open(source) {
            Ok(file) => file,
            Err(_) => return Err("cannot open the file".into()),
        };
        convert_stream(BufReader::new(file), info.animated, options)?
    };

    let mut tries = 0;
    let output_path = loop {
        tries += 1;
        if tries > MAX_NAME_TRIES {
            let name = output_path.file_name().map_or_else(
                || "output".to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            return Err(format!("could not find a free output name for \"{name}\""));
        }
        match write_exclusive(&output_path, &data) {
            Ok(()) => break output_path,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                // Another worker (or an earlier file) took the name after
                // the pre-check. Its file now exists, so re-running the
                // pre-check counts past it.
                output_path = choose_output_path(source, options, |candidate| candidate.exists())?;
            }
            Err(_) => {
                let name = output_path.file_name().map_or_else(
                    || "output".to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                return Err(format!("could not write the output file \"{name}\""));
            }
        }
    };

    Ok(ConvertOutcome {
        output_path,
        output_size: data.len() as u64,
    })
}

/// Creates `path` exclusively and writes `data` into it. `create_new` means
/// an existing file is never opened for writing, so nothing on disk can be
/// overwritten — including files another worker creates between the name
/// pre-check and this call. If writing fails after creation (disk full, …)
/// the just-created partial file is removed; it can only be a file this
/// call created, so no pre-existing file is ever deleted.
fn write_exclusive(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    if let Err(error) = file.write_all(data) {
        let _ = std::fs::remove_file(path);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::image_convert::core::{OutputFormat, RgbColor};
    use image::GenericImageView;

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
        assert_eq!(outcome.output_path, dir.join("photoERX.jpg"));
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
        write_file(&dir, "photoERX.jpg", b"first existing output");
        write_file(&dir, "photoERX (1).jpg", b"second existing output");

        let outcome = convert_one_file(&source, &jpeg_options()).unwrap();
        assert_eq!(outcome.output_path, dir.join("photoERX (2).jpg"));
        // Both pre-existing files still hold their original contents.
        assert_eq!(
            std::fs::read(dir.join("photoERX.jpg")).unwrap(),
            b"first existing output"
        );
        assert_eq!(
            std::fs::read(dir.join("photoERX (1).jpg")).unwrap(),
            b"second existing output"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn pre_existing_output_is_left_byte_for_byte_untouched() {
        let dir = temp_dir("pre-existing");
        let source = write_file(&dir, "photo.png", &small_png());
        write_file(&dir, "photoERX.jpg", b"precious bytes");
        let before = std::fs::read(dir.join("photoERX.jpg")).unwrap();

        let outcome = convert_one_file(&source, &jpeg_options()).unwrap();
        assert_eq!(outcome.output_path, dir.join("photoERX (1).jpg"));
        assert_eq!(std::fs::read(dir.join("photoERX.jpg")).unwrap(), before);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn same_format_recompression_gets_the_erx_suffix() {
        let dir = temp_dir("same-format");
        let source = write_file(&dir, "photo.png", &small_png());

        let options = ConvertOptions {
            format: OutputFormat::Png,
            ..jpeg_options()
        };
        let outcome = convert_one_file(&source, &options).unwrap();
        assert_eq!(outcome.output_path, dir.join("photoERX.png"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Mirrors the batch worker pool: several threads convert same-stem
    /// files into one folder at the same moment. `create_new` must keep
    /// every output alive — none may be lost or overwrite another.
    #[test]
    fn concurrent_same_stem_outputs_never_overwrite_each_other() {
        let out_dir = temp_dir("race-out");
        let options = ConvertOptions {
            format: OutputFormat::Png,
            output_folder: Some(out_dir.clone()),
            ..jpeg_options()
        };
        let sources: Vec<PathBuf> = (0..6)
            .map(|i| {
                write_file(
                    &temp_dir(&format!("race-src-{i}")),
                    "photo.png",
                    &small_png(),
                )
            })
            .collect();

        // A barrier releases all threads together, so their pick-name →
        // write sequences overlap as much as the scheduler allows.
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(sources.len()));
        let options_ref = &options;
        let results: Vec<Result<ConvertOutcome, String>> = std::thread::scope(|scope| {
            let handles: Vec<_> = sources
                .iter()
                .map(|source| {
                    let barrier = std::sync::Arc::clone(&barrier);
                    scope.spawn(move || {
                        barrier.wait();
                        convert_one_file(source, options_ref)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });

        for result in &results {
            result.as_ref().expect("every conversion should succeed");
        }
        let mut names: Vec<String> = results
            .iter()
            .map(|result| {
                result
                    .as_ref()
                    .unwrap()
                    .output_path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names.dedup();
        // Every output is distinct, and together they are exactly the base
        // name plus the count-up names — nothing was lost or merged.
        let mut expected: Vec<String> = std::iter::once("photoERX.png".to_string())
            .chain((1..sources.len() as u32).map(|n| format!("photoERX ({n}).png")))
            .collect();
        expected.sort();
        assert_eq!(names.len(), sources.len(), "distinct outputs: {names:?}");
        assert_eq!(names, expected);
        for name in &names {
            assert!(out_dir.join(name).is_file(), "{name} is missing");
        }
        assert_eq!(std::fs::read_dir(&out_dir).unwrap().count(), sources.len());

        for source in &sources {
            let _ = std::fs::remove_dir_all(source.parent().unwrap());
        }
        let _ = std::fs::remove_dir_all(&out_dir);
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
        assert_eq!(outcome.output_path, out_dir.join("عکس تستERX.jpg"));
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

    /// A file that sniffs as HEIC but is garbage inside: the real WIC path
    /// fails deterministically with the honest message.
    fn heic_garbage_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(168u32.to_be_bytes());
        bytes.extend_from_slice(b"ftyp");
        bytes.extend_from_slice(b"heic");
        bytes.extend_from_slice(b"\0\0\0\0");
        bytes.extend([0u8; 24]);
        bytes.extend([0xA5u8; 128]);
        bytes
    }

    /// One failing HEIC must never stop its neighbors, and results stay in
    /// list order (the batch command gives every file its own result slot).
    #[test]
    fn heic_failures_do_not_stop_neighbors() {
        let dir = temp_dir("heic-batch");
        let broken1 = write_file(&dir, "broken1.heic", &heic_garbage_bytes());
        let good = write_file(&dir, "good.png", &small_png());
        let broken2 = write_file(&dir, "broken2.heic", &heic_garbage_bytes());

        let results: Vec<Result<ConvertOutcome, String>> = [&broken1, &good, &broken2]
            .iter()
            .map(|path| convert_one_file(path, &jpeg_options()))
            .collect();

        assert!(
            results[0].as_ref().unwrap_err().contains("Windows error"),
            "{:?}",
            results[0]
        );
        assert_eq!(
            results[1].as_ref().unwrap().output_path,
            dir.join("goodERX.jpg")
        );
        assert!(
            results[2].as_ref().unwrap_err().contains("Windows error"),
            "{:?}",
            results[2]
        );
        assert!(dir.join("goodERX.jpg").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
