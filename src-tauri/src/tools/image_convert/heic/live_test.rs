//! Live HEIC tests that touch the real machine (#[ignore]; run with
//! `cargo test heic -- --ignored`). They read the kamadak-exif crate test
//! fixture and — for 6c — the owner's own sample photos from the folder in
//! `ERX_HEIC_SAMPLE_DIR` (outside the repo, never committed), and write only
//! into the git-ignored `target/heic-check/` scratch folder.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

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

// ---------------------------------------------------------------------------
// 6c — the owner's real iPhone photos (ERX_HEIC_SAMPLE_DIR, outside the repo)
// ---------------------------------------------------------------------------

/// The owner's sample folder, set per run:
/// `ERX_HEIC_SAMPLE_DIR=C:\dev\erxtools-testdata cargo test heic -- --ignored`
fn sample_dir() -> PathBuf {
    let dir = std::env::var_os("ERX_HEIC_SAMPLE_DIR")
        .map(PathBuf::from)
        .expect("set ERX_HEIC_SAMPLE_DIR to the folder with the owner's sample photos");
    assert!(
        dir.is_dir(),
        "ERX_HEIC_SAMPLE_DIR does not exist: {}",
        dir.display()
    );
    dir
}

/// Every .heic/.heif in the sample folder (extension case-insensitive,
/// sorted so runs are deterministic).
fn samples() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(sample_dir())
        .expect("sample folder must be readable")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .map(|ext| ext.to_string_lossy().to_lowercase())
                    .is_some_and(|ext| ext == "heic" || ext == "heif")
        })
        .collect();
    found.sort();
    assert!(!found.is_empty(), "no .heic/.heif sample photos found");
    found
}

/// Peak working set of this process so far (approximate peak memory).
fn peak_working_set_bytes() -> usize {
    unsafe {
        use windows_sys::Win32::System::ProcessStatus::{
            K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;

        let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        let ok = K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        );
        if ok != 0 {
            counters.PeakWorkingSetSize
        } else {
            0
        }
    }
}

/// A tiny worker pool mirroring the batch command: bounded workers pull
/// indices in list order and every result lands in its own slot, so the
/// caller sees results exactly in list order.
fn run_mini_batch(
    paths: &[PathBuf],
    options: &ConvertOptions,
) -> Vec<Option<Result<PathBuf, String>>> {
    let next_index = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Result<PathBuf, String>>>> =
        Mutex::new((0..paths.len()).map(|_| None).collect());
    let workers = std::thread::available_parallelism()
        .map_or(2, |n| n.get())
        .clamp(1, 4);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next_index.fetch_add(1, Ordering::Relaxed);
                    if index >= paths.len() {
                        break;
                    }
                    let outcome = convert_one_file(&paths[index], options);
                    results.lock().unwrap()[index] = Some(outcome.map(|o| o.output_path));
                }
            });
        }
    });
    results.into_inner().unwrap()
}

/// 6c-1: inspect + decode + convert every sample to JPEG and PNG, logging
/// the facts needed for the 6c-2/6c-3 rotation decision.
#[test]
#[ignore]
fn live_samples_convert_to_jpeg_and_png() {
    for sample in samples() {
        let name = sample.file_name().unwrap().to_string_lossy().into_owned();
        let stem = sample.file_stem().unwrap().to_string_lossy().into_owned();
        let (width, height) =
            super::wic::dimensions(&sample).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let decoded =
            super::wic::decode_first_frame(&sample).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        println!(
            "{name}: container {width}x{height}, decoded {}x{}, WIC frames {}",
            decoded.width, decoded.height, decoded.frame_count
        );
        assert_eq!(decoded.width, width, "{name}");
        assert_eq!(decoded.height, height, "{name}");

        let out_dir = scratch_dir(&format!("sample-{stem}"));
        for format in [OutputFormat::Jpeg, OutputFormat::Png] {
            let start = Instant::now();
            let outcome = convert_one_file(&sample, &options(format, &out_dir))
                .unwrap_or_else(|error| panic!("{name} -> {format:?}: {error}"));
            let elapsed = start.elapsed().as_millis();
            let result = image::open(&outcome.output_path).unwrap();
            println!(
                "  -> {} {}x{} in {elapsed} ms ({} bytes)",
                format.extension(),
                result.dimensions().0,
                result.dimensions().1,
                outcome.output_size
            );
            assert_eq!(result.dimensions(), (width, height), "{name}");
        }
    }
}

/// 6c-4a: every sample converts to all five output formats and each output
/// decodes back with matching dimensions.
#[test]
#[ignore]
fn live_samples_convert_to_all_five_outputs() {
    for sample in samples() {
        let stem = sample.file_stem().unwrap().to_string_lossy().into_owned();
        let (width, height) =
            super::wic::dimensions(&sample).unwrap_or_else(|e| panic!("{stem}: {e:?}"));
        for format in [
            OutputFormat::Jpeg,
            OutputFormat::Png,
            OutputFormat::Webp,
            OutputFormat::Bmp,
            OutputFormat::Tiff,
        ] {
            let out_dir = scratch_dir(&format!("five-{stem}-{}", format.extension()));
            let outcome = convert_one_file(&sample, &options(format, &out_dir))
                .unwrap_or_else(|error| panic!("{stem} -> {format:?}: {error}"));
            let decoded = image::open(&outcome.output_path)
                .unwrap_or_else(|error| panic!("{stem} -> {format:?}: {error}"));
            assert_eq!(
                decoded.dimensions(),
                (width, height),
                "{stem} -> {format:?}"
            );
        }
    }
}

/// 6c-4b: a mixed batch (2 HEIC + 2 generated PNG + 1 generated JPEG) keeps
/// results in list order; each output is named after its own source.
#[test]
#[ignore]
fn live_sample_batch_keeps_list_order() {
    let dir = scratch_dir("batch");
    let all = samples();
    assert!(all.len() >= 3, "need 3 sample photos");

    // Generated test images for the non-HEIC members of the batch.
    let save = |path: &Path, color: [u8; 3], format: image::ImageFormat| {
        let image = image::DynamicImage::ImageRgb8(image::ImageBuffer::from_fn(8, 6, |_, _| {
            image::Rgb(color)
        }));
        let mut out = std::io::Cursor::new(Vec::new());
        image.write_to(&mut out, format).unwrap();
        std::fs::write(path, out.into_inner()).unwrap();
    };
    let png1 = dir.join("gen-a.png");
    save(&png1, [200, 10, 10], image::ImageFormat::Png);
    let png2 = dir.join("gen-b.png");
    save(&png2, [10, 200, 10], image::ImageFormat::Png);
    let jpg1 = dir.join("gen-c.jpg");
    save(&jpg1, [10, 10, 200], image::ImageFormat::Jpeg);

    let paths = vec![
        all[0].clone(),
        png1.clone(),
        all[1].clone(),
        jpg1.clone(),
        all[2].clone(),
    ];
    let out_dir = dir.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();
    let results = run_mini_batch(&paths, &options(OutputFormat::Jpeg, &out_dir));

    for (index, path) in paths.iter().enumerate() {
        let slot = results[index]
            .as_ref()
            .unwrap_or_else(|| panic!("slot {index} never filled"));
        let output = slot
            .as_ref()
            .unwrap_or_else(|error| panic!("slot {index} failed: {error}"));
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        // The generated JPEG re-compressed to JPEG takes the converter's
        // `-converted` name; everything else gets the plain new extension.
        let expected = if path == &jpg1 {
            format!("{stem}-converted.jpg")
        } else {
            format!("{stem}.jpg")
        };
        assert_eq!(
            output.file_name().unwrap().to_string_lossy(),
            expected,
            "slot {index} must hold this source's output"
        );
        assert!(output.exists(), "slot {index}");
    }
}

/// 6c-4c: timing for one photo and for a 20-copy batch (4 workers, like the
/// app), plus the process peak working set as an approximate peak memory.
#[test]
#[ignore]
fn live_sample_timing_and_memory() {
    let sample = &samples()[0];
    let name = sample.file_name().unwrap().to_string_lossy().into_owned();
    let dir = scratch_dir("timing");

    let start = Instant::now();
    let outcome = convert_one_file(sample, &options(OutputFormat::Jpeg, &dir))
        .unwrap_or_else(|error| panic!("{name}: {error}"));
    println!(
        "single photo {name}: {} ms, {} bytes out",
        start.elapsed().as_millis(),
        outcome.output_size
    );

    let many = dir.join("many");
    std::fs::create_dir_all(&many).unwrap();
    let paths: Vec<PathBuf> = (1..=20)
        .map(|i| {
            let copy = many.join(format!("copy{i:02}.HEIC"));
            std::fs::copy(sample, &copy).unwrap();
            copy
        })
        .collect();
    let peak_before = peak_working_set_bytes();
    let start = Instant::now();
    let results = run_mini_batch(&paths, &options(OutputFormat::Jpeg, &many));
    let elapsed = start.elapsed();
    let peak = peak_working_set_bytes();
    for (index, slot) in results.iter().enumerate() {
        slot.as_ref()
            .unwrap_or_else(|| panic!("slot {index} never filled"))
            .as_ref()
            .unwrap_or_else(|error| panic!("slot {index} failed: {error}"));
    }
    println!(
        "20-photo batch (4 workers): {} ms total; peak working set {} MiB (was {} MiB before the batch)",
        elapsed.as_millis(),
        peak / (1024 * 1024),
        peak_before / (1024 * 1024)
    );
    assert!(
        peak < 1_500 * 1024 * 1024,
        "peak memory looks dangerous: {} MiB",
        peak / (1024 * 1024)
    );
}

/// 6c-4d: decode works with a non-ASCII folder/file name and with a path
/// longer than 200 characters (kept under the classic MAX_PATH limit).
#[test]
#[ignore]
fn live_sample_unicode_and_long_paths() {
    let sample = &samples()[0];

    let uni = scratch_dir("Ünïcode ファイル");
    let uni_file = uni.join("写真 テスト.HEIC");
    std::fs::copy(sample, &uni_file).unwrap();
    let outcome = convert_one_file(&uni_file, &options(OutputFormat::Jpeg, &uni))
        .expect("unicode path must convert");
    assert_eq!(
        outcome.output_path,
        uni.join("写真 テスト.jpg"),
        "unicode stem survives into the output name"
    );
    assert!(image::open(&outcome.output_path).is_ok());

    let long_dir = scratch_dir("long");
    let mut long_path = long_dir.clone();
    for level in 0..4 {
        long_path = long_path.join(format!("level-{level}-{}", "d".repeat(38)));
    }
    std::fs::create_dir_all(&long_path).unwrap();
    let long_file = long_path.join("photo.HEIC");
    std::fs::copy(sample, &long_file).unwrap();
    let length = long_file.to_string_lossy().len();
    println!("long path: {length} characters");
    assert!(length > 200, "path must exceed 200 characters: {length}");
    assert!(
        length < 260,
        "test stays under MAX_PATH by design: {length}"
    );
    let outcome = convert_one_file(&long_file, &options(OutputFormat::Jpeg, &long_path))
        .expect("long path must convert");
    assert!(outcome.output_path.exists());
}

/// 6c-3: Windows' HEIC decoder applies the photo rotation itself — verified
/// visually on 2026-11-07 (see `heic_image`'s doc comment in convert.rs).
/// The owner's upright iPhone photos must keep their portrait dimensions,
/// and the pipeline must NOT rotate them a second time.
#[test]
#[ignore]
fn live_sample_rotation_is_applied_by_wic() {
    for sample in samples() {
        let stem = sample.file_stem().unwrap().to_string_lossy().into_owned();
        let (width, height) =
            super::wic::dimensions(&sample).unwrap_or_else(|e| panic!("{stem}: {e:?}"));
        let out_dir = scratch_dir(&format!("rot-{stem}"));
        let outcome = convert_one_file(&sample, &options(OutputFormat::Jpeg, &out_dir))
            .unwrap_or_else(|error| panic!("{stem}: {error}"));
        let decoded = image::open(&outcome.output_path).unwrap();
        assert_eq!(
            decoded.dimensions(),
            (width, height),
            "{stem}: output must keep WIC's rotated dimensions"
        );
        assert!(
            height > width,
            "{stem}: portrait sample must stay portrait ({}x{})",
            width,
            height
        );
    }
}
