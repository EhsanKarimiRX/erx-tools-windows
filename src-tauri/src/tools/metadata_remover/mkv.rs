//! Video handler for MKV and WebM (Matroska, an EBML file).
//!
//! Nothing is re-encoded or moved. Metadata is overwritten IN PLACE with
//! "Void" elements of the same total size (every player skips them), so
//! every offset stays valid and the file keeps its size:
//! - Tags (statistics, encoder and other extra information);
//! - attached PICTURES (cover art); attached fonts are kept because styled
//!   subtitles need them; chapters are content and are kept;
//! - the title, the date and the file names stored in the segment info;
//! - the muxing and writing program names, which are required elements, so
//!   their text is overwritten with zeros instead;
//! - a CRC-32 element inside a changed element (the checksum would no
//!   longer match).
//!
//! Only the headers of the elements are read, never the video data. A
//! Segment of unknown size is accepted; an unknown-size element inside a
//! Segment (a live recording that was never finished) is refused.

use std::io::{Read, Seek, SeekFrom};

use super::model::{Analysis, Category, CleanOptions, FileFormat, Finding};
use super::patch::Patches;

const ID_EBML: u32 = 0x1A45_DFA3;
const ID_SEGMENT: u32 = 0x1853_8067;
const ID_INFO: u32 = 0x1549_A966;
const ID_TAGS: u32 = 0x1254_C367;
const ID_TAG: u32 = 0x7373;
const ID_ATTACHMENTS: u32 = 0x1941_A469;
const ID_ATTACHED_FILE: u32 = 0x61A7;
const ID_FILE_NAME: u32 = 0x466E;
const ID_FILE_MEDIA_TYPE: u32 = 0x4660;
const ID_CHAPTERS: u32 = 0x1043_A770;
const ID_TITLE: u32 = 0x7BA9;
const ID_DATE_UTC: u32 = 0x4461;
const ID_MUXING_APP: u32 = 0x4D80;
const ID_WRITING_APP: u32 = 0x5741;
const ID_SEGMENT_FILENAME: u32 = 0x7384;
const ID_PREV_FILENAME: u32 = 0x003C_83AB;
const ID_NEXT_FILENAME: u32 = 0x003E_83BB;
const ID_CRC32: u32 = 0xBF;

/// Total number of elements looked at in one file.
const MAX_ELEMENTS: u32 = 1_000_000;
const MAX_TEXT_READ: u64 = 256;
const MAX_BLANK_CHECK: u64 = 65_536;
const MAX_DETAIL_CHARS: usize = 120;

fn damaged() -> String {
    "damaged or incomplete video file".to_string()
}

fn unusual() -> String {
    "unusual video structure".to_string()
}

fn read_at<R: Read + Seek>(src: &mut R, pos: u64, buf: &mut [u8]) -> Result<(), String> {
    src.seek(SeekFrom::Start(pos)).map_err(|_| damaged())?;
    src.read_exact(buf).map_err(|_| damaged())
}

fn read_vec<R: Read + Seek>(src: &mut R, pos: u64, len: u64) -> Result<Vec<u8>, String> {
    let size = usize::try_from(len).map_err(|_| damaged())?;
    let mut data = vec![0u8; size];
    read_at(src, pos, &mut data)?;
    Ok(data)
}

/// One element: its ID, where it starts, where its data starts and ends.
#[derive(Debug, Clone, Copy)]
struct Elem {
    id: u32,
    start: u64,
    data_start: u64,
    end: u64,
    unknown: bool,
}

impl Elem {
    fn total(&self) -> u64 {
        self.end - self.start
    }

    fn data_len(&self) -> u64 {
        self.end - self.data_start
    }
}

/// Reads the element header at `pos` inside a container that ends at
/// `limit`. None when `pos` is at or past the end.
fn read_elem<R: Read + Seek>(src: &mut R, pos: u64, limit: u64) -> Result<Option<Elem>, String> {
    if pos >= limit {
        return Ok(None);
    }
    let avail = (limit - pos).min(12) as usize;
    let mut buf = [0u8; 12];
    {
        let part = buf.get_mut(..avail).ok_or_else(damaged)?;
        read_at(src, pos, part)?;
    }
    let head = buf.get(..avail).ok_or_else(damaged)?;

    let first = *head.first().ok_or_else(damaged)?;
    if first == 0 {
        return Err(damaged());
    }
    let id_len = first.leading_zeros() as usize + 1;
    if id_len > 4 {
        return Err(damaged());
    }
    let mut id: u32 = 0;
    for index in 0..id_len {
        let byte = *head.get(index).ok_or_else(damaged)?;
        id = (id << 8) | u32::from(byte);
    }

    let size_first = *head.get(id_len).ok_or_else(damaged)?;
    if size_first == 0 {
        return Err(damaged());
    }
    let size_len = size_first.leading_zeros() as usize + 1;
    let mask = (1u64 << (8 - size_len)) - 1;
    let mut value = u64::from(size_first) & mask;
    let mut all_ones = value == mask;
    for index in 1..size_len {
        let byte = *head.get(id_len + index).ok_or_else(damaged)?;
        value = (value << 8) | u64::from(byte);
        if byte != 0xFF {
            all_ones = false;
        }
    }

    let data_start = pos + (id_len + size_len) as u64;
    let end = if all_ones {
        limit
    } else {
        data_start.checked_add(value).ok_or_else(damaged)?
    };
    if end > limit {
        return Err(damaged());
    }
    Ok(Some(Elem {
        id,
        start: pos,
        data_start,
        end,
        unknown: all_ones,
    }))
}

/// The elements between `start` and `end`. An element of unknown size is
/// refused.
fn children<R: Read + Seek>(
    src: &mut R,
    budget: &mut u32,
    start: u64,
    end: u64,
) -> Result<Vec<Elem>, String> {
    let mut out = Vec::new();
    let mut pos = start;
    while let Some(elem) = read_elem(src, pos, end)? {
        *budget += 1;
        if *budget > MAX_ELEMENTS {
            return Err(damaged());
        }
        if elem.unknown {
            return Err(unusual());
        }
        pos = elem.end;
        out.push(elem);
    }
    Ok(out)
}

/// The start of a Void element (ID 0xEC and the size field) that fills
/// `total` bytes together with zero bytes after it. None when `total` is
/// smaller than 2.
pub fn void_head(total: u64) -> Option<Vec<u8>> {
    for length in 1..=8u64 {
        let payload = total.checked_sub(1 + length)?;
        let max = (1u64 << (7 * length)) - 2;
        if payload <= max {
            let marked = payload | (1u64 << (7 * length));
            let bytes = marked.to_be_bytes();
            let mut out = vec![0xEC];
            out.extend_from_slice(bytes.get(8 - length as usize..)?);
            return Some(out);
        }
    }
    None
}

/// (year, month, day) of a day number counted from 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// The Matroska date (nanoseconds since 2001-01-01) as text.
fn matroska_date(nanoseconds: i64) -> String {
    let seconds = nanoseconds.div_euclid(1_000_000_000);
    // 2001-01-01 is day 11323 after 1970-01-01.
    let days = 11_323 + seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

struct Ctx {
    analysis: Analysis,
    patches: Patches,
    budget: u32,
}

impl Ctx {
    fn add(&mut self, category: Category, label: &str, detail: Option<String>) {
        let mut finding = Finding::new(category, label);
        if let Some(text) = detail
            && !text.is_empty()
        {
            finding = finding.with_detail(&text);
        }
        self.analysis.add_finding(finding);
    }

    /// Replaces the whole element by a Void element of the same size.
    fn void_whole(&mut self, elem: &Elem) -> Result<(), String> {
        let head = void_head(elem.total()).ok_or_else(unusual)?;
        self.patches.replace(elem.start, elem.end, head);
        Ok(())
    }

    /// The text of a string element, cleaned and shortened.
    fn text<R: Read + Seek>(&self, src: &mut R, elem: &Elem) -> Result<String, String> {
        let bytes = read_vec(src, elem.data_start, elem.data_len().min(MAX_TEXT_READ))?;
        let text: String = String::from_utf8_lossy(&bytes)
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_DETAIL_CHARS)
            .collect();
        Ok(text.trim().to_string())
    }

    /// True when the data of the element is not just zero bytes.
    fn has_content<R: Read + Seek>(&self, src: &mut R, elem: &Elem) -> Result<bool, String> {
        let bytes = read_vec(src, elem.data_start, elem.data_len().min(MAX_BLANK_CHECK))?;
        Ok(bytes.iter().any(|byte| *byte != 0))
    }

    fn process_info<R: Read + Seek>(&mut self, src: &mut R, info: &Elem) -> Result<(), String> {
        let kids = children(src, &mut self.budget, info.data_start, info.end)?;
        let mut changed = false;
        let mut crc: Option<Elem> = None;
        for kid in &kids {
            match kid.id {
                ID_CRC32 => crc = Some(*kid),
                ID_TITLE => {
                    if kid.data_len() > 0 {
                        let text = self.text(src, kid)?;
                        self.add(Category::Comment, "Title", Some(text));
                        self.void_whole(kid)?;
                        changed = true;
                    }
                }
                ID_DATE_UTC => {
                    let detail = if kid.data_len() == 8 {
                        let raw = read_vec(src, kid.data_start, 8)?;
                        let array: [u8; 8] = raw.as_slice().try_into().map_err(|_| damaged())?;
                        Some(matroska_date(i64::from_be_bytes(array)))
                    } else {
                        None
                    };
                    self.add(Category::Date, "Creation date", detail);
                    self.void_whole(kid)?;
                    changed = true;
                }
                ID_MUXING_APP | ID_WRITING_APP => {
                    if self.has_content(src, kid)? {
                        let label = if kid.id == ID_MUXING_APP {
                            "Muxing program"
                        } else {
                            "Writing program"
                        };
                        let text = self.text(src, kid)?;
                        self.add(Category::Software, label, Some(text));
                        self.patches.zero(kid.data_start, kid.end);
                        changed = true;
                    }
                }
                ID_SEGMENT_FILENAME | ID_PREV_FILENAME | ID_NEXT_FILENAME if kid.data_len() > 0 => {
                    self.add(Category::Other, "File name stored in the video", None);
                    self.void_whole(kid)?;
                    changed = true;
                }
                _ => {}
            }
        }
        if changed && let Some(checksum) = crc {
            self.void_whole(&checksum)?;
        }
        Ok(())
    }

    fn remove_tags<R: Read + Seek>(&mut self, src: &mut R, tags: &Elem) -> Result<(), String> {
        let kids = children(src, &mut self.budget, tags.data_start, tags.end)?;
        let count = kids.iter().filter(|kid| kid.id == ID_TAG).count();
        let detail = if count > 0 {
            Some(format!("{count} tag{}", if count == 1 { "" } else { "s" }))
        } else {
            None
        };
        self.add(Category::Other, "Tags (extra information)", detail);
        self.void_whole(tags)
    }

    fn process_attachments<R: Read + Seek>(
        &mut self,
        src: &mut R,
        attachments: &Elem,
    ) -> Result<(), String> {
        let kids = children(
            src,
            &mut self.budget,
            attachments.data_start,
            attachments.end,
        )?;
        let mut changed = false;
        let mut crc: Option<Elem> = None;
        for file in &kids {
            match file.id {
                ID_CRC32 => crc = Some(*file),
                ID_ATTACHED_FILE => {
                    let parts = children(src, &mut self.budget, file.data_start, file.end)?;
                    let mut media = String::new();
                    let mut name = String::new();
                    for part in &parts {
                        match part.id {
                            ID_FILE_MEDIA_TYPE => media = self.text(src, part)?,
                            ID_FILE_NAME => name = self.text(src, part)?,
                            _ => {}
                        }
                    }
                    if media.to_ascii_lowercase().starts_with("image/") {
                        self.add(Category::Thumbnail, "Attached picture", Some(name));
                        self.void_whole(file)?;
                        changed = true;
                    } else {
                        self.analysis.add_kept("Attached files (for example fonts)");
                    }
                }
                _ => {}
            }
        }
        if changed && let Some(checksum) = crc {
            self.void_whole(&checksum)?;
        }
        Ok(())
    }

    fn process_segment<R: Read + Seek>(
        &mut self,
        src: &mut R,
        segment: &Elem,
    ) -> Result<(), String> {
        let kids = children(src, &mut self.budget, segment.data_start, segment.end)?;
        for kid in &kids {
            match kid.id {
                ID_INFO => self.process_info(src, kid)?,
                ID_TAGS => self.remove_tags(src, kid)?,
                ID_ATTACHMENTS => self.process_attachments(src, kid)?,
                ID_CHAPTERS => self.analysis.add_kept("Chapters"),
                _ => {}
            }
        }
        Ok(())
    }
}

/// Analyzes an MKV or WebM file of `len` bytes.
pub fn analyze<R: Read + Seek>(
    src: &mut R,
    len: u64,
    _options: &CleanOptions,
) -> Result<Analysis, String> {
    let mut ctx = Ctx {
        analysis: Analysis::new(FileFormat::Mkv, len),
        patches: Patches::new(),
        budget: 0,
    };
    let mut segments: Vec<Elem> = Vec::new();
    let mut pos: u64 = 0;
    let mut first = true;
    while let Some(elem) = read_elem(src, pos, len)? {
        ctx.budget += 1;
        if ctx.budget > MAX_ELEMENTS {
            return Err(damaged());
        }
        if first && elem.id != ID_EBML {
            return Err("not a valid MKV file".to_string());
        }
        first = false;
        if elem.unknown && elem.id != ID_SEGMENT {
            return Err(unusual());
        }
        if elem.id == ID_SEGMENT {
            segments.push(elem);
        }
        pos = elem.end;
    }
    if first {
        return Err("not a valid MKV file".to_string());
    }
    if segments.is_empty() {
        return Err(damaged());
    }
    for segment in &segments {
        ctx.process_segment(src, segment)?;
    }
    ctx.analysis.add_kept("Video, audio and subtitle data");
    let plan = ctx.patches.into_plan(len)?;
    let mut analysis = ctx.analysis;
    analysis.plan = plan;
    Ok(analysis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::metadata_remover::executor::apply_plan;
    use crate::tools::metadata_remover::model::{TestRng, assert_plan_sane};
    use std::io::{Cursor, Write};

    const ID_CLUSTER: u32 = 0x1F43_B675;
    const MUXING: &[u8] = b"libmatroska v1.6.4";
    const WRITING: &[u8] = b"mkvmerge v70";

    fn id_bytes(id: u32) -> Vec<u8> {
        let bytes = id.to_be_bytes();
        let skip = bytes.iter().take_while(|byte| **byte == 0).count();
        bytes[skip..].to_vec()
    }

    fn size_vint(n: u64) -> Vec<u8> {
        for length in 1..=8u32 {
            if n <= (1u64 << (7 * length)) - 2 {
                let marked = n | (1u64 << (7 * length));
                return marked.to_be_bytes()[8 - length as usize..].to_vec();
            }
        }
        panic!("size too large");
    }

    fn el(id: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = id_bytes(id);
        out.extend(size_vint(payload.len() as u64));
        out.extend_from_slice(payload);
        out
    }

    fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
        parts.concat()
    }

    fn void_el(total: usize) -> Vec<u8> {
        let mut out = void_head(total as u64).unwrap();
        let rest = total - out.len();
        out.extend(vec![0u8; rest]);
        out
    }

    fn ebml() -> Vec<u8> {
        el(ID_EBML, &el(0x4282, b"matroska"))
    }

    fn mkv(children: &[Vec<u8>]) -> Vec<u8> {
        let mut out = ebml();
        out.extend(el(ID_SEGMENT, &cat(children)));
        out
    }

    fn cluster() -> Vec<u8> {
        el(ID_CLUSTER, &[0xE7, 0x81, 0x00, 0xA3, 0x84, 1, 2, 3, 4])
    }

    fn tracks() -> Vec<u8> {
        el(0x1654_AE6B, &el(0xAE, &el(0xD7, &[1])))
    }

    fn scale() -> Vec<u8> {
        el(0x2A_D7B1, &[0x0F, 0x42, 0x40])
    }

    fn picture() -> Vec<u8> {
        el(
            ID_ATTACHED_FILE,
            &cat(&[
                el(ID_FILE_NAME, b"cover.jpg"),
                el(ID_FILE_MEDIA_TYPE, b"image/jpeg"),
                el(0x465C, b"PICTURE-BYTES"),
            ]),
        )
    }

    fn font() -> Vec<u8> {
        el(
            ID_ATTACHED_FILE,
            &cat(&[
                el(ID_FILE_NAME, b"arial.ttf"),
                el(ID_FILE_MEDIA_TYPE, b"application/x-truetype-font"),
                el(0x465C, b"FONT-BYTES"),
            ]),
        )
    }

    fn tags() -> Vec<u8> {
        el(
            ID_TAGS,
            &cat(&[el(ID_TAG, b"first"), el(ID_TAG, b"second")]),
        )
    }

    fn secret_file() -> Vec<u8> {
        mkv(&[
            el(
                ID_INFO,
                &cat(&[
                    scale(),
                    el(ID_MUXING_APP, MUXING),
                    el(ID_WRITING_APP, WRITING),
                    el(ID_TITLE, b"My secret title"),
                    el(ID_DATE_UTC, &[0u8; 8]),
                    el(0x73A4, &[7u8; 16]),
                ]),
            ),
            tracks(),
            cluster(),
            tags(),
            el(ID_ATTACHMENTS, &cat(&[picture(), font()])),
        ])
    }

    fn cleaned_file() -> Vec<u8> {
        mkv(&[
            el(
                ID_INFO,
                &cat(&[
                    scale(),
                    el(ID_MUXING_APP, &vec![0u8; MUXING.len()]),
                    el(ID_WRITING_APP, &vec![0u8; WRITING.len()]),
                    void_el(el(ID_TITLE, b"My secret title").len()),
                    void_el(el(ID_DATE_UTC, &[0u8; 8]).len()),
                    el(0x73A4, &[7u8; 16]),
                ]),
            ),
            tracks(),
            cluster(),
            void_el(tags().len()),
            el(ID_ATTACHMENTS, &cat(&[void_el(picture().len()), font()])),
        ])
    }

    fn run(data: &[u8]) -> (Analysis, Vec<u8>) {
        let analysis = analyze(
            &mut Cursor::new(data.to_vec()),
            data.len() as u64,
            &CleanOptions::default(),
        )
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
        let data = mkv(&[
            el(
                ID_INFO,
                &cat(&[
                    scale(),
                    el(ID_MUXING_APP, &[0u8; 8]),
                    el(ID_WRITING_APP, &[0u8; 8]),
                ]),
            ),
            tracks(),
            cluster(),
        ]);
        let (analysis, out) = run(&data);
        assert!(analysis.findings.is_empty());
        assert!(!analysis.has_changes());
        assert_eq!(
            analysis.kept,
            vec!["Video, audio and subtitle data".to_string()]
        );
        assert_eq!(out, data);
    }

    #[test]
    fn metadata_is_removed_in_place() {
        let data = secret_file();
        let (analysis, out) = run(&data);
        assert_eq!(out, cleaned_file());
        assert_eq!(out.len(), data.len());
        assert!(!contains(&out, b"My secret"));
        assert!(!contains(&out, b"libmatroska"));
        assert!(!contains(&out, b"mkvmerge"));
        assert!(!contains(&out, b"PICTURE-BYTES"));
        assert!(!contains(&out, b"cover.jpg"));
        assert!(!contains(&out, b"second"));
        assert!(contains(&out, b"FONT-BYTES"));
        assert_eq!(
            analysis.findings,
            vec![
                Finding::new(Category::Software, "Muxing program")
                    .with_detail("libmatroska v1.6.4"),
                Finding::new(Category::Software, "Writing program").with_detail("mkvmerge v70"),
                Finding::new(Category::Comment, "Title").with_detail("My secret title"),
                Finding::new(Category::Date, "Creation date").with_detail("2001-01-01 00:00:00"),
                Finding::new(Category::Other, "Tags (extra information)").with_detail("2 tags"),
                Finding::new(Category::Thumbnail, "Attached picture").with_detail("cover.jpg"),
            ]
        );
        assert_eq!(
            analysis.kept,
            vec![
                "Attached files (for example fonts)".to_string(),
                "Video, audio and subtitle data".to_string()
            ]
        );
    }

    #[test]
    fn a_second_pass_finds_nothing() {
        let (_, out) = run(&secret_file());
        let (again, again_out) = run(&out);
        assert!(again.findings.is_empty(), "{:?}", labels(&again));
        assert!(!again.has_changes());
        assert_eq!(again_out, out);
    }

    #[test]
    fn fonts_and_chapters_are_kept() {
        let data = mkv(&[el(ID_CHAPTERS, &[0xB6, 0x80]), el(ID_ATTACHMENTS, &font())]);
        let (analysis, out) = run(&data);
        assert!(analysis.findings.is_empty());
        assert!(!analysis.has_changes());
        assert_eq!(
            analysis.kept,
            vec![
                "Chapters".to_string(),
                "Attached files (for example fonts)".to_string(),
                "Video, audio and subtitle data".to_string()
            ]
        );
        assert_eq!(out, data);
    }

    #[test]
    fn a_checksum_is_voided_only_when_its_element_changes() {
        let crc = el(0xBF, &[1, 2, 3, 4]);
        let changed = mkv(&[el(ID_INFO, &cat(&[crc.clone(), el(ID_TITLE, b"T")]))]);
        let expected = mkv(&[el(
            ID_INFO,
            &cat(&[void_el(crc.len()), void_el(el(ID_TITLE, b"T").len())]),
        )]);
        let (analysis, out) = run(&changed);
        assert_eq!(out, expected);
        assert_eq!(labels(&analysis), vec!["Title"]);

        let unchanged = mkv(&[el(ID_INFO, &cat(&[crc, scale()]))]);
        let (analysis, out) = run(&unchanged);
        assert!(analysis.findings.is_empty());
        assert_eq!(out, unchanged);
    }

    #[test]
    fn a_segment_of_unknown_size_is_accepted() {
        let mut data = ebml();
        data.extend_from_slice(&[
            0x18, 0x53, 0x80, 0x67, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        ]);
        data.extend(el(ID_INFO, &el(ID_TITLE, b"Secret")));
        data.extend(cluster());
        let (analysis, out) = run(&data);
        assert_eq!(labels(&analysis), vec!["Title"]);
        assert_eq!(out.len(), data.len());
        assert!(!contains(&out, b"Secret"));
    }

    #[test]
    fn an_unfinished_cluster_is_refused() {
        let mut data = ebml();
        data.extend(el(ID_SEGMENT, &[0x1F, 0x43, 0xB6, 0x75, 0xFF, 1, 2, 3]));
        let error = analyze(
            &mut Cursor::new(data.clone()),
            data.len() as u64,
            &CleanOptions::default(),
        )
        .unwrap_err();
        assert_eq!(error, "unusual video structure");
    }

    #[test]
    fn hostile_inputs_are_rejected_not_crashed() {
        let check = |bytes: Vec<u8>| {
            let len = bytes.len() as u64;
            analyze(&mut Cursor::new(bytes), len, &CleanOptions::default())
        };
        assert!(check(Vec::new()).is_err());
        assert!(check(b"hello world".to_vec()).is_err());
        assert!(check(ebml()).is_err());
        assert!(check(el(ID_SEGMENT, &[])).is_err());

        let mut cut = mkv(&[tracks()]);
        cut.pop();
        assert!(check(cut).is_err());

        let mut long_id = ebml();
        long_id.extend(el(ID_SEGMENT, &[0x08, 0x80]));
        assert!(check(long_id).is_err());

        let mut zero_id = ebml();
        zero_id.extend(el(ID_SEGMENT, &[0x00, 0x80]));
        assert!(check(zero_id).is_err());

        let mut too_long_child = ebml();
        too_long_child.extend(el(ID_SEGMENT, &el(ID_INFO, &[0x7B, 0xA9, 0x85, 1, 2])));
        assert!(check(too_long_child).is_err());

        let mut short_size = ebml();
        short_size.extend(el(ID_SEGMENT, &[0xEC, 0x01, 0, 0]));
        assert!(check(short_size).is_err());
    }

    #[test]
    fn every_truncation_is_rejected() {
        let base = secret_file();
        for length in 0..=base.len() {
            let result = analyze(
                &mut Cursor::new(base[..length].to_vec()),
                length as u64,
                &CleanOptions::default(),
            );
            if length == base.len() {
                assert!(result.is_ok(), "the complete file should parse");
            } else {
                assert!(result.is_err(), "prefix {length} should be rejected");
            }
        }
    }

    #[test]
    fn random_damage_never_panics_and_plans_stay_sane() {
        let base = secret_file();
        let mut rng = TestRng::new(0x7F4A_7C15_9E37_79B9);
        for _ in 0..3000 {
            let mut data = base.clone();
            let flips = 1 + (rng.next_u64() % 3) as usize;
            for _ in 0..flips {
                let index = (rng.next_u64() % data.len() as u64) as usize;
                data[index] = (rng.next_u64() & 0xFF) as u8;
            }
            if let Ok(analysis) = analyze(
                &mut Cursor::new(data.clone()),
                data.len() as u64,
                &CleanOptions::default(),
            ) {
                assert_plan_sane(&analysis);
            }
        }
    }

    #[test]
    fn void_heads_have_the_right_size() {
        assert_eq!(void_head(0), None);
        assert_eq!(void_head(1), None);
        assert_eq!(void_head(2), Some(vec![0xEC, 0x80]));
        assert_eq!(void_head(5), Some(vec![0xEC, 0x83]));
        assert_eq!(void_head(128), Some(vec![0xEC, 0xFE]));
        assert_eq!(void_head(129), Some(vec![0xEC, 0x40, 0x7E]));
        assert_eq!(void_head(130), Some(vec![0xEC, 0x40, 0x7F]));
        assert_eq!(void_head(16_385), Some(vec![0xEC, 0x7F, 0xFE]));
        assert_eq!(void_head(16_386), Some(vec![0xEC, 0x20, 0x3F, 0xFE]));
        for total in 2..=40_000u64 {
            let head = void_head(total).unwrap();
            assert_eq!(head[0], 0xEC);
            let first = head[1];
            let length = first.leading_zeros() as usize + 1;
            assert_eq!(head.len(), 1 + length, "total {total}");
            let mut value = u64::from(first) & ((1u64 << (8 - length)) - 1);
            for byte in &head[2..] {
                value = (value << 8) | u64::from(*byte);
            }
            assert_eq!(value, total - head.len() as u64, "total {total}");
            assert_ne!(value, (1u64 << (7 * length)) - 1, "total {total}");
        }
    }

    #[test]
    fn matroska_dates_are_converted() {
        assert_eq!(matroska_date(0), "2001-01-01 00:00:00");
        assert_eq!(
            matroska_date(365 * 86_400 * 1_000_000_000),
            "2002-01-01 00:00:00"
        );
        assert_eq!(matroska_date(18_429 * 1_000_000_000), "2001-01-01 05:07:09");
        assert_eq!(matroska_date(-1_000_000_000), "2000-12-31 23:59:59");
    }

    /// Needs the sample file of the owner. Run by hand:
    /// cargo test live_real_mkv -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_real_mkv_sample_is_cleaned() {
        use std::fs::File;
        use std::io::{BufReader, BufWriter};

        let path = "C:/dev/erxtools-testdata/Mediatest/A.mkv";
        let options = CleanOptions::default();
        let file = File::open(path).expect("sample file");
        let len = file.metadata().unwrap().len();
        let analysis = analyze(&mut BufReader::new(file), len, &options).expect("analysis");
        println!("A.mkv: {} findings", analysis.findings.len());
        for finding in &analysis.findings {
            println!(
                "  {:?} | {} | {:?}",
                finding.category, finding.label, finding.detail
            );
        }
        for note in &analysis.kept {
            println!("  kept: {note}");
        }
        assert!(analysis.has_changes(), "nothing was found");

        let out_path = "C:/dev/erxtools/target/mkv-live-out.mkv";
        {
            let mut source = BufReader::new(File::open(path).unwrap());
            let mut out = BufWriter::new(File::create(out_path).unwrap());
            let written =
                apply_plan(&mut source, &analysis.plan, &mut out, &mut |_, _| true).unwrap();
            out.flush().unwrap();
            assert_eq!(written, len, "the size must not change");
        }
        let again_file = File::open(out_path).unwrap();
        let again_len = again_file.metadata().unwrap().len();
        assert_eq!(again_len, len);
        let again = analyze(&mut BufReader::new(again_file), again_len, &options).unwrap();
        assert!(!again.has_changes(), "a second run still changes things");
        assert!(again.findings.is_empty(), "a second run still finds things");
        std::fs::remove_file(out_path).unwrap();
    }
}
