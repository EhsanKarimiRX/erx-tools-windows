//! GIF handler: finds metadata blocks and plans their removal WITHOUT
//! touching the picture data. Blocks are copied or dropped as a whole.
//!
//! Removed: comment extensions, XMP, every other unknown application
//! extension, unknown extensions and any data after the trailer. Kept: the
//! header, the color tables, the frames, graphic control (timing and
//! transparency), text drawn on the picture (plain text extension), the loop
//! setting (NETSCAPE / ANIMEXTS) and, by option, the color profile.

use std::io::{Read, Seek, SeekFrom};

use super::model::{Analysis, Category, CleanOptions, FileFormat, Finding};

const MAX_BLOCKS: u32 = 1_000_000;
const MAX_SUB_BLOCKS: u32 = 4_000_000;
const MAX_DETAIL_CHARS: usize = 120;

fn damaged() -> String {
    "damaged or incomplete GIF file".to_string()
}

fn read_at<R: Read + Seek>(src: &mut R, pos: u64, buf: &mut [u8]) -> Result<(), String> {
    src.seek(SeekFrom::Start(pos)).map_err(|_| damaged())?;
    src.read_exact(buf).map_err(|_| damaged())
}

fn read_byte<R: Read + Seek>(src: &mut R, pos: u64) -> Result<u8, String> {
    let mut byte = [0u8; 1];
    read_at(src, pos, &mut byte)?;
    Ok(byte[0])
}

/// Walks the chain of data sub-blocks that starts at `pos` (each one: a
/// length byte and that many bytes; a zero length ends the chain). Returns
/// the position right after the terminator.
fn skip_sub_blocks<R: Read + Seek>(
    src: &mut R,
    start: u64,
    len: u64,
    budget: &mut u32,
) -> Result<u64, String> {
    let mut pos = start;
    loop {
        if pos >= len {
            return Err(damaged());
        }
        *budget += 1;
        if *budget > MAX_SUB_BLOCKS {
            return Err(damaged());
        }
        let size = u64::from(read_byte(src, pos)?);
        pos += 1;
        if size == 0 {
            return Ok(pos);
        }
        pos += size;
        if pos > len {
            return Err(damaged());
        }
    }
}

/// The 11 bytes (8 name + 3 authentication) at the start of an application
/// extension, or None when the first sub-block has another size.
fn application_id<R: Read + Seek>(src: &mut R, pos: u64) -> Result<Option<[u8; 11]>, String> {
    if read_byte(src, pos)? != 11 {
        return Ok(None);
    }
    let mut id = [0u8; 11];
    read_at(src, pos + 1, &mut id)?;
    Ok(Some(id))
}

fn printable(bytes: &[u8]) -> String {
    let text: String = bytes
        .iter()
        .map(|byte| {
            if byte.is_ascii_graphic() || *byte == b' ' {
                char::from(*byte)
            } else {
                '?'
            }
        })
        .collect();
    text.trim().to_string()
}

/// Analyzes a GIF file of `len` bytes.
pub fn analyze<R: Read + Seek>(
    src: &mut R,
    len: u64,
    options: &CleanOptions,
) -> Result<Analysis, String> {
    if len < 13 {
        return Err(damaged());
    }
    let mut head = [0u8; 13];
    read_at(src, 0, &mut head)?;
    if &head[0..6] != b"GIF87a" && &head[0..6] != b"GIF89a" {
        return Err("not a valid GIF file".to_string());
    }
    let packed = head[10];
    let table = if packed & 0x80 != 0 {
        3u64 << ((packed & 0x07) + 1)
    } else {
        0
    };
    let mut pos: u64 = 13 + table;
    if pos > len {
        return Err(damaged());
    }

    let mut analysis = Analysis::new(FileFormat::Gif, len);
    analysis.plan.copy(0, pos);
    let mut blocks: u32 = 0;
    let mut budget: u32 = 0;
    let mut saw_image = false;

    loop {
        blocks += 1;
        if blocks > MAX_BLOCKS {
            return Err(damaged());
        }
        if pos >= len {
            return Err(damaged());
        }
        let start = pos;
        let marker = read_byte(src, pos)?;
        match marker {
            0x3B => {
                analysis.plan.copy(start, 1);
                let end = start + 1;
                if len > end {
                    analysis.add_finding(
                        Finding::new(Category::ExtraData, "Extra data after the image")
                            .with_detail(&format!("{} bytes", len - end)),
                    );
                }
                break;
            }
            0x2C => {
                saw_image = true;
                let mut descriptor = [0u8; 9];
                read_at(src, pos + 1, &mut descriptor)?;
                let mut p = pos + 10;
                if descriptor[8] & 0x80 != 0 {
                    p += 3u64 << ((descriptor[8] & 0x07) + 1);
                }
                // One byte for the LZW minimum code size, then the data.
                let end = skip_sub_blocks(src, p + 1, len, &mut budget)?;
                analysis.plan.copy(start, end - start);
                pos = end;
            }
            0x21 => {
                let label = read_byte(src, pos + 1)?;
                let sub_start = pos + 2;
                let end = skip_sub_blocks(src, sub_start, len, &mut budget)?;
                let keep = match label {
                    0xF9 => true,
                    0x01 => {
                        analysis.add_kept("Text drawn on the picture");
                        true
                    }
                    0xFE => {
                        let mut finding = Finding::new(Category::Comment, "GIF comment");
                        let size = u64::from(read_byte(src, sub_start)?);
                        if size > 0 {
                            let mut text = vec![0u8; size as usize];
                            read_at(src, sub_start + 1, &mut text)?;
                            let shown: String =
                                printable(&text).chars().take(MAX_DETAIL_CHARS).collect();
                            if !shown.is_empty() {
                                finding = finding.with_detail(&shown);
                            }
                        }
                        analysis.add_finding(finding);
                        false
                    }
                    0xFF => match application_id(src, sub_start)? {
                        Some(id) => {
                            if &id[..] == b"NETSCAPE2.0" || &id[..] == b"ANIMEXTS1.0" {
                                analysis.add_kept("Animation loop setting");
                                true
                            } else if &id[..] == b"XMP DataXMP" {
                                analysis.add_finding(Finding::new(Category::Xmp, "XMP metadata"));
                                false
                            } else if &id[..] == b"ICCRGBG1012" {
                                if options.keep_color_profile {
                                    analysis.add_kept("Color profile (ICC)");
                                    true
                                } else {
                                    analysis.add_finding(Finding::new(
                                        Category::Other,
                                        "Color profile (ICC)",
                                    ));
                                    false
                                }
                            } else {
                                let name = printable(&id[..8]);
                                analysis.add_finding(Finding::new(
                                    Category::Other,
                                    &format!("Application data ({name})"),
                                ));
                                false
                            }
                        }
                        None => {
                            analysis.add_finding(Finding::new(Category::Other, "Application data"));
                            false
                        }
                    },
                    _ => {
                        analysis.add_finding(Finding::new(Category::Other, "Unknown extension"));
                        false
                    }
                };
                if keep {
                    analysis.plan.copy(start, end - start);
                }
                pos = end;
            }
            _ => return Err("unusual GIF structure".to_string()),
        }
    }

    if !saw_image {
        return Err(damaged());
    }
    Ok(analysis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::metadata_remover::executor::apply_plan;
    use crate::tools::metadata_remover::model::{TestRng, assert_plan_sane};
    use image::Frame;
    use image::codecs::gif::GifEncoder;
    use std::io::Cursor;

    fn sub_blocks(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for part in data.chunks(255) {
            out.push(part.len() as u8);
            out.extend_from_slice(part);
        }
        out.push(0);
        out
    }

    fn ext(label: u8, blocks: &[&[u8]]) -> Vec<u8> {
        let mut out = vec![0x21, label];
        for block in blocks {
            out.push(block.len() as u8);
            out.extend_from_slice(block);
        }
        out.push(0);
        out
    }

    /// Header, screen descriptor and a global color table of two colors.
    fn header() -> Vec<u8> {
        let mut out = b"GIF89a".to_vec();
        out.extend_from_slice(&[1, 0, 1, 0, 0x80, 0, 0]);
        out.extend_from_slice(&[0, 0, 0, 255, 255, 255]);
        out
    }

    fn frame() -> Vec<u8> {
        let mut out = vec![0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0];
        out.push(2);
        out.extend(sub_blocks(&[0x44, 0x01]));
        out
    }

    fn frame_with_local_table() -> Vec<u8> {
        let mut out = vec![0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0x80];
        out.extend_from_slice(&[10, 20, 30, 40, 50, 60]);
        out.push(2);
        out.extend(sub_blocks(&[0x44, 0x01]));
        out
    }

    fn gce() -> Vec<u8> {
        ext(0xF9, &[&[4, 0, 0, 0]])
    }

    fn netscape() -> Vec<u8> {
        ext(0xFF, &[b"NETSCAPE2.0", &[1, 0, 0]])
    }

    fn build(extras: &[Vec<u8>], frames: &[Vec<u8>], trailing: &[u8]) -> Vec<u8> {
        let mut out = header();
        for part in extras {
            out.extend_from_slice(part);
        }
        for part in frames {
            out.extend_from_slice(part);
        }
        out.push(0x3B);
        out.extend_from_slice(trailing);
        out
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
    fn a_clean_animation_is_left_alone() {
        let data = build(
            &[netscape(), gce()],
            &[frame(), gce(), frame_with_local_table()],
            &[],
        );
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert!(analysis.findings.is_empty());
        assert!(!analysis.has_changes());
        assert_eq!(analysis.kept, vec!["Animation loop setting".to_string()]);
        assert_eq!(out, data);
    }

    #[test]
    fn metadata_is_removed_and_the_rest_is_kept() {
        let comment = ext(0xFE, &[b"hello"]);
        let xmp = ext(0xFF, &[b"XMP DataXMP", b"<x:xmpmeta/>"]);
        let other = ext(0xFF, &[b"SOMEAPP1.00", b"zzz"]);
        let icc = ext(0xFF, &[b"ICCRGBG1012", b"profile"]);
        let text = ext(0x01, &[&[0u8; 12], b"hi"]);
        let data = build(
            &[
                comment,
                xmp,
                other,
                netscape(),
                gce(),
                icc.clone(),
                text.clone(),
            ],
            &[frame()],
            b"TAIL",
        );
        let (analysis, out) = run(&data, &CleanOptions::default());
        let expected = build(&[netscape(), gce(), icc, text], &[frame()], &[]);
        assert_eq!(out, expected);
        assert!(!contains(&out, b"hello"));
        assert!(!contains(&out, b"TAIL"));
        assert_eq!(
            labels(&analysis),
            vec![
                "GIF comment",
                "XMP metadata",
                "Application data (SOMEAPP1)",
                "Extra data after the image",
            ]
        );
        assert_eq!(analysis.findings[0].detail.as_deref(), Some("hello"));
        assert_eq!(analysis.findings[3].detail.as_deref(), Some("4 bytes"));
        assert_eq!(
            analysis.kept,
            vec![
                "Animation loop setting".to_string(),
                "Color profile (ICC)".to_string(),
                "Text drawn on the picture".to_string(),
            ]
        );
    }

    #[test]
    fn the_color_profile_can_be_dropped() {
        let icc = ext(0xFF, &[b"ICCRGBG1012", b"profile"]);
        let data = build(&[icc], &[frame()], &[]);
        let options = CleanOptions {
            keep_orientation: true,
            keep_color_profile: false,
        };
        let (analysis, out) = run(&data, &options);
        assert_eq!(out, build(&[], &[frame()], &[]));
        assert_eq!(labels(&analysis), vec!["Color profile (ICC)"]);
        assert!(analysis.kept.is_empty());
    }

    #[test]
    fn hostile_inputs_are_rejected_not_crashed() {
        let options = CleanOptions::default();
        let check = |bytes: Vec<u8>| {
            let len = bytes.len() as u64;
            analyze(&mut Cursor::new(bytes), len, &options)
        };
        assert!(check(Vec::new()).is_err());
        assert!(check(b"GIF89a".to_vec()).is_err());
        assert!(check(b"hello world, this is no gif at all".to_vec()).is_err());
        assert!(check(header()).is_err());

        let mut no_frames = header();
        no_frames.push(0x3B);
        assert!(check(no_frames).is_err());

        let mut bad_block = header();
        bad_block.extend_from_slice(&[0x05, 0x00]);
        assert!(check(bad_block).is_err());

        let mut long_sub_block = header();
        long_sub_block.extend_from_slice(&[0x21, 0xFE, 200, 1, 2, 3]);
        assert!(check(long_sub_block).is_err());

        let mut cut_descriptor = header();
        cut_descriptor.extend_from_slice(&[0x2C, 0, 0, 0]);
        assert!(check(cut_descriptor).is_err());

        let mut big_table = b"GIF89a".to_vec();
        big_table.extend_from_slice(&[1, 0, 1, 0, 0x87, 0, 0]);
        assert!(check(big_table).is_err());
    }

    #[test]
    fn every_truncation_is_rejected_or_handled() {
        let base = build(&[ext(0xFE, &[b"hello"]), gce()], &[frame()], b"TAIL");
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
        let base = build(
            &[
                ext(0xFE, &[b"hello"]),
                ext(0xFF, &[b"XMP DataXMP", b"<x:xmpmeta/>"]),
                netscape(),
                gce(),
            ],
            &[frame(), frame_with_local_table()],
            b"TAIL",
        );
        let options = CleanOptions::default();
        let mut rng = TestRng::new(0x0DDB_1A5E_5BAD_5EED);
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
        let solid = |color: [u8; 4]| image::ImageBuffer::from_fn(8, 8, |_, _| image::Rgba(color));
        let mut encoded = Cursor::new(Vec::new());
        {
            let mut encoder = GifEncoder::new(&mut encoded);
            encoder
                .encode_frame(Frame::new(solid([255, 0, 0, 255])))
                .unwrap();
            encoder
                .encode_frame(Frame::new(solid([0, 0, 255, 255])))
                .unwrap();
        }
        let base = encoded.into_inner();
        let packed = base[10];
        let table = if packed & 0x80 != 0 {
            3usize << ((packed & 0x07) + 1)
        } else {
            0
        };
        let at = 13 + table;

        let mut data = base[..at].to_vec();
        data.extend(ext(0xFE, &[b"hello"]));
        data.extend_from_slice(&base[at..]);
        data.extend_from_slice(b"TRAILING");

        let (analysis, cleaned) = run(&data, &CleanOptions::default());
        assert!(analysis.has_changes());
        assert!(!contains(&cleaned, b"hello"));
        assert!(!contains(&cleaned, b"TRAILING"));
        assert!(cleaned.len() < data.len());

        let before = image::load_from_memory(&base).unwrap().to_rgba8();
        let after = image::load_from_memory(&cleaned).unwrap().to_rgba8();
        assert_eq!(before.dimensions(), after.dimensions());
        assert_eq!(before.as_raw(), after.as_raw());

        // Cleaning the cleaned file again changes nothing.
        let (again, again_out) = run(&cleaned, &CleanOptions::default());
        assert!(!again.has_changes());
        assert!(again.findings.is_empty());
        assert_eq!(again_out, cleaned);
    }
}
