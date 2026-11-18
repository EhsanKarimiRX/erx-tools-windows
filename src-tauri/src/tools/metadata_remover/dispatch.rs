//! Finds out what kind of file something is (from its CONTENT, never from
//! the extension), checks the size limit and runs the matching handler.
//! Then the readable "found" details from EXIF and XMP are added.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use super::model::{Analysis, CleanOptions, FileFormat};
use super::{bmp, found, gif, jpeg, png, webp};

/// Largest photo the tool accepts (256 MB, like the Image Converter).
pub const MAX_IMAGE_BYTES: u64 = 256 * 1024 * 1024;
/// Largest video the tool accepts (16 GB). Videos are processed as a
/// stream, so memory use stays small.
pub const MAX_VIDEO_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const HEAD_BYTES: u64 = 64;

/// What the first bytes of a file say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detected {
    Known(FileFormat),
    /// A Matroska / WebM file.
    Matroska,
    /// A format we recognize but do not support; the name is shown to the user.
    Unsupported(&'static str),
    Unknown,
}

/// Largest `ftyp` box that detection reads and checks (64 KiB, about 16,000
/// brands). A bigger box is refused as an unusual structure instead of being
/// guessed, so a file is never passed to the MP4 handler unchecked.
const MAX_FTYP_BYTES: u64 = 64 * 1024;

/// The M4V brands that Apple tools write: iTunes movies and their HDR and
/// preview variants.
const M4V_BRANDS: [&[u8; 4]; 3] = [b"M4V ", b"M4VH", b"M4VP"];

fn is_m4v_brand(brand: &[u8]) -> bool {
    M4V_BRANDS.iter().any(|known| brand == known.as_slice())
}

/// True when the `ftyp` box declares an M4V brand, either as the major brand
/// or as one of the compatible brands. Only bytes inside the box count, so
/// bytes of the next box are never read as brands.
fn declares_m4v(ftyp: &[u8]) -> bool {
    let size = match ftyp
        .get(0..4)
        .and_then(|part| <[u8; 4]>::try_from(part).ok())
    {
        Some(bytes) => u32::from_be_bytes(bytes) as usize,
        None => return false,
    };
    let end = size.min(ftyp.len());
    let brand_at = |at: usize| ftyp.get(at..at + 4);
    // 8 = the box header followed by the major brand; 16 = just after the
    // 4-byte minor version, where the compatible brands start.
    if brand_at(8).is_some_and(is_m4v_brand) {
        return true;
    }
    let mut at = 16usize;
    while at + 4 <= end {
        if brand_at(at).is_some_and(is_m4v_brand) {
            return true;
        }
        at += 4;
    }
    false
}

/// Detection that reads the complete `ftyp` box before deciding, so an M4V
/// brand anywhere in the brand list is refused. Every other format is decided
/// from the first bytes only. An invalid, truncated or oversized `ftyp` is
/// refused instead of being guessed.
fn detect_checked<R: Read + Seek>(src: &mut R, head: &[u8], len: u64) -> Result<Detected, String> {
    if head.get(4..8) != Some(&b"ftyp"[..]) {
        return Ok(detect(head));
    }
    let damaged = || "damaged or incomplete video file".to_string();
    // An ftyp box holds at least the 8-byte header, the major brand and the
    // minor version.
    if head.len() < 16 {
        return Err(damaged());
    }
    let size = u64::from(u32::from_be_bytes([head[0], head[1], head[2], head[3]]));
    if size < 16 || size > len {
        return Err(damaged());
    }
    if size > MAX_FTYP_BYTES {
        return Err("unusual video structure".to_string());
    }
    // The buffer is capped by MAX_FTYP_BYTES above, never by the untrusted size.
    let mut ftyp = vec![0u8; size as usize];
    src.seek(SeekFrom::Start(0))
        .map_err(|_| "cannot read the file".to_string())?;
    src.read_exact(&mut ftyp).map_err(|_| damaged())?;
    if declares_m4v(&ftyp) {
        return Ok(Detected::Unsupported("M4V"));
    }
    Ok(detect_brand(&ftyp))
}

fn detect_brand(head: &[u8]) -> Detected {
    let brand: [u8; 4] = match head
        .get(8..12)
        .and_then(|part| <[u8; 4]>::try_from(part).ok())
    {
        Some(found) => found,
        None => return Detected::Unknown,
    };
    // M4V is refused by content, never by the file name.
    if declares_m4v(head) {
        return Detected::Unsupported("M4V");
    }
    match &brand {
        b"qt  " => Detected::Known(FileFormat::Mov),
        b"heic" | b"heix" | b"hevc" | b"hevx" | b"heim" | b"heis" | b"mif1" | b"msf1" => {
            Detected::Known(FileFormat::Heic)
        }
        b"avif" | b"avis" => Detected::Unsupported("AVIF"),
        b"M4A " | b"M4B " | b"M4P " => Detected::Unsupported("Audio"),
        _ => Detected::Known(FileFormat::Mp4),
    }
}

/// Recognizes the file type from its first bytes (at least 12 are needed
/// for the video and WebP types).
pub fn detect(head: &[u8]) -> Detected {
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Detected::Known(FileFormat::Jpeg);
    }
    if head.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Detected::Known(FileFormat::Png);
    }
    if head.starts_with(b"RIFF") && head.get(8..12) == Some(&b"WEBP"[..]) {
        return Detected::Known(FileFormat::Webp);
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return Detected::Known(FileFormat::Gif);
    }
    if head.starts_with(&[b'I', b'I', 0x2A, 0x00]) || head.starts_with(&[b'M', b'M', 0x00, 0x2A]) {
        return Detected::Known(FileFormat::Tiff);
    }
    if head.starts_with(&[b'I', b'I', 0x2B, 0x00]) || head.starts_with(&[b'M', b'M', 0x00, 0x2B]) {
        return Detected::Unsupported("BigTIFF");
    }
    if head.get(4..8) == Some(&b"ftyp"[..]) {
        return detect_brand(head);
    }
    if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Detected::Matroska;
    }
    if head.starts_with(b"BM") {
        return Detected::Known(FileFormat::Bmp);
    }
    Detected::Unknown
}

/// The size limit for one file of this format.
pub fn size_limit(format: FileFormat) -> u64 {
    if format.is_video() {
        MAX_VIDEO_BYTES
    } else {
        MAX_IMAGE_BYTES
    }
}

fn limit_text(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{} GB", bytes >> 30)
    } else {
        format!("{} MB", bytes >> 20)
    }
}

/// Analyzes the file behind `src` (`len` bytes): detects the type, checks
/// the size limit, runs the handler and adds the readable details.
pub fn analyze_reader<R: Read + Seek>(
    src: &mut R,
    len: u64,
    options: &CleanOptions,
) -> Result<Analysis, String> {
    if len == 0 {
        return Err("file is empty".to_string());
    }
    let cannot_read = || "cannot read the file".to_string();
    src.seek(SeekFrom::Start(0)).map_err(|_| cannot_read())?;
    let mut head = Vec::new();
    let mut limited = src.by_ref().take(HEAD_BYTES);
    limited.read_to_end(&mut head).map_err(|_| cannot_read())?;
    let format = match detect_checked(src, &head, len)? {
        Detected::Known(format) => format,
        Detected::Matroska => FileFormat::Mkv,
        Detected::Unsupported(name) => return Err(format!("{name} files are not supported")),
        Detected::Unknown => return Err("not a supported photo or video file".to_string()),
    };
    let limit = size_limit(format);
    if len > limit {
        return Err(format!(
            "file is larger than the {} limit",
            limit_text(limit)
        ));
    }
    let result = match format {
        FileFormat::Jpeg => jpeg::analyze(src, len, options),
        FileFormat::Png => png::analyze(src, len, options),
        FileFormat::Webp => webp::analyze(src, len, options),
        FileFormat::Gif => gif::analyze(src, len, options),
        FileFormat::Bmp => bmp::analyze(src, len, options),
        FileFormat::Tiff => super::tiff::analyze(src, len, options),
        FileFormat::Heic => super::heic::analyze(src, len, options),
        FileFormat::Mp4 | FileFormat::Mov => super::mp4::analyze(src, len, format, options),
        FileFormat::Mkv => super::mkv::analyze(src, len, options),
    };
    let mut analysis = result?;
    found::enrich(&mut analysis);
    Ok(analysis)
}

/// Analyzes one file on disk. The error strings are short, friendly
/// English reasons.
pub fn analyze_path(path: &Path, options: &CleanOptions) -> Result<Analysis, String> {
    let metadata =
        std::fs::metadata(path).map_err(|_| "file not found or not accessible".to_string())?;
    if metadata.is_dir() {
        return Err("folders are not supported".to_string());
    }
    if !metadata.is_file() {
        return Err("not a regular file".to_string());
    }
    let file = File::open(path).map_err(|_| "cannot open the file".to_string())?;
    analyze_reader(&mut BufReader::new(file), metadata.len(), options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::path::PathBuf;

    fn ftyp(brand: &[u8; 4]) -> Vec<u8> {
        let mut out = vec![0, 0, 0, 24];
        out.extend_from_slice(b"ftyp");
        out.extend_from_slice(brand);
        out.extend_from_slice(&[0u8; 12]);
        out
    }

    /// A complete `ftyp` box with a major brand and a list of compatible brands.
    fn ftyp_with(major: &[u8; 4], compatible: &[&[u8; 4]]) -> Vec<u8> {
        let size = 16 + 4 * compatible.len() as u32;
        let mut out = size.to_be_bytes().to_vec();
        out.extend_from_slice(b"ftyp");
        out.extend_from_slice(major);
        out.extend_from_slice(&[0u8; 4]);
        for brand in compatible {
            out.extend_from_slice(*brand);
        }
        out
    }

    /// The M4V brand identifiers the detection must refuse.
    const M4V_IDS: [&[u8; 4]; 3] = [b"M4V ", b"M4VH", b"M4VP"];

    /// Detection as `analyze_reader` runs it: the first 64 bytes plus a reader
    /// that can read the rest of the file.
    fn checked(data: &[u8]) -> Result<Detected, String> {
        let head_len = data.len().min(64);
        detect_checked(
            &mut Cursor::new(data.to_vec()),
            &data[..head_len],
            data.len() as u64,
        )
    }

    /// An EBML element with a one-byte size field (every test payload is small).
    fn ebml_elem(id: &[u8], payload: &[u8]) -> Vec<u8> {
        assert!(payload.len() < 0x7F, "payload too large for this helper");
        let mut out = id.to_vec();
        out.push(0x80 | payload.len() as u8);
        out.extend_from_slice(payload);
        out
    }

    /// A WebM file: the EBML header says DocType "webm" and the Segment holds
    /// one Tags element.
    fn webm_bytes() -> Vec<u8> {
        let doc_type = ebml_elem(&[0x42, 0x82], b"webm");
        let header = ebml_elem(&[0x1A, 0x45, 0xDF, 0xA3], &doc_type);
        let tag = ebml_elem(&[0x73, 0x73], b"x");
        let tags = ebml_elem(&[0x12, 0x54, 0xC3, 0x67], &tag);
        let segment = ebml_elem(&[0x18, 0x53, 0x80, 0x67], &tags);
        let mut out = header;
        out.extend(segment);
        out
    }

    fn png_with_text() -> Vec<u8> {
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        out.extend(png::make_chunk(
            b"IHDR",
            &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0],
        ));
        out.extend(png::make_chunk(b"tEXt", b"Comment\0hi"));
        out.extend(png::make_chunk(b"IDAT", &[1, 2, 3]));
        out.extend(png::make_chunk(b"IEND", &[]));
        out
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("erx-mr-dispatch-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detect_recognizes_the_supported_formats() {
        let known = |head: &[u8]| detect(head);
        assert_eq!(
            known(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Detected::Known(FileFormat::Jpeg)
        );
        assert_eq!(known(&png_with_text()), Detected::Known(FileFormat::Png));
        assert_eq!(
            known(b"RIFF\0\0\0\0WEBPVP8 "),
            Detected::Known(FileFormat::Webp)
        );
        assert_eq!(known(b"GIF89a\x01\0"), Detected::Known(FileFormat::Gif));
        assert_eq!(known(b"GIF87a\x01\0"), Detected::Known(FileFormat::Gif));
        assert_eq!(known(b"BM\0\0\0\0"), Detected::Known(FileFormat::Bmp));
        assert_eq!(
            known(&[b'I', b'I', 0x2A, 0, 8, 0, 0, 0]),
            Detected::Known(FileFormat::Tiff)
        );
        assert_eq!(
            known(&[b'M', b'M', 0, 0x2A, 0, 0, 0, 8]),
            Detected::Known(FileFormat::Tiff)
        );
        assert_eq!(known(&ftyp(b"qt  ")), Detected::Known(FileFormat::Mov));
        assert_eq!(known(&ftyp(b"isom")), Detected::Known(FileFormat::Mp4));
        assert_eq!(known(&ftyp(b"mp42")), Detected::Known(FileFormat::Mp4));
        assert_eq!(known(&ftyp(b"heic")), Detected::Known(FileFormat::Heic));
        assert_eq!(known(&ftyp(b"mif1")), Detected::Known(FileFormat::Heic));
    }

    #[test]
    fn detect_reports_unsupported_and_unknown_files() {
        assert_eq!(detect(&ftyp(b"avif")), Detected::Unsupported("AVIF"));
        assert_eq!(detect(&ftyp(b"M4A ")), Detected::Unsupported("Audio"));
        assert_eq!(
            detect(&[b'I', b'I', 0x2B, 0, 8, 0, 0, 0]),
            Detected::Unsupported("BigTIFF")
        );
        assert_eq!(detect(&[0x1A, 0x45, 0xDF, 0xA3, 0x9F]), Detected::Matroska);
        assert_eq!(detect(b"hello world"), Detected::Unknown);
        assert_eq!(detect(&[]), Detected::Unknown);
        assert_eq!(
            detect(&[0, 0, 0, 24, b'f', b't', b'y', b'p']),
            Detected::Unknown
        );
    }

    #[test]
    fn every_m4v_brand_is_refused_as_a_major_brand() {
        for brand in M4V_IDS {
            let data = ftyp(brand);
            assert_eq!(detect(&data), Detected::Unsupported("M4V"), "{brand:?}");
            assert_eq!(checked(&data).unwrap(), Detected::Unsupported("M4V"));
            let error = analyze_reader(
                &mut Cursor::new(data.clone()),
                data.len() as u64,
                &CleanOptions::default(),
            )
            .unwrap_err();
            assert_eq!(error, "M4V files are not supported", "{brand:?}");
        }
    }

    #[test]
    fn every_m4v_brand_is_refused_as_a_compatible_brand() {
        for brand in M4V_IDS {
            let data = ftyp_with(b"mp42", &[b"mp42", brand, b"isom"]);
            assert_eq!(detect(&data), Detected::Unsupported("M4V"), "{brand:?}");
            assert_eq!(checked(&data).unwrap(), Detected::Unsupported("M4V"));
            let error = analyze_reader(
                &mut Cursor::new(data.clone()),
                data.len() as u64,
                &CleanOptions::default(),
            )
            .unwrap_err();
            assert_eq!(error, "M4V files are not supported", "{brand:?}");
        }
    }

    #[test]
    fn an_m4v_brand_after_the_first_64_bytes_is_still_refused() {
        let mut brands: Vec<&[u8; 4]> = vec![b"isom"; 20];
        // Offset 16 + 15 * 4 = 76, past the 64 bytes the head holds.
        brands[15] = b"M4V ";
        let data = ftyp_with(b"mp42", &brands);
        assert!(data.len() > 64);
        // The first bytes alone cannot see it; the complete scan does.
        assert_eq!(detect(&data[..64]), Detected::Known(FileFormat::Mp4));
        assert_eq!(checked(&data).unwrap(), Detected::Unsupported("M4V"));
        let error = analyze_reader(
            &mut Cursor::new(data.clone()),
            data.len() as u64,
            &CleanOptions::default(),
        )
        .unwrap_err();
        assert_eq!(error, "M4V files are not supported");
    }

    #[test]
    fn a_long_brand_list_without_m4v_is_still_an_ordinary_mp4() {
        // A 4 KB ftyp box: well inside the 64 KiB bound.
        let brands: Vec<&[u8; 4]> = vec![b"isom"; 1000];
        let data = ftyp_with(b"mp42", &brands);
        assert_eq!(checked(&data).unwrap(), Detected::Known(FileFormat::Mp4));
        // Detection passes and the MP4 handler runs (and asks for a moov box).
        let error = analyze_reader(
            &mut Cursor::new(data.clone()),
            data.len() as u64,
            &CleanOptions::default(),
        )
        .unwrap_err();
        assert_eq!(error, "no video information found in the file");
    }

    #[test]
    fn an_invalid_truncated_or_oversized_ftyp_is_refused() {
        // A box too small to hold the major brand.
        let mut too_small = ftyp(b"mp42");
        too_small[0..4].copy_from_slice(&12u32.to_be_bytes());
        assert_eq!(
            checked(&too_small).unwrap_err(),
            "damaged or incomplete video file"
        );

        // A box that claims more bytes than the file holds.
        let mut truncated = ftyp(b"mp42");
        truncated[0..4].copy_from_slice(&200u32.to_be_bytes());
        assert_eq!(
            checked(&truncated).unwrap_err(),
            "damaged or incomplete video file"
        );

        // A box over the safety bound is refused before anything is read; the
        // stated length is large too, so only the bound can refuse it.
        let mut oversized = ftyp(b"mp42");
        oversized[0..4].copy_from_slice(&70_000u32.to_be_bytes());
        let head_len = oversized.len().min(64);
        let error = detect_checked(
            &mut Cursor::new(oversized.clone()),
            &oversized[..head_len],
            70_000,
        )
        .unwrap_err();
        assert_eq!(error, "unusual video structure");

        // A file that ends before the major brand.
        let mut short = vec![0u8, 0, 0, 24];
        short.extend_from_slice(b"ftyp");
        assert_eq!(
            checked(&short).unwrap_err(),
            "damaged or incomplete video file"
        );

        // None of these fell through to the MP4 handler.
        for data in [too_small, truncated, short] {
            let error = analyze_reader(
                &mut Cursor::new(data.clone()),
                data.len() as u64,
                &CleanOptions::default(),
            )
            .unwrap_err();
            assert_ne!(error, "no video information found in the file", "{error}");
        }
    }

    #[test]
    fn ordinary_mp4_brands_still_pass() {
        assert_eq!(detect(&ftyp(b"isom")), Detected::Known(FileFormat::Mp4));
        assert_eq!(detect(&ftyp(b"mp42")), Detected::Known(FileFormat::Mp4));
        assert_eq!(detect(&ftyp(b"iso2")), Detected::Known(FileFormat::Mp4));
        // A real compatible-brand list without M4V stays an ordinary MP4.
        assert_eq!(
            detect(&ftyp_with(b"mp42", &[b"isom", b"iso2", b"avc1", b"mp41"])),
            Detected::Known(FileFormat::Mp4)
        );
        // The audio brands are still refused before any MP4 fallback.
        assert_eq!(
            detect(&ftyp_with(b"M4A ", &[b"M4A ", b"mp42"])),
            Detected::Unsupported("Audio")
        );
    }

    #[test]
    fn a_webm_file_reaches_the_matroska_handler() {
        let data = webm_bytes();
        assert_eq!(detect(&data), Detected::Matroska);
        let analysis = analyze_reader(
            &mut Cursor::new(data.clone()),
            data.len() as u64,
            &CleanOptions::default(),
        )
        .unwrap();
        // The DocType "webm" is not rejected: the Matroska handler cleans it and
        // the result is labelled MKV.
        assert_eq!(analysis.format, FileFormat::Mkv);
        assert!(analysis.has_changes());
        let labels: Vec<&str> = analysis
            .findings
            .iter()
            .map(|finding| finding.label.as_str())
            .collect();
        assert_eq!(labels, vec!["Tags (extra information)"]);
        assert_eq!(analysis.findings[0].detail.as_deref(), Some("1 tag"));
    }

    #[test]
    fn a_png_is_analyzed_through_the_dispatcher() {
        let data = png_with_text();
        let analysis = analyze_reader(
            &mut Cursor::new(data.clone()),
            data.len() as u64,
            &CleanOptions::default(),
        )
        .unwrap();
        assert_eq!(analysis.format, FileFormat::Png);
        assert!(analysis.has_changes());
        let labels: Vec<&str> = analysis
            .findings
            .iter()
            .map(|finding| finding.label.as_str())
            .collect();
        assert_eq!(labels, vec!["Text entry: Comment"]);
    }

    #[test]
    fn rejected_files_get_a_reason() {
        let options = CleanOptions::default();
        let check = |bytes: Vec<u8>| {
            let len = bytes.len() as u64;
            analyze_reader(&mut Cursor::new(bytes), len, &options).unwrap_err()
        };
        assert_eq!(check(Vec::new()), "file is empty");
        assert_eq!(
            check(b"hello world, plain text".to_vec()),
            "not a supported photo or video file"
        );
        assert_eq!(check(ftyp(b"avif")), "AVIF files are not supported");
        assert!(check(b"BM nonsense, not a bitmap".to_vec()).contains("BMP"));
    }

    #[test]
    fn paths_are_checked() {
        let dir = temp_dir("paths");
        let options = CleanOptions::default();
        assert_eq!(
            analyze_path(&dir, &options).unwrap_err(),
            "folders are not supported"
        );
        assert_eq!(
            analyze_path(&dir.join("missing.png"), &options).unwrap_err(),
            "file not found or not accessible"
        );
        let file = dir.join("a.png");
        std::fs::write(&file, png_with_text()).unwrap();
        let analysis = analyze_path(&file, &options).unwrap();
        assert_eq!(analysis.format, FileFormat::Png);
        assert_eq!(analysis.source_len, png_with_text().len() as u64);
        let empty = dir.join("empty.png");
        std::fs::write(&empty, b"").unwrap();
        assert_eq!(analyze_path(&empty, &options).unwrap_err(), "file is empty");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn size_limits_depend_on_the_kind_of_file() {
        assert_eq!(size_limit(FileFormat::Jpeg), 256 * 1024 * 1024);
        assert_eq!(size_limit(FileFormat::Heic), 256 * 1024 * 1024);
        assert_eq!(size_limit(FileFormat::Mp4), 16 * 1024 * 1024 * 1024);
        assert_eq!(size_limit(FileFormat::Mov), 16 * 1024 * 1024 * 1024);
        assert_eq!(limit_text(MAX_IMAGE_BYTES), "256 MB");
        assert_eq!(limit_text(MAX_VIDEO_BYTES), "16 GB");
    }

    /// Needs the sample files of the owner. Run by hand:
    /// cargo test live_real_photos -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_real_photos_are_analyzed() {
        for name in ["A.jpg", "A.HEIC", "A.png"] {
            let path = PathBuf::from(format!("C:/dev/erxtools-testdata/Mediatest/{name}"));
            let analysis = analyze_path(&path, &CleanOptions::default())
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            println!(
                "{name}: format {:?}, changes {}, {} findings",
                analysis.format,
                analysis.has_changes(),
                analysis.findings.len()
            );
            for finding in &analysis.findings {
                println!(
                    "  {:?} | {} | {:?}",
                    finding.category, finding.label, finding.detail
                );
            }
            for note in &analysis.kept {
                println!("  kept: {note}");
            }
        }
    }
}
