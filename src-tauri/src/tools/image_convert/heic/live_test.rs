//! Live HEIC tests that touch the real machine (#[ignore]; run with
//! `cargo test heic -- --ignored`). They read the kamadak-exif crate test
//! fixture — never any personal photo — and write only into the git-ignored
//! `target/heic-check/` scratch folder.

use std::path::{Path, PathBuf};

use image::GenericImageView;

use crate::tools::image_convert::core::{
    ConvertOptions, InputFormat, Inspection, OutputFormat, RgbColor, inspect_path,
};
use crate::tools::image_convert::pipeline::convert_one_file;

/// target/heic-check/ — git-ignored scratch space (R8).
fn scratch_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("heic-check")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The kamadak-exif crate test fixture `exif.heic`, found by scanning the
/// cargo registry source folders (no hardcoded hash).
fn find_kamadak_fixture() -> PathBuf {
    let registry_src = std::env::var_os("USERPROFILE")
        .map(|home| Path::new(&home).join(".cargo").join("registry").join("src"));
    let registry_src = registry_src.expect("USERPROFILE must be set");
    for entry in std::fs::read_dir(&registry_src).expect("registry src folder") {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_dir() {
            continue;
        }
        for version in std::fs::read_dir(entry.path()).unwrap() {
            let version = version.unwrap();
            let name = version.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("kamadak-exif-") {
                let candidate = version.path().join("tests").join("exif.heic");
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }
    panic!("kamadak-exif exif.heic fixture not found under {registry_src:?}");
}

fn options(format: OutputFormat, folder: &Path) -> ConvertOptions {
    ConvertOptions {
        format,
        jpeg_quality: 85,
        background: RgbColor(255, 255, 255),
        output_folder: Some(folder.to_path_buf()),
    }
}

/// The full real pipeline: inspect → HEIC decode → all five outputs, with
/// the converter's own naming and collision handling.
#[test]
#[ignore]
fn live_heic_fixture_inspects_and_converts_to_all_outputs() {
    let fixture = find_kamadak_fixture();
    let (width, height) =
        super::wic::dimensions(&fixture).expect("fixture dimensions must succeed");

    // Inspected as a supported HEIC input (never animated).
    match inspect_path(&fixture) {
        Inspection::Image(info) => {
            assert_eq!(info.format, InputFormat::Heic);
            assert!(!info.animated);
        }
        other => panic!("expected Image(Heic), got {other:?}"),
    }

    // Content over extension: the fixture under a .jpg name is still HEIC.
    let renamed_dir = scratch_dir("renamed");
    let renamed = renamed_dir.join("photo.jpg");
    std::fs::copy(&fixture, &renamed).unwrap();
    match inspect_path(&renamed) {
        Inspection::Image(info) => assert_eq!(info.format, InputFormat::Heic),
        other => panic!("expected Image(Heic) under a .jpg name, got {other:?}"),
    }

    // All five outputs decode back to the fixture's dimensions, and the
    // naming rules behave like every other format.
    for format in [
        OutputFormat::Jpeg,
        OutputFormat::Png,
        OutputFormat::Webp,
        OutputFormat::Bmp,
        OutputFormat::Tiff,
    ] {
        let out_dir = scratch_dir(&format!("out-{}", format.extension()));
        let source = out_dir.join("photo.heic");
        std::fs::copy(&fixture, &source).unwrap();

        let outcome = convert_one_file(&source, &options(format, &out_dir))
            .unwrap_or_else(|error| panic!("{format:?}: {error}"));
        assert_eq!(
            outcome.output_path,
            out_dir.join(format!("photo.{}", format.extension())),
            "{format:?}: output name"
        );

        let decoded = image::open(&outcome.output_path)
            .unwrap_or_else(|error| panic!("{format:?}: output must decode: {error}"));
        assert_eq!(decoded.dimensions(), (width, height), "{format:?}");

        // Never overwrites: converting again counts upwards.
        let second = convert_one_file(&source, &options(format, &out_dir))
            .unwrap_or_else(|error| panic!("{format:?}: second run: {error}"));
        assert_eq!(
            second.output_path,
            out_dir.join(format!("photo (1).{}", format.extension())),
            "{format:?}: collision name"
        );
    }
}
