//! JPEG handler: finds metadata segments and plans their removal WITHOUT
//! touching the compressed image data.
//!
//! The file is walked segment by segment up to the start of the scan; the
//! scan data (the picture itself) is only searched for the next marker and
//! copied unchanged. Removed: EXIF (with its thumbnail and maker notes),
//! XMP, Photoshop/IPTC, comments, every other APPn segment, the JFIF
//! thumbnail and any data after the end of the image (EOI). Kept: the JFIF
//! header (without a thumbnail), the Adobe color transform, the structural
//! segments, and (by option) the color profile and a minimal orientation.

use std::io::{Read, Seek, SeekFrom};

use super::model::{Analysis, Category, CleanOptions, FileFormat, Finding, MAX_RAW_METADATA};
use super::orientation::{jpeg_exif_segment, read_orientation};

/// A real JPEG has a few dozen segments; more means a damaged file.
const MAX_SEGMENTS: u32 = 20_000;
/// Extra 0xFF bytes allowed in front of one marker.
const MAX_FILL_BYTES: u32 = 4096;
const SCAN_CHUNK_BYTES: usize = 64 * 1024;
/// How many payload bytes are looked at to recognize a segment.
const PREFIX_BYTES: u64 = 64;

const EXIF_HEADER: &[u8] = b"Exif\0\0";
const XMP_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const XMP_EXTENSION_HEADER: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
const ICC_HEADER: &[u8] = b"ICC_PROFILE\0";
const PHOTOSHOP_HEADER: &[u8] = b"Photoshop 3.0\0";

fn damaged() -> String {
    "damaged or incomplete JPEG file".to_string()
}

fn read_at<R: Read + Seek>(src: &mut R, pos: u64, buf: &mut [u8]) -> Result<(), String> {
    src.seek(SeekFrom::Start(pos)).map_err(|_| damaged())?;
    src.read_exact(buf).map_err(|_| damaged())
}

fn read_payload<R: Read + Seek>(src: &mut R, start: u64, len: u64) -> Result<Vec<u8>, String> {
    let size = usize::try_from(len).map_err(|_| damaged())?;
    let mut payload = vec![0u8; size];
    read_at(src, start, &mut payload)?;
    Ok(payload)
}

fn is_frame_marker(marker: u8) -> bool {
    matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF)
}

/// Segments that describe the picture and are always kept: the frame
/// header (SOFn), Huffman and arithmetic tables, quantization tables, DNL
/// and restart interval.
fn is_structural(marker: u8) -> bool {
    matches!(marker, 0xC0..=0xC7 | 0xC9..=0xCF | 0xDB..=0xDD)
}

/// Reads the marker at `pos` and returns (marker byte, position right after
/// the marker). Extra 0xFF fill bytes in front of the marker are allowed.
fn read_marker<R: Read + Seek>(src: &mut R, pos: u64, len: u64) -> Result<(u8, u64), String> {
    let mut byte = [0u8; 1];
    read_at(src, pos, &mut byte)?;
    if byte[0] != 0xFF {
        return Err(damaged());
    }
    let mut p = pos + 1;
    let mut fill = 0u32;
    loop {
        if p >= len {
            return Err(damaged());
        }
        read_at(src, p, &mut byte)?;
        if byte[0] != 0xFF {
            return Ok((byte[0], p + 1));
        }
        fill += 1;
        if fill > MAX_FILL_BYTES {
            return Err(damaged());
        }
        p += 1;
    }
}

/// Finds where the entropy-coded scan data that starts at `start` ends: the
/// position of the next real marker. Stuffed bytes (FF 00), restart markers
/// (FF D0 to FF D7) and fill bytes (FF FF) belong to the data.
fn find_scan_end<R: Read + Seek>(src: &mut R, start: u64, len: u64) -> Result<u64, String> {
    let mut buffer = vec![0u8; SCAN_CHUNK_BYTES];
    let mut q = start;
    'chunks: loop {
        if q >= len {
            return Err(damaged());
        }
        let take = (len - q).min(SCAN_CHUNK_BYTES as u64) as usize;
        let chunk = buffer.get_mut(..take).ok_or_else(damaged)?;
        read_at(src, q, chunk)?;
        let more_after = q + (take as u64) < len;
        let mut i = 0usize;
        while i < take {
            if chunk.get(i).copied() != Some(0xFF) {
                i += 1;
                continue;
            }
            match chunk.get(i + 1).copied() {
                Some(0x00) | Some(0xD0..=0xD7) => i += 2,
                Some(0xFF) => i += 1,
                Some(_) => return Ok(q + i as u64),
                None => {
                    // The 0xFF is the last byte of this chunk: look at it
                    // again at the start of the next chunk.
                    if more_after {
                        q += i as u64;
                        continue 'chunks;
                    }
                    return Err(damaged());
                }
            }
        }
        q += take as u64;
    }
}

/// Analyzes a JPEG file of `len` bytes.
pub fn analyze<R: Read + Seek>(
    src: &mut R,
    len: u64,
    options: &CleanOptions,
) -> Result<Analysis, String> {
    if len < 4 {
        return Err(damaged());
    }
    let mut head = [0u8; 2];
    read_at(src, 0, &mut head)?;
    if head != [0xFF, 0xD8] {
        return Err("not a valid JPEG file".to_string());
    }
    let mut analysis = Analysis::new(FileFormat::Jpeg, len);
    analysis.plan.copy(0, 2);

    let mut pos: u64 = 2;
    let mut segments: u32 = 0;
    let mut saw_frame = false;
    let mut saw_scan = false;
    let mut orientation_done = false;

    loop {
        segments += 1;
        if segments > MAX_SEGMENTS {
            return Err(damaged());
        }
        let marker_pos = pos;
        let (marker, body_start) = read_marker(src, marker_pos, len)?;

        match marker {
            0x00 | 0xD8 | 0xD0..=0xD7 => return Err(damaged()),
            0x01 => {
                analysis.plan.copy(marker_pos, body_start - marker_pos);
                pos = body_start;
                continue;
            }
            0xD9 => {
                analysis.plan.copy(marker_pos, body_start - marker_pos);
                let extra = len - body_start;
                if extra > 0 {
                    analysis.add_finding(
                        Finding::new(Category::ExtraData, "Extra data after the image")
                            .with_detail(&format!("{extra} bytes")),
                    );
                }
                break;
            }
            _ => {}
        }

        let mut length_bytes = [0u8; 2];
        read_at(src, body_start, &mut length_bytes)?;
        let segment_len = u64::from(u16::from_be_bytes(length_bytes));
        if segment_len < 2 {
            return Err(damaged());
        }
        let segment_end = body_start + segment_len;
        if segment_end > len {
            return Err(damaged());
        }
        let payload_start = body_start + 2;
        let payload_len = segment_len - 2;
        let prefix_len = payload_len.min(PREFIX_BYTES) as usize;
        let mut prefix = vec![0u8; prefix_len];
        read_at(src, payload_start, &mut prefix)?;

        match marker {
            0xDA => {
                if !saw_frame {
                    return Err(damaged());
                }
                saw_scan = true;
                analysis.plan.copy(marker_pos, segment_end - marker_pos);
                let scan_end = find_scan_end(src, segment_end, len)?;
                analysis.plan.copy(segment_end, scan_end - segment_end);
                pos = scan_end;
                continue;
            }
            m if is_structural(m) => {
                if is_frame_marker(m) {
                    saw_frame = true;
                }
                analysis.plan.copy(marker_pos, segment_end - marker_pos);
            }
            0xE0 => {
                if prefix.starts_with(b"JFIF\0") {
                    analysis.add_kept("JFIF header");
                    let thumb_w = prefix.get(12).copied().unwrap_or(0);
                    let thumb_h = prefix.get(13).copied().unwrap_or(0);
                    if payload_len >= 14 && (thumb_w != 0 || thumb_h != 0) {
                        // Rebuild the header without its thumbnail.
                        let mut rebuilt = vec![0xFF, 0xE0, 0x00, 0x10];
                        rebuilt.extend_from_slice(prefix.get(..12).unwrap_or(&[]));
                        rebuilt.extend_from_slice(&[0, 0]);
                        analysis.plan.insert(rebuilt);
                        analysis.add_finding(Finding::new(
                            Category::Thumbnail,
                            "Thumbnail inside the JFIF header",
                        ));
                    } else {
                        analysis.plan.copy(marker_pos, segment_end - marker_pos);
                    }
                } else if prefix.starts_with(b"JFXX\0") {
                    analysis.add_finding(Finding::new(
                        Category::Thumbnail,
                        "JFIF extension thumbnail",
                    ));
                } else {
                    analysis.add_finding(Finding::new(Category::Other, "Application data (APP0)"));
                }
            }
            0xE1 => {
                if prefix.starts_with(EXIF_HEADER) {
                    let payload = read_payload(src, payload_start, payload_len)?;
                    let tiff = payload.get(EXIF_HEADER.len()..).unwrap_or(&[]);
                    if analysis.exif.is_none() && tiff.len() <= MAX_RAW_METADATA {
                        analysis.exif = Some(tiff.to_vec());
                    }
                    let wanted = if options.keep_orientation && !orientation_done {
                        read_orientation(tiff).filter(|value| *value != 1)
                    } else {
                        None
                    };
                    match wanted {
                        Some(value) => {
                            orientation_done = true;
                            analysis.add_kept("Photo rotation (orientation)");
                            let segment = jpeg_exif_segment(value);
                            let plain = body_start - marker_pos == 2;
                            if plain && segment.get(4..) == Some(payload.as_slice()) {
                                // Already exactly the minimal block.
                                analysis.plan.copy(marker_pos, segment_end - marker_pos);
                            } else {
                                analysis.plan.insert(segment);
                                analysis.add_finding(Finding::new(
                                    Category::Other,
                                    "EXIF metadata block",
                                ));
                            }
                        }
                        None => {
                            analysis
                                .add_finding(Finding::new(Category::Other, "EXIF metadata block"));
                        }
                    }
                } else if prefix.starts_with(XMP_HEADER) {
                    if analysis.xmp.is_none() {
                        let payload = read_payload(src, payload_start, payload_len)?;
                        if let Some(text) = payload.get(XMP_HEADER.len()..)
                            && text.len() <= MAX_RAW_METADATA
                        {
                            analysis.xmp = Some(text.to_vec());
                        }
                    }
                    analysis.add_finding(Finding::new(Category::Xmp, "XMP metadata"));
                } else if prefix.starts_with(XMP_EXTENSION_HEADER) {
                    analysis.add_finding(Finding::new(Category::Xmp, "XMP extension data"));
                } else {
                    analysis.add_finding(Finding::new(Category::Other, "Application data (APP1)"));
                }
            }
            0xE2 => {
                if prefix.starts_with(ICC_HEADER) {
                    if options.keep_color_profile {
                        analysis.add_kept("Color profile (ICC)");
                        analysis.plan.copy(marker_pos, segment_end - marker_pos);
                    } else {
                        analysis.add_finding(Finding::new(Category::Other, "Color profile (ICC)"));
                    }
                } else if prefix.starts_with(b"MPF\0") {
                    analysis.add_finding(Finding::new(
                        Category::ExtraData,
                        "Multi-picture data (MPF)",
                    ));
                } else {
                    analysis.add_finding(Finding::new(Category::Other, "Application data (APP2)"));
                }
            }
            0xED => {
                if prefix.starts_with(PHOTOSHOP_HEADER) {
                    analysis.add_finding(Finding::new(Category::Iptc, "Photoshop / IPTC data"));
                } else {
                    analysis.add_finding(Finding::new(Category::Other, "Application data (APP13)"));
                }
            }
            0xEE => {
                if prefix.starts_with(b"Adobe") {
                    analysis.add_kept("Adobe color information");
                    analysis.plan.copy(marker_pos, segment_end - marker_pos);
                } else {
                    analysis.add_finding(Finding::new(Category::Other, "Application data (APP14)"));
                }
            }
            0xFE => {
                let mut finding = Finding::new(Category::Comment, "JPEG comment");
                let text: String = String::from_utf8_lossy(&prefix)
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect();
                let text = text.trim();
                if !text.is_empty() {
                    finding = finding.with_detail(text);
                }
                analysis.add_finding(finding);
            }
            0xE3..=0xEC | 0xEF => {
                analysis.add_finding(Finding::new(
                    Category::Other,
                    &format!("Application data (APP{})", marker - 0xE0),
                ));
            }
            _ => return Err("unusual JPEG structure".to_string()),
        }
        pos = segment_end;
    }

    if !saw_frame || !saw_scan {
        return Err(damaged());
    }
    Ok(analysis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::metadata_remover::executor::apply_plan;
    use crate::tools::metadata_remover::model::{TestRng, assert_plan_sane};
    use crate::tools::metadata_remover::orientation::minimal_tiff;
    use image::codecs::jpeg::JpegEncoder;
    use image::{ExtendedColorType, ImageEncoder};
    use std::io::Cursor;

    fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0xFF, marker];
        out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    /// A JFIF APP0 segment with an embedded thumbnail of the given size.
    fn jfif(thumb: (u8, u8)) -> Vec<u8> {
        let mut payload = b"JFIF\0".to_vec();
        payload.extend_from_slice(&[1, 1, 1, 0, 72, 0, 72, thumb.0, thumb.1]);
        payload.extend(vec![0xABu8; 3 * thumb.0 as usize * thumb.1 as usize]);
        segment(0xE0, &payload)
    }

    fn dqt() -> Vec<u8> {
        let mut payload = vec![0u8];
        payload.extend(vec![16u8; 64]);
        segment(0xDB, &payload)
    }

    fn sof0() -> Vec<u8> {
        segment(0xC0, &[8, 0, 1, 0, 1, 1, 1, 0x11, 0])
    }

    fn sos() -> Vec<u8> {
        segment(0xDA, &[1, 1, 0x00, 0, 63, 0])
    }

    /// Tables, frame header, scan header, a little scan data and EOI.
    fn core_part() -> Vec<u8> {
        let mut out = dqt();
        out.extend(sof0());
        out.extend(sos());
        out.extend_from_slice(&[0x12, 0xFF, 0x00, 0x34, 0xFF, 0xD0, 0x56, 0xFF, 0xD9]);
        out
    }

    fn fake_jpeg(extras: &[Vec<u8>], trailing: &[u8]) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8];
        for extra in extras {
            out.extend_from_slice(extra);
        }
        out.extend(core_part());
        out.extend_from_slice(trailing);
        out
    }

    fn exif_payload() -> Vec<u8> {
        let mut payload = b"Exif\0\0".to_vec();
        payload.extend(minimal_tiff(6));
        payload.extend_from_slice(b"JUNK-SERIALNUMBER-12345");
        payload
    }

    fn run(data: &[u8], options: &CleanOptions) -> (Analysis, Vec<u8>) {
        let analysis = analyze(&mut Cursor::new(data.to_vec()), data.len() as u64, options)
            .expect("analysis must succeed");
        let mut out: Vec<u8> = Vec::new();
        apply_plan(
            &mut Cursor::new(data.to_vec()),
            &analysis.plan,
            &mut out,
            &mut |_, _| true,
        )
        .expect("plan must apply");
        (analysis, out)
    }

    fn labels(analysis: &Analysis) -> Vec<&str> {
        analysis
            .findings
            .iter()
            .map(|finding| finding.label.as_str())
            .collect()
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    #[test]
    fn a_clean_file_is_left_alone() {
        let data = fake_jpeg(&[jfif((0, 0))], &[]);
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert!(analysis.findings.is_empty());
        assert!(!analysis.has_changes());
        assert_eq!(analysis.kept, vec!["JFIF header".to_string()]);
        assert_eq!(out, data);
    }

    #[test]
    fn extra_fill_bytes_before_a_marker_are_kept() {
        let data = fake_jpeg(&[vec![0xFF], jfif((0, 0))], &[]);
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert!(!analysis.has_changes());
        assert_eq!(out, data);
    }

    #[test]
    fn metadata_is_removed_and_the_important_parts_are_kept() {
        let icc = segment(0xE2, b"ICC_PROFILE\0\x01\x01PROFILEDATA");
        let adobe = segment(0xEE, b"Adobe\x00\x64\x00\x00\x00\x00\x01");
        let mut xmp = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
        xmp.extend_from_slice(b"<x:xmpmeta/>");
        let extras = vec![
            jfif((2, 1)),
            segment(0xE1, &exif_payload()),
            segment(0xE1, &xmp),
            segment(0xED, b"Photoshop 3.0\0abc"),
            icc.clone(),
            adobe.clone(),
            segment(0xFE, b"hello"),
        ];
        let data = fake_jpeg(&extras, b"TRAILING-DATA");
        let (analysis, out) = run(&data, &CleanOptions::default());

        let expected = fake_jpeg(&[jfif((0, 0)), jpeg_exif_segment(6), icc, adobe], &[]);
        assert_eq!(out, expected);
        assert!(!contains(&out, b"SERIALNUMBER"));
        assert!(!contains(&out, b"hello"));
        assert!(!contains(&out, b"TRAILING"));

        assert_eq!(
            labels(&analysis),
            vec![
                "Thumbnail inside the JFIF header",
                "EXIF metadata block",
                "XMP metadata",
                "Photoshop / IPTC data",
                "JPEG comment",
                "Extra data after the image",
            ]
        );
        assert_eq!(
            analysis.kept,
            vec![
                "JFIF header".to_string(),
                "Photo rotation (orientation)".to_string(),
                "Color profile (ICC)".to_string(),
                "Adobe color information".to_string(),
            ]
        );
        let comment = analysis
            .findings
            .iter()
            .find(|finding| finding.label == "JPEG comment")
            .unwrap();
        assert_eq!(comment.detail.as_deref(), Some("hello"));
        let extra = analysis
            .findings
            .iter()
            .find(|finding| finding.label == "Extra data after the image")
            .unwrap();
        assert_eq!(extra.detail.as_deref(), Some("13 bytes"));
        assert_eq!(analysis.exif.as_deref(), Some(&exif_payload()[6..]));
        assert_eq!(analysis.xmp.as_deref(), Some(&b"<x:xmpmeta/>"[..]));
    }

    #[test]
    fn the_orientation_can_be_dropped_too() {
        let data = fake_jpeg(&[jfif((0, 0)), segment(0xE1, &exif_payload())], &[]);
        let options = CleanOptions {
            keep_orientation: false,
            keep_color_profile: true,
        };
        let (analysis, out) = run(&data, &options);
        assert_eq!(out, fake_jpeg(&[jfif((0, 0))], &[]));
        assert_eq!(labels(&analysis), vec!["EXIF metadata block"]);
        assert_eq!(analysis.kept, vec!["JFIF header".to_string()]);
    }

    #[test]
    fn orientation_one_needs_no_block() {
        let mut payload = b"Exif\0\0".to_vec();
        payload.extend(minimal_tiff(1));
        let data = fake_jpeg(&[jfif((0, 0)), segment(0xE1, &payload)], &[]);
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert_eq!(out, fake_jpeg(&[jfif((0, 0))], &[]));
        assert_eq!(labels(&analysis), vec!["EXIF metadata block"]);
    }

    #[test]
    fn an_already_minimal_block_is_kept_untouched() {
        let data = fake_jpeg(&[jfif((0, 0)), jpeg_exif_segment(3)], &[]);
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert!(analysis.findings.is_empty());
        assert!(!analysis.has_changes());
        assert_eq!(out, data);
        assert!(
            analysis
                .kept
                .contains(&"Photo rotation (orientation)".to_string())
        );
    }

    #[test]
    fn the_color_profile_can_be_dropped() {
        let icc = segment(0xE2, b"ICC_PROFILE\0\x01\x01PROFILEDATA");
        let data = fake_jpeg(&[jfif((0, 0)), icc], &[]);
        let options = CleanOptions {
            keep_orientation: true,
            keep_color_profile: false,
        };
        let (analysis, out) = run(&data, &options);
        assert_eq!(out, fake_jpeg(&[jfif((0, 0))], &[]));
        assert_eq!(labels(&analysis), vec!["Color profile (ICC)"]);
        assert_eq!(analysis.kept, vec!["JFIF header".to_string()]);
    }

    #[test]
    fn other_application_segments_are_removed() {
        let extras = vec![
            jfif((0, 0)),
            segment(0xE0, b"JFXX\0\x10abc"),
            segment(0xE3, b"Meta\0abc"),
            segment(0xEF, b"zzz"),
            segment(0xE2, b"MPF\0abcdef"),
        ];
        let data = fake_jpeg(&extras, &[]);
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert_eq!(out, fake_jpeg(&[jfif((0, 0))], &[]));
        assert_eq!(
            labels(&analysis),
            vec![
                "JFIF extension thumbnail",
                "Application data (APP3)",
                "Application data (APP15)",
                "Multi-picture data (MPF)",
            ]
        );
    }

    #[test]
    fn markers_are_found_across_chunk_boundaries() {
        let mut data = vec![0xFF, 0xD8];
        data.extend(dqt());
        data.extend(sof0());
        data.extend(sos());
        let mut entropy = vec![0x11u8; 150_000];
        // A stuffed byte that straddles the end of the first 64 KiB chunk.
        entropy[65_535] = 0xFF;
        entropy[65_536] = 0x00;
        // A restart marker in the middle of the data.
        entropy[100_000] = 0xFF;
        entropy[100_001] = 0xD3;
        data.extend(entropy);
        data.extend_from_slice(&[0xFF, 0xD9]);
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert!(analysis.findings.is_empty());
        assert!(!analysis.has_changes());
        assert_eq!(out, data);
    }

    #[test]
    fn hostile_inputs_are_rejected_not_crashed() {
        let options = CleanOptions::default();
        let check = |bytes: Vec<u8>| {
            let len = bytes.len() as u64;
            analyze(&mut Cursor::new(bytes), len, &options)
        };
        assert!(check(Vec::new()).is_err());
        assert!(check(b"hello world".to_vec()).is_err());
        assert!(check(vec![0xFF, 0xD8]).is_err());
        assert!(check(vec![0xFF, 0xD8, 0xFF, 0xD9]).is_err());
        assert!(check(vec![0xFF; 10_000]).is_err());
        // Segment length 1 and a length that reaches beyond the file.
        assert!(check(vec![0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x01, 0x00, 0x00]).is_err());
        assert!(check(vec![0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFF, 0x00, 0x00]).is_err());
        // Marker 00, a second SOI, a restart marker in the header.
        assert!(check(vec![0xFF, 0xD8, 0xFF, 0x00, 0x00, 0x00]).is_err());
        assert!(check(vec![0xFF, 0xD8, 0xFF, 0xD8, 0x00, 0x00]).is_err());
        assert!(check(vec![0xFF, 0xD8, 0xFF, 0xD0, 0x00, 0x00]).is_err());
        // Thousands of fill bytes.
        let mut flood = vec![0xFF, 0xD8];
        flood.extend(vec![0xFF; 5_000]);
        assert!(check(flood).is_err());
        // A reserved marker.
        let mut reserved = vec![0xFF, 0xD8];
        reserved.extend(segment(0xC8, &[0, 0]));
        reserved.extend_from_slice(&[0xFF, 0xD9]);
        assert!(check(reserved).is_err());
        // A scan before any frame header.
        let mut early_scan = vec![0xFF, 0xD8];
        early_scan.extend(sos());
        early_scan.extend_from_slice(&[1, 2, 3, 0xFF, 0xD9]);
        assert!(check(early_scan).is_err());
    }

    #[test]
    fn every_truncation_is_rejected_or_handled() {
        let base = fake_jpeg(
            &[
                jfif((2, 1)),
                segment(0xE1, &exif_payload()),
                segment(0xFE, b"hello"),
            ],
            b"TAIL",
        );
        let options = CleanOptions::default();
        let complete_from = base.len() - 4;
        for length in 0..=base.len() {
            let result = analyze(
                &mut Cursor::new(base[..length].to_vec()),
                length as u64,
                &options,
            );
            if length >= complete_from {
                assert!(result.is_ok(), "prefix {length} should parse");
            } else {
                assert!(result.is_err(), "prefix {length} should be rejected");
            }
        }
    }

    #[test]
    fn random_damage_never_panics_and_plans_stay_sane() {
        let base = fake_jpeg(
            &[
                jfif((2, 1)),
                segment(0xE1, &exif_payload()),
                segment(0xFE, b"hello"),
                segment(0xED, b"Photoshop 3.0\0abc"),
            ],
            b"TAIL",
        );
        let options = CleanOptions::default();
        let mut rng = TestRng::new(0x2545_F491_4F6C_DD1D);
        for _ in 0..3000 {
            let mut data = base.clone();
            let flips = 1 + (rng.next_u64() % 3) as usize;
            for _ in 0..flips {
                let index = (rng.next_u64() % data.len() as u64) as usize;
                data[index] = (rng.next_u64() & 0xFF) as u8;
            }
            if let Ok(analysis) =
                analyze(&mut Cursor::new(data.clone()), data.len() as u64, &options)
            {
                assert_plan_sane(&analysis);
            }
        }
    }

    #[test]
    fn real_encoder_output_cleans_without_touching_the_pixels() {
        let image = image::RgbImage::from_fn(16, 16, |x, y| {
            image::Rgb([(x * 16) as u8, (y * 16) as u8, 128])
        });
        let mut encoded = Cursor::new(Vec::new());
        JpegEncoder::new_with_quality(&mut encoded, 90)
            .write_image(image.as_raw(), 16, 16, ExtendedColorType::Rgb8)
            .unwrap();
        let base = encoded.into_inner();

        let mut data = base[..2].to_vec();
        data.extend(segment(0xE1, &exif_payload()));
        data.extend(segment(0xFE, b"hello"));
        data.extend_from_slice(&base[2..]);
        data.extend_from_slice(b"TRAILING-DATA");

        let (analysis, cleaned) = run(&data, &CleanOptions::default());
        assert!(analysis.has_changes());
        assert!(!contains(&cleaned, b"SERIALNUMBER"));
        assert!(!contains(&cleaned, b"hello"));
        assert!(!contains(&cleaned, b"TRAILING"));

        let before = image::load_from_memory(&base).unwrap().to_rgb8();
        let after = image::load_from_memory(&cleaned).unwrap().to_rgb8();
        assert_eq!(before.as_raw(), after.as_raw());

        // Cleaning the cleaned file again changes nothing.
        let (again, again_out) = run(&cleaned, &CleanOptions::default());
        assert!(!again.has_changes());
        assert!(again.findings.is_empty());
        assert_eq!(again_out, cleaned);
    }
}
