//! TIFF handler (classic TIFF with ONE picture, either byte order).
//!
//! Nothing is re-encoded or moved. Hidden data is removed IN PLACE:
//!
//! - the metadata entries are taken out of the directory (IFD) of the
//!   picture; the directory is rewritten with the entries that stay and the
//!   freed space at its end is overwritten with zeros;
//! - the data these entries pointed to, and the whole Exif and GPS
//!   directories with everything inside them, are overwritten with zeros.
//!
//! The file keeps its size and every offset stays valid, so the picture data
//! is never touched. Removed: document and page names, description, camera
//! make and model, software, date, artist, copyright, host computer, XMP,
//! IPTC, Photoshop data, Exif and GPS (and, by option, the color profile).
//! Kept: everything that describes the picture, the rotation and all
//! unknown or georeferencing tags. Files with several pages, RAW files
//! (DNG) and files with sub-pictures are refused.

use std::io::{Read, Seek, SeekFrom};

use super::found::exif_date;
use super::model::{Analysis, Category, CleanOptions, FileFormat, Finding};
use super::patch::Patches;

const MAX_ENTRIES: usize = 512;
const MAX_NUMBERS: u64 = 200_000;
const MAX_DEPTH: u32 = 3;
const MAX_TEXT_READ: u64 = 256;
const MAX_DETAIL_CHARS: usize = 120;

const TAG_ORIENTATION: u16 = 274;
const TAG_SUB_IFDS: u16 = 330;
const TAG_EXIF_IFD: u16 = 34665;
const TAG_ICC: u16 = 34675;
const TAG_GPS_IFD: u16 = 34853;
const TAG_INTEROP_IFD: u16 = 40965;
const TAG_DNG_VERSION: u16 = 50706;

fn damaged() -> String {
    "damaged or incomplete TIFF file".to_string()
}

fn unusual() -> String {
    "unusual TIFF structure".to_string()
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

#[derive(Clone, Copy)]
struct Order {
    little: bool,
}

impl Order {
    fn u16(self, bytes: [u8; 2]) -> u16 {
        if self.little {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        }
    }

    fn u32(self, bytes: [u8; 4]) -> u32 {
        if self.little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        }
    }

    fn put_u16(self, value: u16) -> [u8; 2] {
        if self.little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        }
    }

    fn put_u32(self, value: u32) -> [u8; 4] {
        if self.little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        }
    }
}

/// Size in bytes of one value of a TIFF data type (0 = unknown type).
fn type_size(kind: u16) -> u64 {
    match kind {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 | 13 => 4,
        5 | 10 | 12 | 16..=18 => 8,
        _ => 0,
    }
}

/// One directory entry: its 12 raw bytes, its fields, and where its data
/// is when it does not fit into the 4 bytes of the entry (start, end).
#[derive(Clone, Copy)]
struct Entry {
    raw: [u8; 12],
    tag: u16,
    kind: u16,
    count: u32,
    field: [u8; 4],
    outside: Option<(u64, u64)>,
}

fn parse_entry(order: Order, raw: [u8; 12], len: u64) -> Result<Entry, String> {
    let [t0, t1, k0, k1, c0, c1, c2, c3, f0, f1, f2, f3] = raw;
    let tag = order.u16([t0, t1]);
    let kind = order.u16([k0, k1]);
    let count = order.u32([c0, c1, c2, c3]);
    let field = [f0, f1, f2, f3];
    let bytes = type_size(kind) * u64::from(count);
    let outside = if bytes > 4 {
        let start = u64::from(order.u32(field));
        let end = start + bytes;
        if start < 8 || end > len {
            return Err(damaged());
        }
        Some((start, end))
    } else {
        None
    };
    Ok(Entry {
        raw,
        tag,
        kind,
        count,
        field,
        outside,
    })
}

/// One directory: where it starts and ends in the file, its entries and the
/// pointer to the next directory.
struct Ifd {
    start: u64,
    end: u64,
    entries: Vec<Entry>,
    next: u32,
}

fn read_ifd<R: Read + Seek>(src: &mut R, order: Order, pos: u64, len: u64) -> Result<Ifd, String> {
    if pos < 8 || pos.checked_add(2).is_none_or(|end| end > len) {
        return Err(damaged());
    }
    let mut count_bytes = [0u8; 2];
    read_at(src, pos, &mut count_bytes)?;
    let count = usize::from(order.u16(count_bytes));
    if count == 0 || count > MAX_ENTRIES {
        return Err(unusual());
    }
    let table_len = count * 12;
    let end = pos + 2 + table_len as u64 + 4;
    if end > len {
        return Err(damaged());
    }
    let raw = read_vec(src, pos + 2, table_len as u64 + 4)?;
    let table = raw.get(..table_len).ok_or_else(damaged)?;
    let tail = raw.get(table_len..table_len + 4).ok_or_else(damaged)?;
    let next = order.u32(<[u8; 4]>::try_from(tail).map_err(|_| damaged())?);
    let mut entries = Vec::with_capacity(count);
    for chunk in table.as_chunks::<12>().0 {
        let raw_entry = *chunk;
        entries.push(parse_entry(order, raw_entry, len)?);
    }
    Ok(Ifd {
        start: pos,
        end,
        entries,
        next,
    })
}

/// The values of a SHORT or LONG entry.
fn read_numbers<R: Read + Seek>(
    src: &mut R,
    order: Order,
    entry: &Entry,
) -> Result<Vec<u64>, String> {
    if entry.kind != 3 && entry.kind != 4 {
        return Err(unusual());
    }
    if u64::from(entry.count) > MAX_NUMBERS {
        return Err(unusual());
    }
    let size = type_size(entry.kind);
    let bytes = size * u64::from(entry.count);
    let data = match entry.outside {
        Some((start, _)) => read_vec(src, start, bytes)?,
        None => {
            let wanted = usize::try_from(bytes).map_err(|_| unusual())?;
            entry.field.get(..wanted).ok_or_else(unusual)?.to_vec()
        }
    };
    let mut out = Vec::with_capacity(entry.count as usize);
    if size == 2 {
        for pair in data.as_chunks::<2>().0 {
            let value = order.u16(*pair);
            out.push(u64::from(value));
        }
    } else {
        for quad in data.as_chunks::<4>().0 {
            let value = order.u32(*quad);
            out.push(u64::from(value));
        }
    }
    Ok(out)
}

/// The byte ranges of the picture data (strips or tiles). A file without
/// any is refused.
fn data_ranges<R: Read + Seek>(
    src: &mut R,
    order: Order,
    entries: &[Entry],
    len: u64,
) -> Result<Vec<(u64, u64)>, String> {
    let find = |tag: u16| entries.iter().find(|entry| entry.tag == tag);
    let mut out: Vec<(u64, u64)> = Vec::new();
    let mut found = false;
    for (offsets_tag, counts_tag) in [(273u16, 279u16), (324, 325)] {
        let (Some(offsets), Some(counts)) = (find(offsets_tag), find(counts_tag)) else {
            continue;
        };
        let starts = read_numbers(src, order, offsets)?;
        let lengths = read_numbers(src, order, counts)?;
        if starts.len() != lengths.len() {
            return Err(unusual());
        }
        for (start, length) in starts.iter().zip(lengths.iter()) {
            let end = start.checked_add(*length).ok_or_else(unusual)?;
            if end > len {
                return Err(damaged());
            }
            if *length > 0 {
                out.push((*start, end));
            }
        }
        found = true;
    }
    if let (Some(offset), Some(length)) = (find(513), find(514)) {
        let starts = read_numbers(src, order, offset)?;
        let lengths = read_numbers(src, order, length)?;
        if let (Some(start), Some(size)) = (starts.first(), lengths.first()) {
            let end = start.checked_add(*size).ok_or_else(unusual)?;
            if end > len {
                return Err(damaged());
            }
            if *size > 0 {
                out.push((*start, end));
            }
            found = true;
        }
    }
    if found { Ok(out) } else { Err(unusual()) }
}

/// Collects the byte ranges of a whole sub-directory (Exif, GPS, Interop):
/// the directory itself and the data of every entry in it.
fn collect_subifd<R: Read + Seek>(
    src: &mut R,
    order: Order,
    pointer: u64,
    len: u64,
    depth: u32,
    out: &mut Vec<(u64, u64)>,
) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(unusual());
    }
    let ifd = read_ifd(src, order, pointer, len)?;
    out.push((ifd.start, ifd.end));
    for entry in &ifd.entries {
        if let Some(range) = entry.outside {
            out.push(range);
        }
        if entry.tag == TAG_INTEROP_IFD && entry.kind == 4 && entry.count == 1 {
            let inner = u64::from(order.u32(entry.field));
            collect_subifd(src, order, inner, len, depth + 1, out)?;
        }
    }
    Ok(())
}

/// The text of an ASCII entry, cleaned and shortened. None when it is not
/// text or empty.
fn entry_text<R: Read + Seek>(src: &mut R, entry: &Entry) -> Option<String> {
    if entry.kind != 2 {
        return None;
    }
    let want = u64::from(entry.count).min(MAX_TEXT_READ);
    let bytes = match entry.outside {
        Some((start, _)) => read_vec(src, start, want).ok()?,
        None => entry.field.get(..usize::try_from(want).ok()?)?.to_vec(),
    };
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    let text: String = String::from_utf8_lossy(bytes.get(..end)?)
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_DETAIL_CHARS)
        .collect();
    let text = text.trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn camera_text(make: Option<&str>, model: Option<&str>) -> Option<String> {
    match (make, model) {
        (Some(make), Some(model)) => {
            if model.starts_with(make) {
                Some(model.to_string())
            } else {
                Some(format!("{make} {model}"))
            }
        }
        (Some(one), None) | (None, Some(one)) => Some(one.to_string()),
        (None, None) => None,
    }
}

fn add_text(analysis: &mut Analysis, category: Category, label: &str, text: Option<String>) {
    let mut finding = Finding::new(category, label);
    if let Some(value) = text {
        finding = finding.with_detail(&value);
    }
    analysis.add_finding(finding);
}

/// Analyzes a TIFF file of `len` bytes.
pub fn analyze<R: Read + Seek>(
    src: &mut R,
    len: u64,
    options: &CleanOptions,
) -> Result<Analysis, String> {
    if len < 8 {
        return Err(damaged());
    }
    let mut head = [0u8; 8];
    read_at(src, 0, &mut head)?;
    let [b0, b1, m0, m1, i0, i1, i2, i3] = head;
    let little = match [b0, b1] {
        [b'I', b'I'] => true,
        [b'M', b'M'] => false,
        _ => return Err("not a valid TIFF file".to_string()),
    };
    let order = Order { little };
    if order.u16([m0, m1]) != 42 {
        return Err("not a valid TIFF file".to_string());
    }
    let first = u64::from(order.u32([i0, i1, i2, i3]));
    let ifd = read_ifd(src, order, first, len)?;
    if ifd.next != 0 {
        return Err("multi-page TIFF files are not supported".to_string());
    }
    if ifd.entries.iter().any(|entry| entry.tag == TAG_DNG_VERSION) {
        return Err("RAW and DNG files are not supported".to_string());
    }
    if ifd.entries.iter().any(|entry| entry.tag == TAG_SUB_IFDS) {
        return Err(unusual());
    }

    let mut protected = data_ranges(src, order, &ifd.entries, len)?;
    protected.push((0, 8));
    let make = ifd
        .entries
        .iter()
        .find(|entry| entry.tag == 271)
        .and_then(|entry| entry_text(src, entry));
    let model = ifd
        .entries
        .iter()
        .find(|entry| entry.tag == 272)
        .and_then(|entry| entry_text(src, entry));
    let mut camera_done = false;

    let mut analysis = Analysis::new(FileFormat::Tiff, len);
    let mut patches = Patches::new();
    let mut zero: Vec<(u64, u64)> = Vec::new();
    let mut kept_raw: Vec<[u8; 12]> = Vec::new();
    let mut removed_any = false;

    for entry in &ifd.entries {
        let remove = match entry.tag {
            269 => {
                let text = entry_text(src, entry);
                add_text(&mut analysis, Category::Comment, "Document name", text);
                true
            }
            270 => {
                let text = entry_text(src, entry);
                add_text(&mut analysis, Category::Comment, "Image description", text);
                true
            }
            271 | 272 => {
                if !camera_done {
                    camera_done = true;
                    let text = camera_text(make.as_deref(), model.as_deref());
                    add_text(
                        &mut analysis,
                        Category::Camera,
                        "Camera make and model",
                        text,
                    );
                }
                true
            }
            285 => {
                let text = entry_text(src, entry);
                add_text(&mut analysis, Category::Comment, "Page name", text);
                true
            }
            305 => {
                let text = entry_text(src, entry);
                add_text(&mut analysis, Category::Software, "Software", text);
                true
            }
            306 => {
                let text = entry_text(src, entry).map(|value| exif_date(&value));
                add_text(&mut analysis, Category::Date, "Date modified", text);
                true
            }
            315 => {
                let text = entry_text(src, entry);
                add_text(&mut analysis, Category::Author, "Artist", text);
                true
            }
            316 => {
                let text = entry_text(src, entry);
                add_text(&mut analysis, Category::Other, "Host computer", text);
                true
            }
            700 => {
                analysis.add_finding(Finding::new(Category::Xmp, "XMP metadata"));
                true
            }
            33432 => {
                let text = entry_text(src, entry);
                add_text(&mut analysis, Category::Author, "Copyright", text);
                true
            }
            33723 => {
                analysis.add_finding(Finding::new(Category::Iptc, "IPTC data"));
                true
            }
            34377 => {
                analysis.add_finding(Finding::new(Category::Iptc, "Photoshop data"));
                true
            }
            TAG_EXIF_IFD | TAG_GPS_IFD => {
                if entry.kind != 4 || entry.count != 1 {
                    return Err(unusual());
                }
                if entry.tag == TAG_EXIF_IFD {
                    analysis.add_finding(Finding::new(Category::Other, "EXIF metadata block"));
                } else {
                    analysis.add_finding(Finding::new(Category::Location, "GPS data"));
                }
                let pointer = u64::from(order.u32(entry.field));
                collect_subifd(src, order, pointer, len, 1, &mut zero)?;
                true
            }
            TAG_ICC => {
                if options.keep_color_profile {
                    analysis.add_kept("Color profile (ICC)");
                    false
                } else {
                    analysis.add_finding(Finding::new(Category::Other, "Color profile (ICC)"));
                    true
                }
            }
            _ => false,
        };
        if remove {
            removed_any = true;
            if let Some(range) = entry.outside {
                zero.push(range);
            }
        } else {
            kept_raw.push(entry.raw);
            if let Some(range) = entry.outside {
                protected.push(range);
            }
            if entry.tag == TAG_ORIENTATION {
                analysis.add_kept("Photo rotation (orientation)");
            }
        }
    }

    if removed_any {
        let mut ranges = zero.clone();
        ranges.sort_unstable();
        for pair in ranges.windows(2) {
            if let [first_range, second_range] = pair
                && first_range.1 > second_range.0
            {
                return Err(unusual());
            }
        }
        for (zero_start, zero_end) in &zero {
            for (keep_start, keep_end) in &protected {
                if zero_start < keep_end && keep_start < zero_end {
                    return Err(unusual());
                }
            }
        }
        let count = u16::try_from(kept_raw.len()).map_err(|_| unusual())?;
        let mut new_ifd = Vec::with_capacity(2 + kept_raw.len() * 12 + 4);
        new_ifd.extend_from_slice(&order.put_u16(count));
        for raw in &kept_raw {
            new_ifd.extend_from_slice(raw);
        }
        new_ifd.extend_from_slice(&order.put_u32(ifd.next));
        patches.replace(ifd.start, ifd.end, new_ifd);
        for (start, end) in &zero {
            patches.zero(*start, *end);
        }
    }
    analysis.add_kept("Picture data");
    analysis.plan = patches.into_plan(len)?;
    Ok(analysis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::metadata_remover::executor::apply_plan;
    use crate::tools::metadata_remover::model::{TestRng, assert_plan_sane};
    use std::io::Cursor;
    use std::path::PathBuf;

    const BASE_TAGS: [u16; 9] = [256, 257, 258, 259, 262, 273, 277, 278, 279];

    #[derive(Clone)]
    struct E {
        tag: u16,
        kind: u16,
        count: u32,
        field: [u8; 4],
    }

    /// Builds a TIFF: the header, 16 picture bytes at offset 8, the data of
    /// the entries and the directories after them.
    struct B {
        data: Vec<u8>,
        little: bool,
    }

    impl B {
        fn new(little: bool) -> B {
            let mut data = if little {
                b"II".to_vec()
            } else {
                b"MM".to_vec()
            };
            let magic: [u8; 2] = if little {
                42u16.to_le_bytes()
            } else {
                42u16.to_be_bytes()
            };
            data.extend_from_slice(&magic);
            data.extend_from_slice(&[0, 0, 0, 0]);
            data.extend((0..16u8).map(|i| i * 10 + 1));
            B { data, little }
        }

        fn u16b(&self, value: u16) -> [u8; 2] {
            if self.little {
                value.to_le_bytes()
            } else {
                value.to_be_bytes()
            }
        }

        fn u32b(&self, value: u32) -> [u8; 4] {
            if self.little {
                value.to_le_bytes()
            } else {
                value.to_be_bytes()
            }
        }

        fn align(&mut self) {
            if self.data.len() % 2 == 1 {
                self.data.push(0);
            }
        }

        fn put(&mut self, bytes: &[u8]) -> u32 {
            self.align();
            let at = self.data.len() as u32;
            self.data.extend_from_slice(bytes);
            at
        }

        fn short(&self, tag: u16, value: u16) -> E {
            let [a, b] = self.u16b(value);
            E {
                tag,
                kind: 3,
                count: 1,
                field: [a, b, 0, 0],
            }
        }

        fn long(&self, tag: u16, value: u32) -> E {
            E {
                tag,
                kind: 4,
                count: 1,
                field: self.u32b(value),
            }
        }

        fn blob(&mut self, tag: u16, kind: u16, count: u32, bytes: &[u8]) -> E {
            if bytes.len() <= 4 {
                let mut field = [0u8; 4];
                field[..bytes.len()].copy_from_slice(bytes);
                E {
                    tag,
                    kind,
                    count,
                    field,
                }
            } else {
                let at = self.put(bytes);
                E {
                    tag,
                    kind,
                    count,
                    field: self.u32b(at),
                }
            }
        }

        fn ascii(&mut self, tag: u16, text: &str) -> E {
            let mut bytes = text.as_bytes().to_vec();
            bytes.push(0);
            let count = bytes.len() as u32;
            self.blob(tag, 2, count, &bytes)
        }

        /// Writes a directory (entries sorted by tag) and returns its offset.
        fn ifd(&mut self, mut entries: Vec<E>, next: u32) -> u32 {
            entries.sort_by_key(|entry| entry.tag);
            self.align();
            let at = self.data.len() as u32;
            let mut raw: Vec<u8> = Vec::new();
            raw.extend_from_slice(&self.u16b(entries.len() as u16));
            for entry in &entries {
                raw.extend_from_slice(&self.u16b(entry.tag));
                raw.extend_from_slice(&self.u16b(entry.kind));
                raw.extend_from_slice(&self.u32b(entry.count));
                raw.extend_from_slice(&entry.field);
            }
            raw.extend_from_slice(&self.u32b(next));
            self.data.extend_from_slice(&raw);
            at
        }

        fn finish(mut self, first: u32) -> Vec<u8> {
            let bytes = self.u32b(first);
            self.data[4..8].copy_from_slice(&bytes);
            self.data
        }
    }

    /// The entries every test file has: a 4 x 4 gray picture in one strip.
    fn base(b: &B) -> Vec<E> {
        vec![
            b.short(256, 4),
            b.short(257, 4),
            b.short(258, 8),
            b.short(259, 1),
            b.short(262, 1),
            b.long(273, 8),
            b.short(277, 1),
            b.short(278, 4),
            b.long(279, 16),
        ]
    }

    fn plain(little: bool) -> Vec<u8> {
        let mut b = B::new(little);
        let entries = base(&b);
        let at = b.ifd(entries, 0);
        b.finish(at)
    }

    fn expected_pixels() -> Vec<u8> {
        (0..16u8).map(|i| i * 10 + 1).collect()
    }

    fn rationals() -> Vec<u8> {
        [47u32, 1, 30, 1, 0, 1]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }

    const ICC: &[u8] = b"ICC-PROFILE-BYTES-20";

    /// Every kind of metadata, an Exif directory and a GPS directory.
    fn secret_file() -> Vec<u8> {
        let mut b = B::new(true);
        let mut e = base(&b);
        e.push(b.ascii(269, "SECRET-DOC-NAME"));
        e.push(b.ascii(271, "Canon"));
        e.push(b.ascii(272, "EOS 5D"));
        e.push(b.short(274, 6));
        e.push(b.ascii(305, "Paint"));
        e.push(b.ascii(306, "2020:01:02 03:04:05"));
        e.push(b.ascii(315, "Ehsan"));
        e.push(b.blob(700, 1, 12, b"<x:xmpmeta/>"));
        e.push(b.ascii(33432, "(c) Me"));
        e.push(b.blob(33723, 7, 6, b"IPTC-1"));
        e.push(b.blob(34377, 1, 8, b"PHOTOSHP"));
        e.push(b.blob(34675, 7, ICC.len() as u32, ICC));
        let taken = b.ascii(36867, "2023:05:14 08:30:00");
        let maker = b.blob(37500, 7, 12, b"SECRET-MAKER");
        let exif_ifd = b.ifd(vec![taken, maker], 0);
        e.push(b.long(34665, exif_ifd));
        let gps_ref = b.blob(1, 2, 2, b"N\0");
        let latitude = b.blob(2, 5, 3, &rationals());
        let gps_ifd = b.ifd(vec![gps_ref, latitude], 0);
        e.push(b.long(34853, gps_ifd));
        let at = b.ifd(e, 0);
        b.finish(at)
    }

    fn light_file() -> Vec<u8> {
        let mut b = B::new(true);
        let mut e = base(&b);
        e.push(b.ascii(269, "SECRET-DOC-NAME"));
        e.push(b.ascii(305, "Paint"));
        let taken = b.ascii(36867, "2023:05:14 08:30:00");
        let maker = b.blob(37500, 7, 12, b"SECRET-MAKER");
        let exif_ifd = b.ifd(vec![taken, maker], 0);
        e.push(b.long(34665, exif_ifd));
        let at = b.ifd(e, 0);
        b.finish(at)
    }

    /// The tags of the first directory, in file order.
    fn tags_of(data: &[u8]) -> Vec<u16> {
        let little = data[0] == b'I';
        let rd16 = |at: usize| -> u16 {
            let bytes = [data[at], data[at + 1]];
            if little {
                u16::from_le_bytes(bytes)
            } else {
                u16::from_be_bytes(bytes)
            }
        };
        let rd32 = |at: usize| -> u32 {
            let bytes = [data[at], data[at + 1], data[at + 2], data[at + 3]];
            if little {
                u32::from_le_bytes(bytes)
            } else {
                u32::from_be_bytes(bytes)
            }
        };
        let ifd = rd32(4) as usize;
        let count = rd16(ifd) as usize;
        (0..count).map(|i| rd16(ifd + 2 + i * 12)).collect()
    }

    fn run_with(data: &[u8], options: &CleanOptions) -> (Analysis, Vec<u8>) {
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

    fn run(data: &[u8]) -> (Analysis, Vec<u8>) {
        run_with(data, &CleanOptions::default())
    }

    fn analyze_bytes(bytes: Vec<u8>) -> Result<Analysis, String> {
        let len = bytes.len() as u64;
        analyze(&mut Cursor::new(bytes), len, &CleanOptions::default())
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
        for little in [true, false] {
            let data = plain(little);
            assert_eq!(tags_of(&data), BASE_TAGS.to_vec());
            let (analysis, out) = run(&data);
            assert!(analysis.findings.is_empty());
            assert!(!analysis.has_changes());
            assert_eq!(analysis.kept, vec!["Picture data".to_string()]);
            assert_eq!(out, data);
        }
    }

    #[test]
    fn metadata_is_removed_in_place() {
        let data = secret_file();
        let (analysis, out) = run(&data);
        assert_eq!(out.len(), data.len());
        assert_eq!(&out[8..24], &data[8..24]);
        assert_eq!(
            tags_of(&out),
            vec![256, 257, 258, 259, 262, 273, 274, 277, 278, 279, 34675]
        );
        for secret in [
            &b"SECRET"[..],
            b"Canon",
            b"EOS 5D",
            b"Paint",
            b"Ehsan",
            b"(c) Me",
            b"IPTC-1",
            b"PHOTOSHP",
            b"xmpmeta",
            b"2023:05:14",
            b"2020:01:02",
        ] {
            assert!(!contains(&out, secret), "{:?} is still there", secret);
        }
        assert!(!contains(&out, &rationals()));
        assert!(contains(&out, ICC));
        assert_eq!(
            analysis.findings,
            vec![
                Finding::new(Category::Comment, "Document name").with_detail("SECRET-DOC-NAME"),
                Finding::new(Category::Camera, "Camera make and model").with_detail("Canon EOS 5D"),
                Finding::new(Category::Software, "Software").with_detail("Paint"),
                Finding::new(Category::Date, "Date modified").with_detail("2020-01-02 03:04:05"),
                Finding::new(Category::Author, "Artist").with_detail("Ehsan"),
                Finding::new(Category::Xmp, "XMP metadata"),
                Finding::new(Category::Author, "Copyright").with_detail("(c) Me"),
                Finding::new(Category::Iptc, "IPTC data"),
                Finding::new(Category::Iptc, "Photoshop data"),
                Finding::new(Category::Other, "EXIF metadata block"),
                Finding::new(Category::Location, "GPS data"),
            ]
        );
        assert_eq!(
            analysis.kept,
            vec![
                "Photo rotation (orientation)".to_string(),
                "Color profile (ICC)".to_string(),
                "Picture data".to_string(),
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
    fn the_color_profile_option_works() {
        let mut b = B::new(true);
        let mut e = base(&b);
        e.push(b.blob(34675, 7, ICC.len() as u32, ICC));
        let at = b.ifd(e, 0);
        let data = b.finish(at);

        let (kept, kept_out) = run(&data);
        assert!(kept.findings.is_empty());
        assert!(!kept.has_changes());
        assert_eq!(
            kept.kept,
            vec![
                "Color profile (ICC)".to_string(),
                "Picture data".to_string()
            ]
        );
        assert_eq!(kept_out, data);

        let options = CleanOptions {
            keep_orientation: true,
            keep_color_profile: false,
        };
        let (dropped, out) = run_with(&data, &options);
        assert_eq!(labels(&dropped), vec!["Color profile (ICC)"]);
        assert_eq!(dropped.kept, vec!["Picture data".to_string()]);
        assert_eq!(out.len(), data.len());
        assert_eq!(tags_of(&out), BASE_TAGS.to_vec());
        assert!(!contains(&out, ICC));
    }

    #[test]
    fn big_endian_files_work() {
        let mut b = B::new(false);
        let mut e = base(&b);
        e.push(b.ascii(269, "SECRET-BE-DOC"));
        e.push(b.short(274, 8));
        let at = b.ifd(e, 0);
        let data = b.finish(at);
        let (analysis, out) = run(&data);
        assert_eq!(out.len(), data.len());
        assert_eq!(&out[8..24], &data[8..24]);
        assert_eq!(
            tags_of(&out),
            vec![256, 257, 258, 259, 262, 273, 274, 277, 278, 279]
        );
        assert!(!contains(&out, b"SECRET"));
        assert_eq!(labels(&analysis), vec!["Document name"]);
        assert_eq!(
            analysis.kept,
            vec![
                "Photo rotation (orientation)".to_string(),
                "Picture data".to_string()
            ]
        );
        let (again, again_out) = run(&out);
        assert!(!again.has_changes());
        assert_eq!(again_out, out);
    }

    #[test]
    fn the_pixels_stay_the_same() {
        let data = light_file();
        let before = image::load_from_memory(&data).expect("the original must decode");
        let (analysis, out) = run(&data);
        assert!(analysis.has_changes());
        assert!(!contains(&out, b"SECRET"));
        let after = image::load_from_memory(&out).expect("the cleaned file must decode");
        assert_eq!(before.to_luma8().into_raw(), expected_pixels());
        assert_eq!(after.to_luma8().into_raw(), expected_pixels());
    }

    #[test]
    fn unsupported_and_broken_files_are_refused() {
        // A RAW (DNG) file.
        let mut b = B::new(true);
        let mut e = base(&b);
        e.push(b.blob(50706, 1, 4, &[1, 4, 0, 0]));
        let at = b.ifd(e, 0);
        assert_eq!(
            analyze_bytes(b.finish(at)).unwrap_err(),
            "RAW and DNG files are not supported"
        );

        // A second page.
        let mut b = B::new(true);
        let e = base(&b);
        let at = b.ifd(e, 8);
        assert_eq!(
            analyze_bytes(b.finish(at)).unwrap_err(),
            "multi-page TIFF files are not supported"
        );

        // Sub-pictures.
        let mut b = B::new(true);
        let mut e = base(&b);
        e.push(b.long(330, 8));
        let at = b.ifd(e, 0);
        assert_eq!(
            analyze_bytes(b.finish(at)).unwrap_err(),
            "unusual TIFF structure"
        );

        // Removable data that lies on top of the picture data.
        let mut b = B::new(true);
        let mut e = base(&b);
        e.push(E {
            tag: 269,
            kind: 2,
            count: 10,
            field: [8, 0, 0, 0],
        });
        let at = b.ifd(e, 0);
        assert_eq!(
            analyze_bytes(b.finish(at)).unwrap_err(),
            "unusual TIFF structure"
        );

        // Data that lies outside of the file.
        let mut b = B::new(true);
        let mut e = base(&b);
        e.push(E {
            tag: 271,
            kind: 2,
            count: 100,
            field: [0xFF, 0xFF, 0xFF, 0x00],
        });
        let at = b.ifd(e, 0);
        assert_eq!(
            analyze_bytes(b.finish(at)).unwrap_err(),
            "damaged or incomplete TIFF file"
        );

        assert_eq!(
            analyze_bytes(b"hello world, plain text".to_vec()).unwrap_err(),
            "not a valid TIFF file"
        );
        assert_eq!(
            analyze_bytes(vec![0x49, 0x49, 42]).unwrap_err(),
            "damaged or incomplete TIFF file"
        );
        assert!(analyze_bytes(Vec::new()).is_err());
    }

    #[test]
    fn every_truncation_is_rejected() {
        let base_file = secret_file();
        for length in 0..=base_file.len() {
            let result = analyze_bytes(base_file[..length].to_vec());
            if length == base_file.len() {
                assert!(result.is_ok(), "the complete file should parse");
            } else {
                assert!(result.is_err(), "prefix {length} should be rejected");
            }
        }
    }

    #[test]
    fn random_damage_never_panics_and_plans_stay_sane() {
        let base_file = secret_file();
        let options = CleanOptions::default();
        let mut rng = TestRng::new(0x3C6E_F372_FE94_F82B);
        for _ in 0..3000 {
            let mut data = base_file.clone();
            let flips = 1 + (rng.next_u64() % 3) as usize;
            for _ in 0..flips {
                let index = (rng.next_u64() % data.len() as u64) as usize;
                data[index] = (rng.next_u64() & 0xFF) as u8;
            }
            if let Ok(analysis) =
                analyze(&mut Cursor::new(data.clone()), data.len() as u64, &options)
            {
                assert_plan_sane(&analysis);
                assert_eq!(analysis.plan.output_len(), data.len() as u64);
            }
        }
    }

    /// Needs the sample file of the owner (and the image crate's TIFF
    /// decoder for the pixel check). Run by hand:
    /// cargo test live_real_tiff -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_real_tiff_is_cleaned() {
        use crate::tools::metadata_remover::dispatch::analyze_path;
        use crate::tools::metadata_remover::pipeline::{FileResult, clean_one_file};

        let source = PathBuf::from("C:/dev/erxtools-testdata/Mediatest/file_example_TIFF_5MB.tiff");
        let out_dir = PathBuf::from("C:/dev/erxtools/target/mr-tiff-live-out");
        let _ = std::fs::remove_dir_all(&out_dir);
        std::fs::create_dir_all(&out_dir).unwrap();
        let before = std::fs::read(&source).expect("sample file");
        let options = CleanOptions::default();
        let found = analyze_path(&source, &options).expect("analysis");
        println!(
            "TIFF sample: format {:?}, {} findings",
            found.format,
            found.findings.len()
        );
        for finding in &found.findings {
            let detail = if finding.category == Category::Location {
                None
            } else {
                finding.detail.clone()
            };
            println!(
                "  found: {:?} | {} | {:?}",
                finding.category, finding.label, detail
            );
        }
        for note in &found.kept {
            println!("  kept: {note}");
        }
        assert!(found.has_changes(), "nothing was found in the sample");

        let result = clean_one_file(&source, Some(out_dir.as_path()), &options, &mut |_, _| true)
            .expect("cleaning");
        match result {
            FileResult::Cleaned(done) => {
                println!(
                    "  cleaned: {} -> {} bytes, pixels verified: {}",
                    done.source_size, done.output_size, done.pixels_verified
                );
                assert_eq!(done.output_size, done.source_size, "the size changed");
                assert!(done.pixels_verified, "the pixels were not compared");
                let again = analyze_path(&done.output_path, &options).unwrap();
                assert!(!again.has_changes(), "a second run still changes things");
            }
            other => panic!("unexpected result {other:?}"),
        }
        assert_eq!(
            std::fs::read(&source).unwrap(),
            before,
            "the original changed"
        );
        std::fs::remove_dir_all(&out_dir).unwrap();
    }
}
