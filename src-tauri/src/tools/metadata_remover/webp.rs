//! WebP handler: finds metadata chunks and plans their removal WITHOUT
//! touching the image data. Whole RIFF chunks are copied or dropped.
//!
//! Removed: EXIF, XMP, every unknown chunk and any data after the RIFF
//! container. Kept: the image chunks (VP8, VP8L, ALPH, ANIM, ANMF) and, by
//! option, the color profile (ICCP) and a fresh minimal EXIF chunk that
//! holds only the orientation. The flags in the VP8X header (which say
//! which metadata chunks exist) and the RIFF size are rewritten to match.

use std::io::{Read, Seek, SeekFrom};

use super::model::{Analysis, Category, CleanOptions, FileFormat, Finding, MAX_RAW_METADATA};
use super::orientation::{minimal_tiff, read_orientation};

const MAX_CHUNKS: u32 = 100_000;
const FLAG_ICC: u8 = 0x20;
const FLAG_EXIF: u8 = 0x08;
const FLAG_XMP: u8 = 0x04;

fn damaged() -> String {
    "damaged or incomplete WebP file".to_string()
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

/// A complete RIFF chunk: name, little-endian size, data and a padding byte
/// when the size is odd.
pub fn riff_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 9);
    out.extend_from_slice(kind);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
    out
}

fn printable(kind: &[u8; 4]) -> String {
    kind.iter()
        .map(|byte| {
            if byte.is_ascii_graphic() || *byte == b' ' {
                char::from(*byte)
            } else {
                '?'
            }
        })
        .collect()
}

/// One piece of the rebuilt file.
enum Piece {
    /// A byte range of the source file (start, end).
    Copy(u64, u64),
    Bytes(Vec<u8>),
}

impl Piece {
    fn size(&self) -> u64 {
        match self {
            Piece::Copy(start, end) => end.saturating_sub(*start),
            Piece::Bytes(bytes) => bytes.len() as u64,
        }
    }
}

/// Analyzes a WebP file of `len` bytes.
pub fn analyze<R: Read + Seek>(
    src: &mut R,
    len: u64,
    options: &CleanOptions,
) -> Result<Analysis, String> {
    if len < 12 {
        return Err(damaged());
    }
    let mut head = [0u8; 12];
    read_at(src, 0, &mut head)?;
    let [r0, r1, r2, r3, s0, s1, s2, s3, w0, w1, w2, w3] = head;
    if [r0, r1, r2, r3] != *b"RIFF" || [w0, w1, w2, w3] != *b"WEBP" {
        return Err("not a valid WebP file".to_string());
    }
    let riff_size = u64::from(u32::from_le_bytes([s0, s1, s2, s3]));
    let riff_end = 8 + riff_size;
    if riff_size < 4 || riff_end > len {
        return Err(damaged());
    }

    let mut analysis = Analysis::new(FileFormat::Webp, len);
    if len > riff_end {
        analysis.add_finding(
            Finding::new(Category::ExtraData, "Extra data after the image")
                .with_detail(&format!("{} bytes", len - riff_end)),
        );
    }

    let mut pieces: Vec<Piece> = Vec::new();
    let mut changed = false;
    let mut vp8x: Option<(u64, u8)> = None;
    let mut icc_removed = false;
    let mut exif_removed = false;
    let mut xmp_removed = false;
    let mut exif_kept = false;
    let mut orientation_seen = false;
    let mut pending: Option<u16> = None;
    let mut saw_image = false;
    let mut pos: u64 = 12;
    let mut index: u32 = 0;

    while pos < riff_end {
        index += 1;
        if index > MAX_CHUNKS {
            return Err(damaged());
        }
        if riff_end - pos < 8 {
            return Err(damaged());
        }
        let mut header = [0u8; 8];
        read_at(src, pos, &mut header)?;
        let [k0, k1, k2, k3, z0, z1, z2, z3] = header;
        let kind = [k0, k1, k2, k3];
        let size = u64::from(u32::from_le_bytes([z0, z1, z2, z3]));
        let end = pos + 8 + size + (size & 1);
        if end > riff_end {
            return Err(damaged());
        }
        let data_start = pos + 8;

        match &kind {
            b"VP8X" => {
                if index != 1 || size != 10 {
                    return Err("unusual WebP structure".to_string());
                }
                let mut flags = [0u8; 1];
                read_at(src, data_start, &mut flags)?;
                vp8x = Some((pos, flags[0]));
                pieces.push(Piece::Copy(pos, end));
            }
            b"ICCP" => {
                if options.keep_color_profile {
                    analysis.add_kept("Color profile (ICC)");
                    pieces.push(Piece::Copy(pos, end));
                } else {
                    analysis.add_finding(Finding::new(Category::Other, "Color profile (ICC)"));
                    icc_removed = true;
                    changed = true;
                }
            }
            b"ANIM" | b"ALPH" => pieces.push(Piece::Copy(pos, end)),
            b"ANMF" | b"VP8 " | b"VP8L" => {
                saw_image = true;
                pieces.push(Piece::Copy(pos, end));
            }
            b"EXIF" => {
                let mut tiff: Option<Vec<u8>> = None;
                if size <= MAX_RAW_METADATA as u64 {
                    let payload = read_payload(src, data_start, size)?;
                    let stripped = payload.strip_prefix(b"Exif\0\0").map(<[u8]>::to_vec);
                    tiff = Some(stripped.unwrap_or(payload));
                }
                if analysis.exif.is_none() {
                    analysis.exif = tiff.clone();
                }
                let mut handled = false;
                if options.keep_orientation && vp8x.is_some() && !orientation_seen {
                    orientation_seen = true;
                    let value = tiff
                        .as_deref()
                        .and_then(read_orientation)
                        .filter(|value| *value != 1);
                    if let Some(value) = value {
                        analysis.add_kept("Photo rotation (orientation)");
                        if size == 26 && tiff.as_deref() == Some(minimal_tiff(value).as_slice()) {
                            // Already exactly the minimal block: keep it in place.
                            pieces.push(Piece::Copy(pos, end));
                            exif_kept = true;
                            handled = true;
                        } else {
                            pending = Some(value);
                        }
                    }
                }
                if !handled {
                    analysis.add_finding(Finding::new(Category::Other, "EXIF metadata block"));
                    exif_removed = true;
                    changed = true;
                }
            }
            b"XMP " => {
                if analysis.xmp.is_none() && size <= MAX_RAW_METADATA as u64 {
                    analysis.xmp = Some(read_payload(src, data_start, size)?);
                }
                analysis.add_finding(Finding::new(Category::Xmp, "XMP metadata"));
                xmp_removed = true;
                changed = true;
            }
            _ => {
                analysis.add_finding(Finding::new(
                    Category::Other,
                    &format!("Private data chunk ({})", printable(&kind)),
                ));
                changed = true;
            }
        }
        pos = end;
    }

    if !saw_image {
        return Err(damaged());
    }

    // The new flags of the VP8X header.
    let mut new_flags: Option<u8> = None;
    if let Some((_, flags)) = vp8x {
        let mut updated = flags;
        if exif_removed {
            updated &= !FLAG_EXIF;
            if exif_kept || pending.is_some() {
                updated |= FLAG_EXIF;
            }
        }
        if xmp_removed {
            updated &= !FLAG_XMP;
        }
        if icc_removed {
            updated &= !FLAG_ICC;
        }
        if updated != flags {
            new_flags = Some(updated);
        }
    }

    if !changed {
        analysis.plan.copy(0, riff_end);
        return Ok(analysis);
    }

    if let (Some(flags), Some((start, _))) = (new_flags, vp8x) {
        let mut data = read_payload(src, start + 8, 10)?;
        if let Some(first) = data.first_mut() {
            *first = flags;
        }
        if let Some(slot) = pieces.first_mut() {
            *slot = Piece::Bytes(riff_chunk(b"VP8X", &data));
        }
    }
    if let Some(value) = pending {
        pieces.push(Piece::Bytes(riff_chunk(b"EXIF", &minimal_tiff(value))));
    }
    let body = pieces
        .iter()
        .fold(4u64, |total, piece| total.saturating_add(piece.size()));
    let new_size =
        u32::try_from(body).map_err(|_| "the file is too large to handle".to_string())?;
    let mut riff_header = b"RIFF".to_vec();
    riff_header.extend_from_slice(&new_size.to_le_bytes());
    riff_header.extend_from_slice(b"WEBP");
    analysis.plan.insert(riff_header);
    for piece in pieces {
        match piece {
            Piece::Copy(start, end) => analysis.plan.copy(start, end.saturating_sub(start)),
            Piece::Bytes(bytes) => analysis.plan.insert(bytes),
        }
    }
    Ok(analysis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::metadata_remover::executor::apply_plan;
    use crate::tools::metadata_remover::model::{TestRng, assert_plan_sane};
    use image::codecs::webp::WebPEncoder;
    use image::{ExtendedColorType, ImageEncoder};
    use std::io::Cursor;

    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        riff_chunk(kind, data)
    }

    /// A VP8X chunk for a 16 x 16 canvas with the given flags.
    fn vp8x(flags: u8) -> Vec<u8> {
        chunk(b"VP8X", &[flags, 0, 0, 0, 15, 0, 0, 15, 0, 0])
    }

    /// A fake image chunk with an odd size (so it has a padding byte).
    fn image_chunk() -> Vec<u8> {
        chunk(b"VP8L", &[1, 2, 3, 4, 5])
    }

    fn riff(chunks: &[Vec<u8>], trailing: &[u8]) -> Vec<u8> {
        let body: usize = chunks.iter().map(|part| part.len()).sum();
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&((4 + body) as u32).to_le_bytes());
        out.extend_from_slice(b"WEBP");
        for part in chunks {
            out.extend_from_slice(part);
        }
        out.extend_from_slice(trailing);
        out
    }

    fn exif_with_junk(orientation: u16, junk: &[u8]) -> Vec<u8> {
        let mut data = minimal_tiff(orientation);
        data.extend_from_slice(junk);
        data
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
    fn a_clean_simple_file_is_left_alone() {
        let data = riff(&[image_chunk()], &[]);
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert!(analysis.findings.is_empty());
        assert!(analysis.kept.is_empty());
        assert!(!analysis.has_changes());
        assert_eq!(out, data);
    }

    #[test]
    fn metadata_is_removed_and_the_flags_are_fixed() {
        let exif = exif_with_junk(6, b"SERIALNUMBER-1234");
        let data = riff(
            &[
                vp8x(0x2C),
                chunk(b"ICCP", b"PROFILE!"),
                image_chunk(),
                chunk(b"EXIF", &exif),
                chunk(b"XMP ", b"<x:xmpmeta/>"),
            ],
            &[],
        );
        let (analysis, out) = run(&data, &CleanOptions::default());
        let expected = riff(
            &[
                vp8x(0x28),
                chunk(b"ICCP", b"PROFILE!"),
                image_chunk(),
                chunk(b"EXIF", &minimal_tiff(6)),
            ],
            &[],
        );
        assert_eq!(out, expected);
        assert!(!contains(&out, b"SERIALNUMBER"));
        assert_eq!(
            labels(&analysis),
            vec!["EXIF metadata block", "XMP metadata"]
        );
        assert_eq!(
            analysis.kept,
            vec![
                "Color profile (ICC)".to_string(),
                "Photo rotation (orientation)".to_string()
            ]
        );
        assert_eq!(analysis.exif.as_deref(), Some(exif.as_slice()));
        assert_eq!(analysis.xmp.as_deref(), Some(&b"<x:xmpmeta/>"[..]));
    }

    #[test]
    fn orientation_and_color_profile_can_be_dropped() {
        let data = riff(
            &[
                vp8x(0x2C),
                chunk(b"ICCP", b"PROFILE!"),
                image_chunk(),
                chunk(b"EXIF", &exif_with_junk(6, b"SERIALNUMBER-1234")),
                chunk(b"XMP ", b"<x:xmpmeta/>"),
            ],
            &[],
        );
        let options = CleanOptions {
            keep_orientation: false,
            keep_color_profile: false,
        };
        let (analysis, out) = run(&data, &options);
        assert_eq!(out, riff(&[vp8x(0x00), image_chunk()], &[]));
        assert_eq!(
            labels(&analysis),
            vec!["Color profile (ICC)", "EXIF metadata block", "XMP metadata"]
        );
        assert!(analysis.kept.is_empty());
    }

    #[test]
    fn orientation_one_needs_no_chunk() {
        let data = riff(
            &[vp8x(0x08), image_chunk(), chunk(b"EXIF", &minimal_tiff(1))],
            &[],
        );
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert_eq!(out, riff(&[vp8x(0x00), image_chunk()], &[]));
        assert_eq!(labels(&analysis), vec!["EXIF metadata block"]);
        assert!(analysis.kept.is_empty());
    }

    #[test]
    fn an_already_minimal_chunk_stays_untouched() {
        let data = riff(
            &[vp8x(0x08), image_chunk(), chunk(b"EXIF", &minimal_tiff(8))],
            &[],
        );
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert!(analysis.findings.is_empty());
        assert!(!analysis.has_changes());
        assert_eq!(out, data);
        assert_eq!(
            analysis.kept,
            vec!["Photo rotation (orientation)".to_string()]
        );
    }

    #[test]
    fn an_exif_header_prefix_is_understood() {
        let mut prefixed = b"Exif\0\0".to_vec();
        prefixed.extend(minimal_tiff(6));
        let data = riff(&[vp8x(0x08), image_chunk(), chunk(b"EXIF", &prefixed)], &[]);
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert_eq!(
            out,
            riff(
                &[vp8x(0x08), image_chunk(), chunk(b"EXIF", &minimal_tiff(6))],
                &[]
            )
        );
        assert_eq!(analysis.exif.as_deref(), Some(minimal_tiff(6).as_slice()));
        assert_eq!(labels(&analysis), vec!["EXIF metadata block"]);
    }

    #[test]
    fn without_a_vp8x_header_no_orientation_is_written() {
        let data = riff(
            &[image_chunk(), chunk(b"EXIF", &exif_with_junk(6, b"AA"))],
            &[],
        );
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert_eq!(out, riff(&[image_chunk()], &[]));
        assert_eq!(labels(&analysis), vec!["EXIF metadata block"]);
        assert!(analysis.kept.is_empty());
    }

    #[test]
    fn unknown_chunks_are_removed() {
        let data = riff(&[vp8x(0x00), chunk(b"ABCD", b"xyz"), image_chunk()], &[]);
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert_eq!(out, riff(&[vp8x(0x00), image_chunk()], &[]));
        assert_eq!(labels(&analysis), vec!["Private data chunk (ABCD)"]);
    }

    #[test]
    fn data_after_the_container_is_removed() {
        let data = riff(&[image_chunk()], b"TAIL");
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert_eq!(out, riff(&[image_chunk()], &[]));
        assert_eq!(labels(&analysis), vec!["Extra data after the image"]);
        assert_eq!(analysis.findings[0].detail.as_deref(), Some("4 bytes"));
        assert!(analysis.has_changes());
    }

    #[test]
    fn hostile_inputs_are_rejected_not_crashed() {
        let options = CleanOptions::default();
        let check = |bytes: Vec<u8>| {
            let len = bytes.len() as u64;
            analyze(&mut Cursor::new(bytes), len, &options)
        };
        assert!(check(Vec::new()).is_err());
        assert!(check(b"hello world, this is no webp".to_vec()).is_err());

        let mut wave = riff(&[image_chunk()], &[]);
        wave[8..12].copy_from_slice(b"WAVE");
        assert!(check(wave).is_err());

        let mut too_big = riff(&[image_chunk()], &[]);
        too_big[4..8].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
        assert!(check(too_big).is_err());

        let mut tiny_size = b"RIFF".to_vec();
        tiny_size.extend_from_slice(&3u32.to_le_bytes());
        tiny_size.extend_from_slice(b"WEBP");
        assert!(check(tiny_size).is_err());

        let mut long_chunk = b"RIFF".to_vec();
        long_chunk.extend_from_slice(&15u32.to_le_bytes());
        long_chunk.extend_from_slice(b"WEBP");
        long_chunk.extend_from_slice(b"VP8L");
        long_chunk.extend_from_slice(&1000u32.to_le_bytes());
        long_chunk.extend_from_slice(&[1, 2, 3]);
        assert!(check(long_chunk).is_err());

        let wrong_size_vp8x = riff(&[chunk(b"VP8X", &[0u8; 12]), image_chunk()], &[]);
        assert!(check(wrong_size_vp8x).is_err());

        let late_vp8x = riff(&[image_chunk(), vp8x(0x00)], &[]);
        assert!(check(late_vp8x).is_err());

        let no_image = riff(&[chunk(b"EXIF", &minimal_tiff(6))], &[]);
        assert!(check(no_image).is_err());

        let mut leftover = riff(&[image_chunk()], &[]);
        leftover.extend_from_slice(&[1, 2, 3]);
        let new_size = (leftover.len() - 8) as u32;
        leftover[4..8].copy_from_slice(&new_size.to_le_bytes());
        assert!(check(leftover).is_err());
    }

    #[test]
    fn every_truncation_is_rejected_or_handled() {
        let base = riff(
            &[
                vp8x(0x08),
                image_chunk(),
                chunk(b"EXIF", &exif_with_junk(6, b"JUNK")),
                chunk(b"XMP ", b"<x:xmpmeta/>"),
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
        let base = riff(
            &[
                vp8x(0x2C),
                chunk(b"ICCP", b"PROFILE!"),
                image_chunk(),
                chunk(b"EXIF", &exif_with_junk(6, b"JUNK")),
                chunk(b"XMP ", b"<x:xmpmeta/>"),
            ],
            b"TAIL",
        );
        let options = CleanOptions::default();
        let mut rng = TestRng::new(0x1234_5678_9ABC_DEF1);
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
        WebPEncoder::new_lossless(&mut encoded)
            .write_image(image.as_raw(), 16, 16, ExtendedColorType::Rgb8)
            .unwrap();
        let base = encoded.into_inner();
        assert_eq!(&base[0..4], b"RIFF");
        let riff_size = u32::from_le_bytes([base[4], base[5], base[6], base[7]]) as usize;
        let mut chunks: &[u8] = &base[12..8 + riff_size];
        if chunks.starts_with(b"VP8X") {
            chunks = &chunks[18..];
        }

        let mut body = vp8x(0x08);
        body.extend_from_slice(chunks);
        body.extend(chunk(b"EXIF", &exif_with_junk(6, b"SERIALNUMBER-1234")));
        let mut data = b"RIFF".to_vec();
        data.extend_from_slice(&((4 + body.len()) as u32).to_le_bytes());
        data.extend_from_slice(b"WEBP");
        data.extend(body);
        data.extend_from_slice(b"TRAILING-DATA");

        let (analysis, cleaned) = run(&data, &CleanOptions::default());
        assert!(analysis.has_changes());
        assert!(!contains(&cleaned, b"SERIALNUMBER"));
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
