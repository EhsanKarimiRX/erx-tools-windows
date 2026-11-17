//! File-level pipeline for ONE file: analyze it, write the cleaned copy to a
//! temporary file, check the copy, and move it to its final name without
//! ever replacing an existing file. The original is only read; it is never
//! changed or deleted.
//!
//! The check before the final name: the size must match the plan, the new
//! file is analyzed again by the same handler (nothing removable may be
//! left) and, for photos, the pixels are compared with the original through
//! a SHA-256 digest, so only one picture is in memory at a time.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use image::ImageReader;
use sha2::{Digest, Sha256};

use super::dispatch::{analyze_path, analyze_reader};
use super::executor::apply_plan;
use super::model::{Analysis, CleanOptions, FileFormat, Finding};
use super::safe_write::{
    STALE_TEMP_AGE, choose_output_path, cleanup_stale_temp, move_no_replace, random_token,
    temp_path,
};
use crate::tools::image_convert::core::MAX_PIXELS;

const WRITE_BUFFER: usize = 1 << 20;
/// Upper bound for output-name attempts when the name keeps being taken.
const MAX_MOVE_TRIES: u32 = 10_000;
/// Windows antivirus can briefly lock a freshly written file.
const MOVE_RETRIES: u32 = 5;
const MOVE_WAIT: Duration = Duration::from_millis(150);
const CHECK_FAILED: &str = "the cleaned copy did not pass the safety check, so it was not kept";

/// What one successful cleaning produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanedFile {
    pub output_path: PathBuf,
    pub output_size: u64,
    pub source_size: u64,
    pub format: FileFormat,
    /// The hidden data that was removed.
    pub removed: Vec<Finding>,
    /// Things that were looked at and deliberately kept.
    pub kept: Vec<String>,
    /// True when the pixels of the new file were compared with the original.
    pub pixels_verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileResult {
    Cleaned(CleanedFile),
    /// The file has no metadata: no new file was made.
    NothingToRemove {
        format: FileFormat,
        kept: Vec<String>,
    },
    Cancelled,
}

/// Deletes the temporary file unless `keep` was set.
struct TempGuard {
    path: PathBuf,
    keep: bool,
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn write_error(path: &Path) -> String {
    let name = path.file_name().map_or_else(
        || "output".to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    format!("could not write the output file \"{name}\"")
}

/// Width, height and SHA-256 of the RGBA pixels of a picture, or None when
/// it cannot be decoded or is larger than the pixel limit.
fn pixel_digest(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let first = ImageReader::new(BufReader::new(File::open(path).ok()?))
        .with_guessed_format()
        .ok()?;
    let (width, height) = first.into_dimensions().ok()?;
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return None;
    }
    let image = ImageReader::new(BufReader::new(File::open(path).ok()?))
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let rgba = image.into_rgba8();
    let digest = Sha256::digest(rgba.as_raw());
    Some((width, height, digest.as_slice().to_vec()))
}

/// Compares the pixels of the original and the cleaned copy. Ok(true) =
/// compared and equal; Ok(false) = not compared (a video, HEIC, a picture
/// the decoder cannot read, or one over the pixel limit); Err = the cleaned
/// copy cannot be read back or its pixels differ.
pub fn pixels_match(source: &Path, output: &Path, format: FileFormat) -> Result<bool, String> {
    if format.is_video() || format == FileFormat::Heic {
        return Ok(false);
    }
    let Some(original) = pixel_digest(source) else {
        return Ok(false);
    };
    let Some(cleaned) = pixel_digest(output) else {
        return Err("the cleaned image cannot be read back".to_string());
    };
    if original != cleaned {
        return Err("the pixels of the cleaned copy differ from the original".to_string());
    }
    Ok(true)
}

/// Writes the plan into a NEW temporary file and flushes it to disk.
/// Ok(None) = cancelled. All file handles are closed when this returns.
fn write_plan(
    source: &Path,
    analysis: &Analysis,
    temp: &Path,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> io::Result<Option<u64>> {
    let out_file = OpenOptions::new().write(true).create_new(true).open(temp)?;
    let mut input = BufReader::new(File::open(source)?);
    let mut writer = BufWriter::with_capacity(WRITE_BUFFER, out_file);
    let written = match apply_plan(&mut input, &analysis.plan, &mut writer, progress) {
        Ok(count) => count,
        Err(error) if error.kind() == io::ErrorKind::Interrupted => return Ok(None),
        Err(error) => return Err(error),
    };
    writer.flush()?;
    let file = writer.into_inner().map_err(|error| error.into_error())?;
    file.sync_all()?;
    Ok(Some(written))
}

/// Cleans one file. `out_folder` None = the folder of the original.
///
/// `progress(done, total)` counts written bytes; returning false cancels
/// (no file is left behind). The error strings are short, friendly English
/// reasons.
pub fn clean_one_file(
    source: &Path,
    out_folder: Option<&Path>,
    options: &CleanOptions,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<FileResult, String> {
    if !source.is_absolute() {
        return Err("file path is not absolute".to_string());
    }
    if let Some(folder) = out_folder
        && !folder.is_dir()
    {
        return Err("the chosen output folder does not exist".to_string());
    }
    let analysis = analyze_path(source, options)?;
    if !analysis.has_changes() {
        return Ok(FileResult::NothingToRemove {
            format: analysis.format,
            kept: analysis.kept,
        });
    }

    let no_name = || "could not find a free name for the new file".to_string();
    let mut candidate =
        choose_output_path(source, out_folder, |path| path.exists()).ok_or_else(no_name)?;
    let folder = candidate
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    cleanup_stale_temp(&folder, STALE_TEMP_AGE);

    let temp = temp_path(&candidate, &random_token()?);
    let written = match write_plan(source, &analysis, &temp, progress) {
        Ok(Some(count)) => count,
        Ok(None) => {
            let _ = fs::remove_file(&temp);
            return Ok(FileResult::Cancelled);
        }
        Err(error) => {
            // A name that already existed is not ours: leave it alone.
            if error.kind() != io::ErrorKind::AlreadyExists {
                let _ = fs::remove_file(&temp);
            }
            return Err(write_error(&candidate));
        }
    };
    let mut guard = TempGuard {
        path: temp.clone(),
        keep: false,
    };

    // The checks before the file gets its final name.
    if written != analysis.plan.output_len() {
        return Err(CHECK_FAILED.to_string());
    }
    let temp_len = fs::metadata(&temp)
        .map(|metadata| metadata.len())
        .map_err(|_| CHECK_FAILED.to_string())?;
    if temp_len != written {
        return Err(CHECK_FAILED.to_string());
    }
    let check_file = File::open(&temp).map_err(|_| CHECK_FAILED.to_string())?;
    match analyze_reader(&mut BufReader::new(check_file), written, options) {
        Ok(again) if !again.has_changes() => {}
        _ => return Err(CHECK_FAILED.to_string()),
    }
    let pixels_verified =
        pixels_match(source, &temp, analysis.format).map_err(|_| CHECK_FAILED.to_string())?;

    // The final name: Windows itself refuses an existing target.
    let mut tries = 0u32;
    let mut lock_retries = 0u32;
    let final_path = loop {
        tries += 1;
        if tries > MAX_MOVE_TRIES {
            return Err(no_name());
        }
        match move_no_replace(&temp, &candidate) {
            Ok(()) => break candidate,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                candidate = choose_output_path(source, out_folder, |path| path.exists())
                    .ok_or_else(no_name)?;
            }
            Err(_) => {
                lock_retries += 1;
                if lock_retries >= MOVE_RETRIES {
                    return Err(write_error(&candidate));
                }
                std::thread::sleep(MOVE_WAIT);
            }
        }
    };
    guard.keep = true;

    Ok(FileResult::Cleaned(CleanedFile {
        output_path: final_path,
        output_size: written,
        source_size: analysis.source_len,
        format: analysis.format,
        removed: analysis.findings,
        kept: analysis.kept,
        pixels_verified,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::metadata_remover::orientation::{jpeg_exif_segment, minimal_tiff};
    use crate::tools::metadata_remover::png::make_chunk;
    use image::codecs::jpeg::JpegEncoder;
    use image::codecs::png::PngEncoder;
    use image::{ExtendedColorType, ImageEncoder};
    use std::io::Cursor;
    use std::time::SystemTime;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("erx-mr-pipeline-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn gradient(flip: bool) -> image::RgbImage {
        image::RgbImage::from_fn(8, 8, |x, y| {
            if flip {
                image::Rgb([255 - (x * 16) as u8, 255 - (y * 16) as u8, 7])
            } else {
                image::Rgb([(x * 16) as u8, (y * 16) as u8, 128])
            }
        })
    }

    fn png_bytes(image: &image::RgbImage) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        PngEncoder::new(&mut out)
            .write_image(
                image.as_raw(),
                image.width(),
                image.height(),
                ExtendedColorType::Rgb8,
            )
            .unwrap();
        out.into_inner()
    }

    /// A real PNG with a text chunk after the header and extra bytes at the end.
    fn dirty_png() -> Vec<u8> {
        let base = png_bytes(&gradient(false));
        let mut data = base[..33].to_vec();
        data.extend(make_chunk(b"tEXt", b"Comment\0hello"));
        data.extend_from_slice(&base[33..]);
        data.extend_from_slice(b"TRAILING-DATA");
        data
    }

    fn jpeg_bytes() -> Vec<u8> {
        let image = gradient(false);
        let mut out = Cursor::new(Vec::new());
        JpegEncoder::new_with_quality(&mut out, 90)
            .write_image(image.as_raw(), 8, 8, ExtendedColorType::Rgb8)
            .unwrap();
        out.into_inner()
    }

    fn pixels(bytes: &[u8]) -> Vec<u8> {
        image::load_from_memory(bytes).unwrap().to_rgb8().into_raw()
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    fn clean(source: &Path, out_folder: Option<&Path>) -> Result<FileResult, String> {
        clean_one_file(source, out_folder, &CleanOptions::default(), &mut |_, _| {
            true
        })
    }

    fn name_of(path: &Path) -> String {
        path.file_name().unwrap().to_string_lossy().into_owned()
    }

    #[test]
    fn a_png_is_cleaned_into_a_new_file() {
        let dir = temp_dir("png");
        let source = dir.join("photo.png");
        let data = dirty_png();
        fs::write(&source, &data).unwrap();

        let FileResult::Cleaned(done) = clean(&source, None).unwrap() else {
            panic!("expected a cleaned file");
        };
        assert_eq!(done.output_path, dir.join("photoERX.png"));
        assert_eq!(done.format, FileFormat::Png);
        assert!(done.pixels_verified);
        assert_eq!(done.source_size, data.len() as u64);
        let cleaned = fs::read(&done.output_path).unwrap();
        assert_eq!(done.output_size, cleaned.len() as u64);
        assert!(cleaned.len() < data.len());
        assert!(!contains(&cleaned, b"hello"));
        assert!(!contains(&cleaned, b"TRAILING"));
        assert!(
            done.removed
                .iter()
                .any(|finding| finding.label == "Text entry: Comment")
        );
        assert!(
            done.removed
                .iter()
                .any(|finding| finding.label == "Extra data after the image")
        );
        // The original is untouched and nothing else is left in the folder.
        assert_eq!(fs::read(&source).unwrap(), data);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        assert_eq!(pixels(&cleaned), pixels(&png_bytes(&gradient(false))));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_file_without_metadata_makes_no_new_file() {
        let dir = temp_dir("nothing");
        let source = dir.join("clean.png");
        fs::write(&source, png_bytes(&gradient(false))).unwrap();
        let result = clean(&source, None).unwrap();
        assert!(matches!(
            result,
            FileResult::NothingToRemove {
                format: FileFormat::Png,
                ..
            }
        ));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn taken_names_count_up_and_are_never_overwritten() {
        let dir = temp_dir("taken");
        let source = dir.join("photo.png");
        fs::write(&source, dirty_png()).unwrap();
        fs::write(dir.join("photoERX.png"), b"precious bytes 1").unwrap();
        fs::write(dir.join("photoERX (1).png"), b"precious bytes 2").unwrap();

        let FileResult::Cleaned(done) = clean(&source, None).unwrap() else {
            panic!("expected a cleaned file");
        };
        assert_eq!(name_of(&done.output_path), "photoERX (2).png");
        assert_eq!(
            fs::read(dir.join("photoERX.png")).unwrap(),
            b"precious bytes 1"
        );
        assert_eq!(
            fs::read(dir.join("photoERX (1).png")).unwrap(),
            b"precious bytes 2"
        );
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 4);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_chosen_output_folder_receives_the_file() {
        let source_dir = temp_dir("custom-src");
        let out_dir = temp_dir("custom-out");
        let source = source_dir.join("photo.png");
        fs::write(&source, dirty_png()).unwrap();

        let FileResult::Cleaned(done) = clean(&source, Some(out_dir.as_path())).unwrap() else {
            panic!("expected a cleaned file");
        };
        assert_eq!(done.output_path, out_dir.join("photoERX.png"));
        assert_eq!(fs::read_dir(&source_dir).unwrap().count(), 1);
        assert_eq!(fs::read_dir(&out_dir).unwrap().count(), 1);
        fs::remove_dir_all(&source_dir).unwrap();
        fs::remove_dir_all(&out_dir).unwrap();
    }

    #[test]
    fn cancelling_leaves_no_file_behind() {
        let dir = temp_dir("cancel");
        let source = dir.join("photo.png");
        fs::write(&source, dirty_png()).unwrap();
        let result =
            clean_one_file(&source, None, &CleanOptions::default(), &mut |_, _| false).unwrap();
        assert_eq!(result, FileResult::Cancelled);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        assert_eq!(fs::read(&source).unwrap(), dirty_png());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_jpeg_keeps_its_rotation_and_its_pixels() {
        let dir = temp_dir("jpeg");
        let source = dir.join("photo.jpg");
        let mut payload = b"Exif\0\0".to_vec();
        payload.extend(minimal_tiff(6));
        payload.extend_from_slice(b"SERIALNUMBER-1");
        let mut segment = vec![0xFF, 0xE1];
        segment.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        segment.extend(payload);
        let base = jpeg_bytes();
        let mut data = base[..2].to_vec();
        data.extend(segment);
        data.extend_from_slice(&base[2..]);
        fs::write(&source, &data).unwrap();

        let FileResult::Cleaned(done) = clean(&source, None).unwrap() else {
            panic!("expected a cleaned file");
        };
        assert_eq!(name_of(&done.output_path), "photoERX.jpg");
        assert!(done.pixels_verified);
        let cleaned = fs::read(&done.output_path).unwrap();
        assert_eq!(&cleaned[0..2], &[0xFF, 0xD8]);
        assert_eq!(&cleaned[2..38], jpeg_exif_segment(6).as_slice());
        assert!(!contains(&cleaned, b"SERIALNUMBER"));
        assert!(
            done.kept
                .contains(&"Photo rotation (orientation)".to_string())
        );
        assert!(
            done.removed
                .iter()
                .any(|finding| finding.label == "EXIF metadata block")
        );
        assert_eq!(pixels(&cleaned), pixels(&base));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bad_inputs_get_friendly_reasons() {
        let dir = temp_dir("errors");
        assert_eq!(
            clean(&dir.join("missing.png"), None).unwrap_err(),
            "file not found or not accessible"
        );
        assert_eq!(
            clean(Path::new("relative.png"), None).unwrap_err(),
            "file path is not absolute"
        );
        assert_eq!(clean(&dir, None).unwrap_err(), "folders are not supported");
        let text = dir.join("notes.txt");
        fs::write(&text, b"just some text, not a photo").unwrap();
        assert_eq!(
            clean(&text, None).unwrap_err(),
            "not a supported photo or video file"
        );
        let source = dir.join("photo.png");
        fs::write(&source, dirty_png()).unwrap();
        let missing_folder = dir.join("nope");
        assert_eq!(
            clean(&source, Some(missing_folder.as_path())).unwrap_err(),
            "the chosen output folder does not exist"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn concurrent_same_name_outputs_never_overwrite_each_other() {
        let out_dir = temp_dir("race-out");
        let sources: Vec<PathBuf> = (0..6)
            .map(|index| {
                let dir = temp_dir(&format!("race-src-{index}"));
                let path = dir.join("photo.png");
                fs::write(&path, dirty_png()).unwrap();
                path
            })
            .collect();
        let options = CleanOptions::default();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(sources.len()));
        let results: Vec<Result<FileResult, String>> = std::thread::scope(|scope| {
            let handles: Vec<_> = sources
                .iter()
                .map(|source| {
                    let barrier = std::sync::Arc::clone(&barrier);
                    let out_ref = &out_dir;
                    let options_ref = &options;
                    scope.spawn(move || {
                        barrier.wait();
                        clean_one_file(source, Some(out_ref.as_path()), options_ref, &mut |_, _| {
                            true
                        })
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });

        let mut names: Vec<String> = results
            .iter()
            .map(
                |result| match result.as_ref().expect("every file should be cleaned") {
                    FileResult::Cleaned(done) => name_of(&done.output_path),
                    other => panic!("unexpected result {other:?}"),
                },
            )
            .collect();
        names.sort();
        let mut expected: Vec<String> = std::iter::once("photoERX.png".to_string())
            .chain((1..6).map(|number| format!("photoERX ({number}).png")))
            .collect();
        expected.sort();
        assert_eq!(names, expected);
        assert_eq!(fs::read_dir(&out_dir).unwrap().count(), 6);
        for source in &sources {
            let _ = fs::remove_dir_all(source.parent().unwrap());
        }
        let _ = fs::remove_dir_all(&out_dir);
    }

    #[test]
    fn pixel_comparison_detects_differences() {
        let dir = temp_dir("pixels");
        let first = dir.join("a.png");
        let second = dir.join("b.png");
        let text = dir.join("notes.txt");
        fs::write(&first, png_bytes(&gradient(false))).unwrap();
        fs::write(&second, png_bytes(&gradient(true))).unwrap();
        fs::write(&text, b"not a picture").unwrap();

        assert_eq!(pixels_match(&first, &first, FileFormat::Png), Ok(true));
        let different = pixels_match(&first, &second, FileFormat::Png).unwrap_err();
        assert!(different.contains("pixels"), "{different}");
        assert_eq!(pixels_match(&text, &first, FileFormat::Png), Ok(false));
        assert!(pixels_match(&first, &text, FileFormat::Png).is_err());
        assert_eq!(pixels_match(&first, &second, FileFormat::Mp4), Ok(false));
        assert_eq!(pixels_match(&first, &second, FileFormat::Heic), Ok(false));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn stale_temporary_files_are_cleaned_but_young_ones_stay() {
        let dir = temp_dir("stale");
        let source = dir.join("photo.png");
        fs::write(&source, dirty_png()).unwrap();
        let stale = dir.join("old.png.0123abcd.erxtmp");
        let young = dir.join("new.png.89abcdef.erxtmp");
        fs::write(&stale, b"x").unwrap();
        fs::write(&young, b"x").unwrap();
        let file = OpenOptions::new().write(true).open(&stale).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(7200))
            .unwrap();
        drop(file);

        let result = clean(&source, None).unwrap();
        assert!(matches!(result, FileResult::Cleaned(_)));
        assert!(!stale.exists());
        assert!(young.exists());
        assert!(dir.join("photoERX.png").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Needs the sample files of the owner. Run by hand:
    /// cargo test live_real_samples_are_cleaned_end_to_end -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_real_samples_are_cleaned_end_to_end() {
        let samples = "C:/dev/erxtools-testdata/Mediatest";
        let out_dir = PathBuf::from("C:/dev/erxtools/target/mr-live-out");
        let _ = fs::remove_dir_all(&out_dir);
        fs::create_dir_all(&out_dir).unwrap();
        for name in ["A.jpg", "A.HEIC", "A.png", "A.MOV", "A.mp4", "A.mkv"] {
            let source = PathBuf::from(format!("{samples}/{name}"));
            let before = fs::metadata(&source).unwrap().len();
            let result = clean(&source, Some(out_dir.as_path()))
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            match result {
                FileResult::Cleaned(done) => {
                    println!(
                        "{name}: cleaned to {} ({} -> {} bytes), {} removed, pixels verified: {}",
                        name_of(&done.output_path),
                        done.source_size,
                        done.output_size,
                        done.removed.len(),
                        done.pixels_verified
                    );
                    for finding in &done.removed {
                        println!("  removed: {} | {:?}", finding.label, finding.detail);
                    }
                    for note in &done.kept {
                        println!("  kept: {note}");
                    }
                    if done.format.is_video() {
                        assert_eq!(done.output_size, done.source_size, "{name}: size changed");
                    }
                    let again = analyze_path(&done.output_path, &CleanOptions::default()).unwrap();
                    assert!(
                        !again.has_changes(),
                        "{name}: a second run still changes things"
                    );
                }
                FileResult::NothingToRemove { format, .. } => {
                    println!("{name}: nothing to remove ({format:?})");
                }
                FileResult::Cancelled => panic!("{name}: cancelled"),
            }
            assert_eq!(
                fs::metadata(&source).unwrap().len(),
                before,
                "{name}: the original changed"
            );
        }
        fs::remove_dir_all(&out_dir).unwrap();
    }
}
