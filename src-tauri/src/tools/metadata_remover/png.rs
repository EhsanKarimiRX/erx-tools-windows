//! PNG handler: finds metadata chunks and plans their removal WITHOUT
//! touching the image data. Whole chunks are copied or dropped; every kept
//! chunk keeps its own valid CRC. Only a freshly made orientation chunk
//! needs a CRC, computed by the small `crc32` below.
//!
//! Removed: tEXt, zTXt, iTXt (this includes XMP), tIME, eXIf, Content
//! Credentials (caBX), every other unknown ancillary chunk and any data
//! after IEND. Kept: all chunks that affect how the picture is drawn
//! (gamma, chromaticities, sRGB, pixel size, palette, transparency,
//! background, animation chunks, ...) and, by option, the color profile
//! (iCCP) and a minimal orientation chunk.

use std::io::{Read, Seek, SeekFrom};

use super::model::{Analysis, Category, CleanOptions, FileFormat, Finding, MAX_RAW_METADATA};
use super::orientation::{minimal_tiff, read_orientation};

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
/// Even a long animated PNG stays far below this.
const MAX_CHUNKS: u32 = 1_000_000;
/// The PNG specification limits a chunk length to 2^31 - 1.
const MAX_CHUNK_LENGTH: u32 = 0x7FFF_FFFF;
/// How many bytes of a text chunk are read to show its keyword and text.
const MAX_TEXT_READ: u64 = 4096;
/// At most this many text entries are listed one by one.
const MAX_TEXT_FINDINGS: u32 = 20;
const MAX_DETAIL_CHARS: usize = 120;
const XMP_KEYWORD: &[u8] = b"XML:com.adobe.xmp";
const DRAWING_NOTE: &str = "Drawing information (gamma, color, pixel size)";

fn damaged() -> String {
    "damaged or incomplete PNG file".to_string()
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

/// CRC-32 as used by PNG chunks (polynomial 0xEDB88320).
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A complete chunk: length, type, data and CRC.
pub fn make_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(data.len() + 4);
    body.extend_from_slice(kind);
    body.extend_from_slice(data);
    let mut out = Vec::with_capacity(data.len() + 12);
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
    out
}

/// A complete eXIf chunk (38 bytes) that holds only the Orientation tag.
pub fn orientation_chunk(orientation: u16) -> Vec<u8> {
    make_chunk(b"eXIf", &minimal_tiff(orientation))
}

/// Splits at the first zero byte: (before, after the zero byte).
fn split_zero(data: &[u8]) -> Option<(&[u8], &[u8])> {
    let index = data.iter().position(|byte| *byte == 0)?;
    let (before, after) = data.split_at_checked(index)?;
    Some((before, after.get(1..)?))
}

/// Splits the data of an iTXt chunk into (keyword, compressed flag, text).
fn itxt_parts(data: &[u8]) -> Option<(&[u8], bool, &[u8])> {
    let (keyword, rest) = split_zero(data)?;
    let (flag, rest) = rest.split_first()?;
    let (_method, rest) = rest.split_first()?;
    let (_language, rest) = split_zero(rest)?;
    let (_translated, text) = split_zero(rest)?;
    Some((keyword, *flag != 0, text))
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}

/// Printable, trimmed and shortened text for the "found" list.
fn clean_text(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_DETAIL_CHARS)
        .collect();
    cleaned.trim().to_string()
}

fn text_category(keyword: &str) -> Category {
    match keyword.to_ascii_lowercase().as_str() {
        "author" | "artist" | "copyright" => Category::Author,
        "software" => Category::Software,
        "creation time" => Category::Date,
        "title" | "description" | "comment" | "warning" | "disclaimer" | "source" => {
            Category::Comment
        }
        _ => Category::Other,
    }
}

/// Describes one text chunk: the finding, and the raw XMP packet when the
/// chunk holds an uncompressed one.
fn describe_text<R: Read + Seek>(
    src: &mut R,
    kind: &[u8; 4],
    data_start: u64,
    length: u32,
) -> Result<(Finding, Option<Vec<u8>>), String> {
    let want = u64::from(length).min(MAX_TEXT_READ);
    let prefix = read_payload(src, data_start, want)?;
    let keyword_end = prefix
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(prefix.len())
        .min(79);
    let keyword_bytes = prefix.get(..keyword_end).unwrap_or(&[]);
    let keyword = clean_text(&latin1(keyword_bytes));

    if kind == b"iTXt" && keyword_bytes == XMP_KEYWORD {
        let mut xmp = None;
        if u64::from(length) <= MAX_RAW_METADATA as u64 {
            let full = if prefix.len() as u64 == u64::from(length) {
                prefix.clone()
            } else {
                read_payload(src, data_start, u64::from(length))?
            };
            if let Some((_, false, text)) = itxt_parts(&full) {
                xmp = Some(text.to_vec());
            }
        }
        return Ok((Finding::new(Category::Xmp, "XMP metadata"), xmp));
    }

    let (category, detail) = match kind {
        b"tEXt" => {
            let value = split_zero(&prefix).map(|(_, text)| clean_text(&latin1(text)));
            (text_category(&keyword), value)
        }
        b"iTXt" => {
            let value = match itxt_parts(&prefix) {
                Some((_, false, text)) => Some(clean_text(&String::from_utf8_lossy(text))),
                _ => None,
            };
            (text_category(&keyword), value)
        }
        // zTXt: the text is compressed, only the keyword is readable.
        _ => (text_category(&keyword), None),
    };
    let detail = detail.filter(|value| !value.is_empty());
    let label = if keyword.is_empty() {
        "Text entry".to_string()
    } else {
        format!("Text entry: {keyword}")
    };
    Ok((
        Finding {
            category,
            label,
            detail,
        },
        None,
    ))
}

/// Analyzes a PNG file of `len` bytes.
pub fn analyze<R: Read + Seek>(
    src: &mut R,
    len: u64,
    options: &CleanOptions,
) -> Result<Analysis, String> {
    if len < SIGNATURE.len() as u64 {
        return Err(damaged());
    }
    let mut signature = [0u8; 8];
    read_at(src, 0, &mut signature)?;
    if signature != SIGNATURE {
        return Err("not a valid PNG file".to_string());
    }

    let mut analysis = Analysis::new(FileFormat::Png, len);
    // Byte ranges (start, end) of the chunks that stay; the first one is IHDR.
    let mut kept: Vec<(u64, u64)> = Vec::new();
    let mut pending_orientation: Option<u16> = None;
    let mut orientation_seen = false;
    let mut text_findings: u32 = 0;
    let mut more_text: u32 = 0;
    let mut saw_idat = false;
    let mut finished = false;
    let mut pos: u64 = SIGNATURE.len() as u64;
    let mut index: u32 = 0;

    while !finished {
        index += 1;
        if index > MAX_CHUNKS {
            return Err(damaged());
        }
        let mut header = [0u8; 8];
        read_at(src, pos, &mut header)?;
        let [l0, l1, l2, l3, k0, k1, k2, k3] = header;
        let length = u32::from_be_bytes([l0, l1, l2, l3]);
        let kind = [k0, k1, k2, k3];
        if length > MAX_CHUNK_LENGTH || !kind.iter().all(u8::is_ascii_alphabetic) {
            return Err(damaged());
        }
        let end = pos + 12 + u64::from(length);
        if end > len {
            return Err(damaged());
        }
        // The first chunk must be IHDR (13 bytes); no other chunk may be.
        if (index == 1) != (&kind == b"IHDR") || (index == 1 && length != 13) {
            return Err(damaged());
        }
        let data_start = pos + 8;
        let chunk = (pos, end);

        match &kind {
            b"IHDR" | b"PLTE" => kept.push(chunk),
            b"IDAT" => {
                saw_idat = true;
                kept.push(chunk);
            }
            b"IEND" => {
                if !saw_idat {
                    return Err(damaged());
                }
                kept.push(chunk);
                if len > end {
                    analysis.add_finding(
                        Finding::new(Category::ExtraData, "Extra data after the image")
                            .with_detail(&format!("{} bytes", len - end)),
                    );
                }
                finished = true;
            }
            b"gAMA" | b"cHRM" | b"sRGB" | b"pHYs" | b"sBIT" | b"cICP" | b"mDCV" | b"cLLI" => {
                analysis.add_kept(DRAWING_NOTE);
                kept.push(chunk);
            }
            b"tRNS" | b"bKGD" | b"hIST" | b"sPLT" | b"sTER" | b"acTL" | b"fcTL" | b"fdAT" => {
                kept.push(chunk);
            }
            b"iCCP" => {
                if options.keep_color_profile {
                    analysis.add_kept("Color profile (ICC)");
                    kept.push(chunk);
                } else {
                    analysis.add_finding(Finding::new(Category::Other, "Color profile (ICC)"));
                }
            }
            b"tEXt" | b"zTXt" | b"iTXt" => {
                let (finding, xmp) = describe_text(src, &kind, data_start, length)?;
                if analysis.xmp.is_none() {
                    analysis.xmp = xmp;
                }
                if finding.category == Category::Xmp {
                    analysis.add_finding(finding);
                } else if text_findings < MAX_TEXT_FINDINGS {
                    text_findings += 1;
                    analysis.add_finding(finding);
                } else {
                    more_text += 1;
                }
            }
            b"tIME" => {
                let mut finding = Finding::new(Category::Date, "Last-modified time");
                if length == 7 {
                    let mut raw = [0u8; 7];
                    read_at(src, data_start, &mut raw)?;
                    let [y0, y1, month, day, hour, minute, second] = raw;
                    let year = u16::from_be_bytes([y0, y1]);
                    finding = finding.with_detail(&format!(
                        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}"
                    ));
                }
                analysis.add_finding(finding);
            }
            b"eXIf" => {
                let mut tiff: Option<Vec<u8>> = None;
                if u64::from(length) <= MAX_RAW_METADATA as u64 {
                    tiff = Some(read_payload(src, data_start, u64::from(length))?);
                }
                if analysis.exif.is_none() {
                    analysis.exif = tiff.clone();
                }
                let mut handled = false;
                // Only the first eXIf chunk decides about the orientation.
                if options.keep_orientation && !orientation_seen {
                    orientation_seen = true;
                    let value = tiff
                        .as_deref()
                        .and_then(read_orientation)
                        .filter(|value| *value != 1);
                    if let Some(value) = value {
                        analysis.add_kept("Photo rotation (orientation)");
                        if tiff.as_deref() == Some(minimal_tiff(value).as_slice()) {
                            // Already exactly the minimal block: keep it in place.
                            kept.push(chunk);
                            handled = true;
                        } else {
                            pending_orientation = Some(value);
                        }
                    }
                }
                if !handled {
                    analysis.add_finding(Finding::new(Category::Other, "EXIF metadata block"));
                }
            }
            _ => {
                // An unknown chunk whose first letter is a capital is
                // critical: the file cannot be handled safely.
                if k0.is_ascii_uppercase() {
                    return Err("unusual PNG structure".to_string());
                }
                let label = if &kind == b"caBX" {
                    "Content Credentials (C2PA)".to_string()
                } else {
                    format!("Private data chunk ({})", String::from_utf8_lossy(&kind))
                };
                analysis.add_finding(Finding::new(Category::Other, &label));
            }
        }
        pos = end;
    }

    if more_text > 0 {
        analysis.add_finding(
            Finding::new(Category::Other, "More text entries")
                .with_detail(&format!("{more_text} more")),
        );
    }

    // Build the plan: signature, IHDR, the new orientation chunk (if any),
    // then every other kept chunk.
    let mut ranges = kept.into_iter();
    analysis.plan.copy(0, SIGNATURE.len() as u64);
    if let Some((start, end)) = ranges.next() {
        analysis.plan.copy(start, end - start);
    }
    if let Some(value) = pending_orientation {
        analysis.plan.insert(orientation_chunk(value));
    }
    for (start, end) in ranges {
        analysis.plan.copy(start, end - start);
    }
    Ok(analysis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::metadata_remover::executor::apply_plan;
    use crate::tools::metadata_remover::model::{TestRng, assert_plan_sane};
    use image::codecs::png::PngEncoder;
    use image::{ExtendedColorType, ImageEncoder};
    use std::io::Cursor;

    fn ihdr() -> Vec<u8> {
        make_chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0])
    }

    fn idat() -> Vec<u8> {
        make_chunk(b"IDAT", &[1, 2, 3, 4, 5])
    }

    fn iend() -> Vec<u8> {
        make_chunk(b"IEND", &[])
    }

    fn gama() -> Vec<u8> {
        make_chunk(b"gAMA", &[0, 1, 0x86, 0xA0])
    }

    /// Signature, IHDR, `before`, IDAT, `after`, IEND and `trailing` bytes.
    fn build(before: &[Vec<u8>], after: &[Vec<u8>], trailing: &[u8]) -> Vec<u8> {
        let mut out = SIGNATURE.to_vec();
        out.extend(ihdr());
        for part in before {
            out.extend_from_slice(part);
        }
        out.extend(idat());
        for part in after {
            out.extend_from_slice(part);
        }
        out.extend(iend());
        out.extend_from_slice(trailing);
        out
    }

    fn clean() -> Vec<u8> {
        build(&[], &[], &[])
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
    fn a_clean_file_is_left_alone() {
        let data = clean();
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert!(analysis.findings.is_empty());
        assert!(analysis.kept.is_empty());
        assert!(!analysis.has_changes());
        assert_eq!(out, data);
    }

    #[test]
    fn drawing_chunks_are_kept() {
        let phys = make_chunk(b"pHYs", &[0, 0, 0x0B, 0x13, 0, 0, 0x0B, 0x13, 1]);
        let data = build(&[gama(), phys], &[], &[]);
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert!(analysis.findings.is_empty());
        assert!(!analysis.has_changes());
        assert_eq!(analysis.kept, vec![DRAWING_NOTE.to_string()]);
        assert_eq!(out, data);
    }

    #[test]
    fn text_time_and_private_chunks_are_removed() {
        let before = vec![
            make_chunk(b"tEXt", b"Software\0Paint"),
            make_chunk(b"zTXt", b"Comment\0\0\x78\x9C\x01"),
            make_chunk(b"iTXt", b"Author\0\0\0\0\0Ehsan"),
            make_chunk(b"tIME", &[0x07, 0xE7, 5, 14, 8, 30, 0]),
            make_chunk(b"prVW", b"junk"),
            make_chunk(b"caBX", b"c2pa"),
        ];
        let after = vec![make_chunk(b"tEXt", b"Source\0camera")];
        let data = build(&before, &after, b"TAIL");
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert_eq!(out, clean());
        assert_eq!(
            labels(&analysis),
            vec![
                "Text entry: Software",
                "Text entry: Comment",
                "Text entry: Author",
                "Last-modified time",
                "Private data chunk (prVW)",
                "Content Credentials (C2PA)",
                "Text entry: Source",
                "Extra data after the image",
            ]
        );
        assert_eq!(analysis.findings[0].detail.as_deref(), Some("Paint"));
        assert_eq!(analysis.findings[0].category, Category::Software);
        assert_eq!(analysis.findings[1].detail, None);
        assert_eq!(analysis.findings[2].detail.as_deref(), Some("Ehsan"));
        assert_eq!(analysis.findings[2].category, Category::Author);
        assert_eq!(
            analysis.findings[3].detail.as_deref(),
            Some("2023-05-14 08:30:00")
        );
        assert_eq!(analysis.findings[3].category, Category::Date);
        assert_eq!(analysis.findings[6].detail.as_deref(), Some("camera"));
        assert_eq!(analysis.findings[7].detail.as_deref(), Some("4 bytes"));
        assert!(analysis.kept.is_empty());
    }

    #[test]
    fn xmp_is_found_and_stored() {
        let xmp = make_chunk(b"iTXt", b"XML:com.adobe.xmp\0\0\0\0\0<x:xmpmeta/>");
        let data = build(&[xmp], &[], &[]);
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert_eq!(out, clean());
        assert_eq!(labels(&analysis), vec!["XMP metadata"]);
        assert_eq!(analysis.findings[0].category, Category::Xmp);
        assert_eq!(analysis.xmp.as_deref(), Some(&b"<x:xmpmeta/>"[..]));
    }

    #[test]
    fn orientation_is_kept_as_a_fresh_minimal_chunk() {
        let exif = exif_with_junk(6, b"SERIALNUMBER-1234");
        let data = build(&[gama(), make_chunk(b"eXIf", &exif)], &[], &[]);
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert_eq!(out, build(&[orientation_chunk(6), gama()], &[], &[]));
        assert!(!contains(&out, b"SERIALNUMBER"));
        assert_eq!(labels(&analysis), vec!["EXIF metadata block"]);
        assert_eq!(
            analysis.kept,
            vec![
                DRAWING_NOTE.to_string(),
                "Photo rotation (orientation)".to_string()
            ]
        );
        assert_eq!(analysis.exif.as_deref(), Some(exif.as_slice()));
    }

    #[test]
    fn the_orientation_can_be_dropped() {
        let exif = exif_with_junk(6, b"SERIALNUMBER-1234");
        let data = build(&[gama(), make_chunk(b"eXIf", &exif)], &[], &[]);
        let options = CleanOptions {
            keep_orientation: false,
            keep_color_profile: true,
        };
        let (analysis, out) = run(&data, &options);
        assert_eq!(out, build(&[gama()], &[], &[]));
        assert_eq!(labels(&analysis), vec!["EXIF metadata block"]);
        assert_eq!(analysis.kept, vec![DRAWING_NOTE.to_string()]);
    }

    #[test]
    fn orientation_one_needs_no_chunk() {
        let data = build(&[make_chunk(b"eXIf", &minimal_tiff(1))], &[], &[]);
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert_eq!(out, clean());
        assert_eq!(labels(&analysis), vec!["EXIF metadata block"]);
        assert!(analysis.kept.is_empty());
    }

    #[test]
    fn an_already_minimal_chunk_stays_untouched() {
        let data = build(&[orientation_chunk(8)], &[], &[]);
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
    fn the_color_profile_option_works() {
        let icc = make_chunk(b"iCCP", b"profile\0\0ZZZZ");
        let data = build(&[icc], &[], &[]);

        let (kept_analysis, kept_out) = run(&data, &CleanOptions::default());
        assert!(kept_analysis.findings.is_empty());
        assert!(!kept_analysis.has_changes());
        assert_eq!(kept_out, data);
        assert_eq!(kept_analysis.kept, vec!["Color profile (ICC)".to_string()]);

        let options = CleanOptions {
            keep_orientation: true,
            keep_color_profile: false,
        };
        let (dropped, dropped_out) = run(&data, &options);
        assert_eq!(dropped_out, clean());
        assert_eq!(labels(&dropped), vec!["Color profile (ICC)"]);
        assert!(dropped.kept.is_empty());
    }

    #[test]
    fn only_the_first_exif_chunk_decides() {
        let first = make_chunk(b"eXIf", &exif_with_junk(6, b"AAAA"));
        let second = make_chunk(b"eXIf", &exif_with_junk(3, b"BBBB"));
        let data = build(&[first, second], &[], &[]);
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert_eq!(out, build(&[orientation_chunk(6)], &[], &[]));
        assert_eq!(labels(&analysis), vec!["EXIF metadata block"]);
        assert_eq!(
            analysis.kept,
            vec!["Photo rotation (orientation)".to_string()]
        );
    }

    #[test]
    fn many_text_entries_are_summarized() {
        let before: Vec<Vec<u8>> = (0..25)
            .map(|i| make_chunk(b"tEXt", format!("K{i}\0v").as_bytes()))
            .collect();
        let data = build(&before, &[], &[]);
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert_eq!(out, clean());
        assert_eq!(analysis.findings.len(), 21);
        assert_eq!(analysis.findings[0].label, "Text entry: K0");
        assert_eq!(analysis.findings[19].label, "Text entry: K19");
        assert_eq!(analysis.findings[20].label, "More text entries");
        assert_eq!(analysis.findings[20].detail.as_deref(), Some("5 more"));
    }

    #[test]
    fn a_text_chunk_without_separator_is_still_removed() {
        let data = build(&[make_chunk(b"tEXt", b"nonulhere")], &[], &[]);
        let (analysis, out) = run(&data, &CleanOptions::default());
        assert_eq!(out, clean());
        assert_eq!(labels(&analysis), vec!["Text entry: nonulhere"]);
    }

    #[test]
    fn hostile_inputs_are_rejected_not_crashed() {
        let options = CleanOptions::default();
        let check = |bytes: Vec<u8>| {
            let len = bytes.len() as u64;
            analyze(&mut Cursor::new(bytes), len, &options)
        };
        assert!(check(Vec::new()).is_err());
        assert!(check(b"hello world, this is no png".to_vec()).is_err());
        assert!(check(SIGNATURE.to_vec()).is_err());

        let mut huge = SIGNATURE.to_vec();
        huge.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
        huge.extend_from_slice(b"IHDR");
        huge.extend_from_slice(&[0u8; 13]);
        assert!(check(huge).is_err());

        let mut wrong_first = SIGNATURE.to_vec();
        wrong_first.extend(make_chunk(b"IDAT", &[1, 2]));
        wrong_first.extend(iend());
        assert!(check(wrong_first).is_err());

        let mut short_header = SIGNATURE.to_vec();
        short_header.extend(make_chunk(b"IHDR", &[0u8; 5]));
        short_header.extend(idat());
        short_header.extend(iend());
        assert!(check(short_header).is_err());

        let critical = check(build(&[make_chunk(b"ABCD", &[1])], &[], &[])).unwrap_err();
        assert!(critical.contains("unusual"));

        assert!(check(build(&[make_chunk(b"te1t", &[])], &[], &[])).is_err());

        let mut no_idat = SIGNATURE.to_vec();
        no_idat.extend(ihdr());
        no_idat.extend(iend());
        assert!(check(no_idat).is_err());

        let mut no_iend = SIGNATURE.to_vec();
        no_iend.extend(ihdr());
        no_iend.extend(idat());
        assert!(check(no_iend).is_err());

        let mut too_long = SIGNATURE.to_vec();
        too_long.extend(ihdr());
        too_long.extend_from_slice(&[0, 0, 1, 0]);
        too_long.extend_from_slice(b"IDAT");
        too_long.extend_from_slice(&[0u8; 10]);
        assert!(check(too_long).is_err());

        assert!(check(build(&[ihdr()], &[], &[])).is_err());
    }

    #[test]
    fn every_truncation_is_rejected_or_handled() {
        let base = build(
            &[
                make_chunk(b"tEXt", b"Comment\0hello"),
                make_chunk(b"eXIf", &exif_with_junk(6, b"JUNK")),
            ],
            &[],
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
        let base = build(
            &[
                make_chunk(b"tEXt", b"Comment\0hello"),
                make_chunk(b"eXIf", &exif_with_junk(6, b"JUNK")),
                make_chunk(b"tIME", &[0x07, 0xE7, 5, 14, 8, 30, 0]),
            ],
            &[],
            b"TAIL",
        );
        let options = CleanOptions::default();
        let mut rng = TestRng::new(0x9E37_79B9_7F4A_7C15);
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
        PngEncoder::new(&mut encoded)
            .write_image(image.as_raw(), 16, 16, ExtendedColorType::Rgb8)
            .unwrap();
        let base = encoded.into_inner();
        assert_eq!(&base[12..16], b"IHDR");

        // Signature (8) + IHDR chunk (25) = 33 bytes, then our extra chunks.
        let mut data = base[..33].to_vec();
        data.extend(make_chunk(b"tEXt", b"Comment\0hello"));
        data.extend(make_chunk(
            b"eXIf",
            &exif_with_junk(6, b"SERIALNUMBER-1234"),
        ));
        data.extend_from_slice(&base[33..]);
        data.extend_from_slice(b"TRAILING-DATA");

        let (analysis, cleaned) = run(&data, &CleanOptions::default());
        assert!(analysis.has_changes());
        assert!(!contains(&cleaned, b"SERIALNUMBER"));
        assert!(!contains(&cleaned, b"hello"));
        assert!(!contains(&cleaned, b"TRAILING"));
        assert!(cleaned.len() < data.len());

        let before = image::load_from_memory(&base).unwrap().to_rgb8();
        let after = image::load_from_memory(&cleaned).unwrap().to_rgb8();
        assert_eq!(before.as_raw(), after.as_raw());

        // Cleaning the cleaned file again changes nothing.
        let (again, again_out) = run(&cleaned, &CleanOptions::default());
        assert!(!again.has_changes());
        assert!(again.findings.is_empty());
        assert_eq!(again_out, cleaned);
    }

    #[test]
    fn crc32_matches_the_known_values() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(
            make_chunk(b"IEND", &[]),
            vec![0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82]
        );
    }

    #[test]
    fn the_new_orientation_chunk_is_well_formed() {
        let chunk = orientation_chunk(6);
        assert_eq!(chunk.len(), 38);
        assert_eq!(&chunk[0..4], &[0, 0, 0, 26]);
        assert_eq!(&chunk[4..8], b"eXIf");
        assert_eq!(
            u32::from_be_bytes([chunk[34], chunk[35], chunk[36], chunk[37]]),
            crc32(&chunk[4..34])
        );
        assert_eq!(read_orientation(&chunk[8..34]), Some(6));
    }
}
