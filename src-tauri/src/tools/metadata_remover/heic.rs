//! HEIC handler (the ISO base media "meta" box of a HEIF picture).
//!
//! Nothing is re-encoded or moved. The Exif item and the XMP item (a "mime"
//! item with the type application/rdf+xml) are removed IN PLACE:
//! - the data of the item is overwritten with zeros (it lies in mdat or in
//!   idat, never anywhere else; otherwise the file is refused);
//! - the item type in its infe entry is changed to "free", so no reader
//!   takes the zeros for Exif or XMP.
//!
//! The file keeps its size and every offset stays valid. Pictures, tiles,
//! thumbnails and the HDR gain map are never touched. The rotation of a HEIC
//! photo is stored with the picture (irot / imir), not in Exif, so removing
//! Exif does not turn the photo. The color profile is a property of the
//! picture; it stays (it cannot be removed here).

use std::io::{Read, Seek, SeekFrom};

use super::model::{Analysis, Category, CleanOptions, FileFormat, Finding, MAX_RAW_METADATA};
use super::patch::Patches;

/// Total number of boxes looked at in one file.
const MAX_BOXES: u32 = 100_000;
const MAX_ITEMS: usize = 100_000;
const MAX_EXTENTS: usize = 200_000;
/// Largest iinf or iloc table that is read into memory (8 MiB).
const MAX_TABLE_BYTES: u64 = 8 * 1024 * 1024;

fn damaged() -> String {
    "damaged or incomplete HEIC file".to_string()
}

fn unusual() -> String {
    "unusual HEIC structure".to_string()
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

fn be16(data: &[u8], at: usize) -> Option<u16> {
    let end = at.checked_add(2)?;
    Some(u16::from_be_bytes(data.get(at..end)?.try_into().ok()?))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    Some(u32::from_be_bytes(data.get(at..end)?.try_into().ok()?))
}

fn be64(data: &[u8], at: usize) -> Option<u64> {
    let end = at.checked_add(8)?;
    Some(u64::from_be_bytes(data.get(at..end)?.try_into().ok()?))
}

/// A number of `width` bytes (0, 4 or 8) as the iloc box stores them.
fn uint(data: &[u8], at: usize, width: usize) -> Option<u64> {
    match width {
        0 => Some(0),
        4 => be32(data, at).map(u64::from),
        8 => be64(data, at),
        _ => None,
    }
}

/// One box: its type, where it starts and ends, and how long its header is.
#[derive(Debug, Clone, Copy)]
struct Bx {
    kind: [u8; 4],
    start: u64,
    end: u64,
    header_len: u64,
}

impl Bx {
    fn payload(&self) -> u64 {
        self.start + self.header_len
    }
}

/// Reads the box that starts at `pos` inside a container that ends at
/// `limit`. None when fewer than 8 bytes are left.
fn read_box<R: Read + Seek>(src: &mut R, pos: u64, limit: u64) -> Result<Option<Bx>, String> {
    if limit <= pos || limit - pos < 8 {
        return Ok(None);
    }
    let mut head = [0u8; 8];
    read_at(src, pos, &mut head)?;
    let [s0, s1, s2, s3, k0, k1, k2, k3] = head;
    let size32 = u32::from_be_bytes([s0, s1, s2, s3]);
    let kind = [k0, k1, k2, k3];
    let (size, header_len) = match size32 {
        0 => (limit - pos, 8u64),
        1 => {
            if limit - pos < 16 {
                return Err(damaged());
            }
            let mut big = [0u8; 8];
            read_at(src, pos + 8, &mut big)?;
            (u64::from_be_bytes(big), 16u64)
        }
        other => (u64::from(other), 8u64),
    };
    if size < header_len || size > limit - pos {
        return Err(damaged());
    }
    Ok(Some(Bx {
        kind,
        start: pos,
        end: pos + size,
        header_len,
    }))
}

/// The boxes between `start` and `end`. Fewer than 8 trailing bytes are
/// ignored.
fn children<R: Read + Seek>(
    src: &mut R,
    budget: &mut u32,
    start: u64,
    end: u64,
) -> Result<Vec<Bx>, String> {
    let mut out = Vec::new();
    let mut pos = start;
    while let Some(found) = read_box(src, pos, end)? {
        *budget += 1;
        if *budget > MAX_BOXES {
            return Err(damaged());
        }
        pos = found.end;
        out.push(found);
    }
    Ok(out)
}

fn find(kids: &[Bx], kind: &[u8; 4]) -> Option<Bx> {
    kids.iter().find(|found| &found.kind == kind).copied()
}

/// The payload of a box (without its header), read into memory.
fn read_payload<R: Read + Seek>(src: &mut R, b: &Bx) -> Result<Vec<u8>, String> {
    let len = b.end - b.payload();
    if len > MAX_TABLE_BYTES {
        return Err(unusual());
    }
    read_vec(src, b.payload(), len)
}

/// One entry of the item information table (iinf): the id of the item, its
/// type, where the 4 bytes of the type are in the file and, for "mime"
/// items, the content type in lower case.
struct Entry {
    id: u32,
    kind: [u8; 4],
    kind_pos: u64,
    ctype: String,
}

/// `body` is the payload of one infe box; `body_start` is where it starts in
/// the file. Entries older than version 2 have no item type and are skipped.
fn parse_infe(body: &[u8], body_start: u64) -> Result<Option<Entry>, String> {
    let version = *body.first().ok_or_else(damaged)?;
    if version < 2 {
        return Ok(None);
    }
    let (id, mut at) = if version == 2 {
        (u32::from(be16(body, 4).ok_or_else(damaged)?), 6usize)
    } else {
        (be32(body, 4).ok_or_else(damaged)?, 8usize)
    };
    // The protection index (2 bytes) sits between the id and the type.
    at += 2;
    let end = at.checked_add(4).ok_or_else(damaged)?;
    let kind: [u8; 4] = body
        .get(at..end)
        .ok_or_else(damaged)?
        .try_into()
        .map_err(|_| damaged())?;
    let kind_pos = body_start + at as u64;
    let rest = body.get(end..).ok_or_else(damaged)?;
    let ctype = match rest.iter().position(|byte| *byte == 0) {
        Some(name_end) => {
            let after = rest.get(name_end + 1..).ok_or_else(damaged)?;
            let text_end = after
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(after.len());
            String::from_utf8_lossy(after.get(..text_end).ok_or_else(damaged)?).to_lowercase()
        }
        None => String::new(),
    };
    Ok(Some(Entry {
        id,
        kind,
        kind_pos,
        ctype,
    }))
}

/// `data` is the payload of the iinf box; `payload_start` is where it starts
/// in the file.
fn parse_iinf(data: &[u8], payload_start: u64) -> Result<Vec<Entry>, String> {
    let version = *data.first().ok_or_else(damaged)?;
    let count_len = if version == 0 { 2usize } else { 4usize };
    let mut at = 4 + count_len;
    let mut out = Vec::new();
    while at + 8 <= data.len() {
        let size = be32(data, at).ok_or_else(damaged)? as usize;
        let kind: [u8; 4] = data
            .get(at + 4..at + 8)
            .ok_or_else(damaged)?
            .try_into()
            .map_err(|_| damaged())?;
        let end = at.checked_add(size).ok_or_else(damaged)?;
        if size < 8 || end > data.len() {
            return Err(damaged());
        }
        if &kind == b"infe" {
            if out.len() >= MAX_ITEMS {
                return Err(unusual());
            }
            let body = data.get(at + 8..end).ok_or_else(damaged)?;
            if let Some(entry) = parse_infe(body, payload_start + at as u64 + 8)? {
                out.push(entry);
            }
        }
        at = end;
    }
    Ok(out)
}

/// Where the data of one item is stored: the construction method (0 = in
/// the file, 1 = in idat) and its extents as (start, length), the start
/// being relative to the file or to idat.
struct Loc {
    id: u32,
    method: u16,
    data_ref: u16,
    extents: Vec<(u64, u64)>,
}

struct Table {
    items: Vec<Loc>,
    length_size: usize,
}

/// `d` is the payload of the iloc box.
fn parse_iloc(d: &[u8]) -> Result<Table, String> {
    let version = *d.first().ok_or_else(damaged)?;
    if version > 2 {
        return Err(unusual());
    }
    let sizes = *d.get(4).ok_or_else(damaged)?;
    let more = *d.get(5).ok_or_else(damaged)?;
    let offset_size = usize::from(sizes >> 4);
    let length_size = usize::from(sizes & 15);
    let base_size = usize::from(more >> 4);
    let index_size = if version >= 1 {
        usize::from(more & 15)
    } else {
        0
    };
    let valid = |width: usize| matches!(width, 0 | 4 | 8);
    if !(valid(offset_size) && valid(length_size) && valid(base_size) && valid(index_size)) {
        return Err(unusual());
    }
    let mut at = 6usize;
    let count = if version < 2 {
        let value = be16(d, at).ok_or_else(damaged)?;
        at += 2;
        usize::from(value)
    } else {
        let value = be32(d, at).ok_or_else(damaged)?;
        at += 4;
        value as usize
    };
    if count > MAX_ITEMS {
        return Err(unusual());
    }
    let mut items = Vec::new();
    let mut total = 0usize;
    for _ in 0..count {
        let id = if version < 2 {
            let value = be16(d, at).ok_or_else(damaged)?;
            at += 2;
            u32::from(value)
        } else {
            let value = be32(d, at).ok_or_else(damaged)?;
            at += 4;
            value
        };
        let method = if version >= 1 {
            let value = be16(d, at).ok_or_else(damaged)?;
            at += 2;
            value & 15
        } else {
            0
        };
        let data_ref = be16(d, at).ok_or_else(damaged)?;
        at += 2;
        let base = uint(d, at, base_size).ok_or_else(damaged)?;
        at += base_size;
        let extent_count = usize::from(be16(d, at).ok_or_else(damaged)?);
        at += 2;
        total += extent_count;
        if total > MAX_EXTENTS {
            return Err(unusual());
        }
        let mut extents = Vec::new();
        for _ in 0..extent_count {
            at += index_size;
            let offset = uint(d, at, offset_size).ok_or_else(damaged)?;
            at += offset_size;
            let length = uint(d, at, length_size).ok_or_else(damaged)?;
            at += length_size;
            let start = base.checked_add(offset).ok_or_else(unusual)?;
            extents.push((start, length));
        }
        items.push(Loc {
            id,
            method,
            data_ref,
            extents,
        });
    }
    Ok(Table { items, length_size })
}

/// The TIFF block of an Exif item: 4 bytes with the distance to the TIFF
/// header (counted from the end of those 4 bytes), then the data.
fn exif_tiff(item: &[u8]) -> Option<&[u8]> {
    let offset = usize::try_from(be32(item, 0)?).ok()?;
    let tiff = item.get(4usize.checked_add(offset)?..)?;
    if tiff.is_empty() { None } else { Some(tiff) }
}

fn within(start: u64, end: u64, areas: &[(u64, u64)]) -> bool {
    areas.iter().any(|(from, to)| start >= *from && end <= *to)
}

/// Analyzes a HEIC file of `len` bytes.
pub fn analyze<R: Read + Seek>(
    src: &mut R,
    len: u64,
    options: &CleanOptions,
) -> Result<Analysis, String> {
    let mut budget = 0u32;
    let top = children(src, &mut budget, 0, len)?;
    if top.is_empty() {
        return Err("not a valid HEIC file".to_string());
    }
    let mut mdats: Vec<(u64, u64)> = Vec::new();
    let mut metas: Vec<Bx> = Vec::new();
    for b in &top {
        match &b.kind {
            b"mdat" => mdats.push((b.payload(), b.end)),
            b"meta" => metas.push(*b),
            _ => {}
        }
    }
    let meta = match metas.as_slice() {
        [one] => *one,
        [] => return Err("no picture information found in the file".to_string()),
        _ => return Err(unusual()),
    };
    if meta.end - meta.payload() < 4 {
        return Err(damaged());
    }
    let kids = children(src, &mut budget, meta.payload() + 4, meta.end)?;
    let iinf = find(&kids, b"iinf").ok_or_else(unusual)?;
    let iloc = find(&kids, b"iloc").ok_or_else(unusual)?;
    let idat = find(&kids, b"idat").map(|b| (b.payload(), b.end));
    let entries = parse_iinf(&read_payload(src, &iinf)?, iinf.payload())?;
    let table = parse_iloc(&read_payload(src, &iloc)?)?;

    let mut analysis = Analysis::new(FileFormat::Heic, len);
    let mut patches = Patches::new();
    let mut removed_ids: Vec<u32> = Vec::new();
    let mut removed_ranges: Vec<(u64, u64)> = Vec::new();

    for entry in &entries {
        let is_exif = &entry.kind == b"Exif";
        let is_xmp = &entry.kind == b"mime" && entry.ctype.starts_with("application/rdf+xml");
        if !is_exif && !is_xmp {
            continue;
        }
        removed_ids.push(entry.id);
        let mut data: Option<Vec<u8>> = None;
        if let Some(loc) = table.items.iter().find(|loc| loc.id == entry.id) {
            if loc.data_ref != 0 || table.length_size == 0 {
                return Err(unusual());
            }
            let areas: Vec<(u64, u64)> = match loc.method {
                0 => mdats.clone(),
                1 => idat.into_iter().collect(),
                _ => return Err(unusual()),
            };
            let shift = if loc.method == 1 {
                idat.map_or(0, |(from, _)| from)
            } else {
                0
            };
            for (offset, length) in &loc.extents {
                if *length == 0 {
                    continue;
                }
                let start = shift.checked_add(*offset).ok_or_else(unusual)?;
                let end = start.checked_add(*length).ok_or_else(unusual)?;
                if !within(start, end, &areas) {
                    return Err(unusual());
                }
                patches.zero(start, end);
                removed_ranges.push((start, end));
            }
            if let [(offset, length)] = loc.extents.as_slice()
                && *length > 0
                && *length <= MAX_RAW_METADATA as u64
            {
                let first = shift.checked_add(*offset).ok_or_else(unusual)?;
                data = Some(read_vec(src, first, *length)?);
            }
        }
        patches.replace(entry.kind_pos, entry.kind_pos + 4, b"free".to_vec());
        if is_exif {
            if analysis.exif.is_none()
                && let Some(bytes) = &data
                && let Some(tiff) = exif_tiff(bytes)
            {
                analysis.exif = Some(tiff.to_vec());
            }
            analysis.add_finding(Finding::new(Category::Other, "EXIF metadata block"));
        } else {
            if analysis.xmp.is_none()
                && let Some(bytes) = data
            {
                analysis.xmp = Some(bytes);
            }
            analysis.add_finding(Finding::new(Category::Xmp, "XMP metadata"));
        }
    }

    // A picture that shares bytes with a removed item must never be zeroed.
    for loc in &table.items {
        if removed_ids.contains(&loc.id) {
            continue;
        }
        let shift = match loc.method {
            0 => 0u64,
            1 => match idat {
                Some((from, _)) => from,
                None => continue,
            },
            _ => continue,
        };
        for (offset, length) in &loc.extents {
            if *length == 0 {
                continue;
            }
            let Some(start) = shift.checked_add(*offset) else {
                continue;
            };
            let Some(end) = start.checked_add(*length) else {
                continue;
            };
            if removed_ranges
                .iter()
                .any(|(from, to)| start < *to && *from < end)
            {
                return Err(unusual());
            }
        }
    }

    // What is kept: rotation and color profile are properties of the picture.
    let mut rotation = false;
    let mut profile = false;
    if let Some(iprp) = find(&kids, b"iprp") {
        let parts = children(src, &mut budget, iprp.payload(), iprp.end)?;
        if let Some(ipco) = find(&parts, b"ipco") {
            let props = children(src, &mut budget, ipco.payload(), ipco.end)?;
            for prop in &props {
                match &prop.kind {
                    b"irot" | b"imir" => rotation = true,
                    b"colr" => {
                        let mut kind = [0u8; 4];
                        if prop.end - prop.payload() >= 4
                            && read_at(src, prop.payload(), &mut kind).is_ok()
                            && (&kind == b"prof" || &kind == b"rICC")
                        {
                            profile = true;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if rotation {
        analysis.add_kept("Photo rotation (stored with the picture)");
    }
    if profile {
        if options.keep_color_profile {
            analysis.add_kept("Color profile (ICC)");
        } else {
            analysis.add_kept("Color profile (ICC) (cannot be removed from HEIC files)");
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
    use crate::tools::metadata_remover::orientation::minimal_tiff;
    use std::io::Cursor;
    use std::path::PathBuf;

    const IMAGE_LEN: usize = 40;
    const ROTATION_NOTE: &str = "Photo rotation (stored with the picture)";

    fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(payload);
        out
    }

    /// A box with version and flags in front of its body.
    fn full(kind: &[u8; 4], version: u8, body: &[u8]) -> Vec<u8> {
        let mut payload = vec![version, 0, 0, 0];
        payload.extend_from_slice(body);
        bx(kind, &payload)
    }

    fn infe(id: u16, kind: &[u8; 4], name: &str, ctype: Option<&str>) -> Vec<u8> {
        let mut body = id.to_be_bytes().to_vec();
        body.extend_from_slice(&[0, 0]);
        body.extend_from_slice(kind);
        body.extend_from_slice(name.as_bytes());
        body.push(0);
        if let Some(text) = ctype {
            body.extend_from_slice(text.as_bytes());
            body.push(0);
        }
        full(b"infe", 2, &body)
    }

    struct Place1 {
        id: u16,
        method: u16,
        offset: u32,
        len: u32,
    }

    fn iloc(v0: bool, locs: &[Place1]) -> Vec<u8> {
        let mut body = Vec::new();
        if v0 {
            body.extend_from_slice(&[0x44, 0x40]);
        } else {
            body.extend_from_slice(&[0x44, 0x00]);
        }
        body.extend_from_slice(&(locs.len() as u16).to_be_bytes());
        for loc in locs {
            body.extend_from_slice(&loc.id.to_be_bytes());
            if v0 {
                body.extend_from_slice(&[0, 0]);
                body.extend_from_slice(&loc.offset.to_be_bytes());
                body.extend_from_slice(&1u16.to_be_bytes());
                body.extend_from_slice(&0u32.to_be_bytes());
            } else {
                body.extend_from_slice(&loc.method.to_be_bytes());
                body.extend_from_slice(&[0, 0]);
                body.extend_from_slice(&1u16.to_be_bytes());
                body.extend_from_slice(&loc.offset.to_be_bytes());
            }
            body.extend_from_slice(&loc.len.to_be_bytes());
        }
        full(b"iloc", if v0 { 0 } else { 1 }, &body)
    }

    fn exif_data() -> Vec<u8> {
        let mut data = vec![0, 0, 0, 6];
        data.extend_from_slice(b"Exif\0\0");
        data.extend(minimal_tiff(6));
        data.extend_from_slice(b"SECRET-1");
        data
    }

    fn xmp_data() -> Vec<u8> {
        b"<x:xmpmeta>SECRET-XMP</x:xmpmeta>".to_vec()
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Absent,
        Present,
        Cleaned,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Place {
        Normal,
        Idat,
        Outside,
        Overlap,
    }

    #[derive(Clone, Copy)]
    struct Spec {
        exif: State,
        xmp: State,
        place: Place,
        props: bool,
        v0: bool,
    }

    impl Spec {
        fn base() -> Spec {
            Spec {
                exif: State::Present,
                xmp: State::Present,
                place: Place::Normal,
                props: true,
                v0: false,
            }
        }
    }

    /// ftyp, meta (item table, locations, optional idat and properties) and
    /// mdat. The Exif item and the XMP item are present, absent, or already
    /// cleaned (type "free", zeros), so the expected result of a cleaning is
    /// the same fixture in the Cleaned state.
    fn heic(spec: Spec) -> Vec<u8> {
        assert!(!(spec.v0 && spec.place == Place::Idat));
        let mut mdat = vec![0x11u8; IMAGE_LEN];
        let mut idat: Vec<u8> = Vec::new();
        let mut exif_at: Option<(usize, usize)> = None;
        let mut xmp_at: Option<(usize, usize)> = None;
        if spec.exif != State::Absent {
            let mut data = exif_data();
            if spec.exif == State::Cleaned {
                data = vec![0u8; data.len()];
            }
            if spec.place == Place::Idat {
                exif_at = Some((idat.len(), data.len()));
                idat.extend(data);
            } else {
                exif_at = Some((mdat.len(), data.len()));
                mdat.extend(data);
            }
        }
        if spec.xmp != State::Absent {
            let mut data = xmp_data();
            if spec.xmp == State::Cleaned {
                data = vec![0u8; data.len()];
            }
            xmp_at = Some((mdat.len(), data.len()));
            mdat.extend(data);
        }
        let ftyp = bx(b"ftyp", b"heic\0\0\0\0mif1heic");
        let meta_for = |mdat_start: u32| -> Vec<u8> {
            let mut entries = vec![infe(1, b"hvc1", "", None)];
            let mut locs = vec![Place1 {
                id: 1,
                method: 0,
                offset: mdat_start,
                len: IMAGE_LEN as u32,
            }];
            if let Some((at, length)) = exif_at {
                let kind: &[u8; 4] = if spec.exif == State::Cleaned {
                    b"free"
                } else {
                    b"Exif"
                };
                entries.push(infe(2, kind, "", None));
                let (method, offset) = match spec.place {
                    Place::Normal => (0u16, mdat_start + at as u32),
                    Place::Idat => (1u16, at as u32),
                    Place::Outside => (0u16, 8u32),
                    Place::Overlap => (0u16, mdat_start + 10),
                };
                locs.push(Place1 {
                    id: 2,
                    method,
                    offset,
                    len: length as u32,
                });
            }
            if let Some((at, length)) = xmp_at {
                let kind: &[u8; 4] = if spec.xmp == State::Cleaned {
                    b"free"
                } else {
                    b"mime"
                };
                entries.push(infe(3, kind, "", Some("application/rdf+xml")));
                locs.push(Place1 {
                    id: 3,
                    method: 0,
                    offset: mdat_start + at as u32,
                    len: length as u32,
                });
            }
            let mut table = (entries.len() as u16).to_be_bytes().to_vec();
            for entry in &entries {
                table.extend_from_slice(entry);
            }
            let mut kids = vec![
                full(b"pitm", 0, &[0, 1]),
                full(b"iinf", 0, &table),
                iloc(spec.v0, &locs),
            ];
            if !idat.is_empty() {
                kids.push(bx(b"idat", &idat));
            }
            if spec.props {
                let props = [bx(b"irot", &[1]), bx(b"colr", b"prof\0\0\0\0")].concat();
                kids.push(bx(b"iprp", &bx(b"ipco", &props)));
            }
            let mut payload = vec![0u8; 4];
            for kid in &kids {
                payload.extend_from_slice(kid);
            }
            bx(b"meta", &payload)
        };
        let probe = meta_for(0);
        let mdat_start = (ftyp.len() + probe.len() + 8) as u32;
        let meta = meta_for(mdat_start);
        assert_eq!(meta.len(), probe.len());
        let mut out = ftyp;
        out.extend(meta);
        out.extend(bx(b"mdat", &mdat));
        out
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

    fn all_kept() -> Vec<String> {
        vec![
            ROTATION_NOTE.to_string(),
            "Color profile (ICC)".to_string(),
            "Picture data".to_string(),
        ]
    }

    #[test]
    fn a_clean_file_is_left_alone() {
        let data = heic(Spec {
            exif: State::Absent,
            xmp: State::Absent,
            ..Spec::base()
        });
        let (analysis, out) = run(&data);
        assert!(analysis.findings.is_empty());
        assert!(!analysis.has_changes());
        assert_eq!(analysis.kept, all_kept());
        assert_eq!(out, data);
    }

    #[test]
    fn metadata_is_zeroed_and_the_items_are_renamed() {
        let data = heic(Spec::base());
        let (analysis, out) = run(&data);
        let expected = heic(Spec {
            exif: State::Cleaned,
            xmp: State::Cleaned,
            ..Spec::base()
        });
        assert_eq!(out, expected);
        assert_eq!(out.len(), data.len());
        assert!(!contains(&out, b"SECRET"));
        assert_eq!(
            labels(&analysis),
            vec!["EXIF metadata block", "XMP metadata"]
        );
        assert_eq!(analysis.kept, all_kept());
        assert_eq!(analysis.exif.as_deref(), Some(&exif_data()[10..]));
        assert_eq!(analysis.xmp.as_deref(), Some(xmp_data().as_slice()));
    }

    #[test]
    fn a_second_pass_finds_nothing() {
        let (_, out) = run(&heic(Spec::base()));
        let (again, again_out) = run(&out);
        assert!(again.findings.is_empty(), "{:?}", labels(&again));
        assert!(!again.has_changes());
        assert_eq!(again_out, out);
    }

    #[test]
    fn only_one_kind_of_metadata() {
        let exif_only = heic(Spec {
            xmp: State::Absent,
            ..Spec::base()
        });
        let (analysis, out) = run(&exif_only);
        assert_eq!(
            out,
            heic(Spec {
                exif: State::Cleaned,
                xmp: State::Absent,
                ..Spec::base()
            })
        );
        assert_eq!(labels(&analysis), vec!["EXIF metadata block"]);

        let xmp_only = heic(Spec {
            exif: State::Absent,
            ..Spec::base()
        });
        let (analysis, out) = run(&xmp_only);
        assert_eq!(
            out,
            heic(Spec {
                exif: State::Absent,
                xmp: State::Cleaned,
                ..Spec::base()
            })
        );
        assert_eq!(labels(&analysis), vec!["XMP metadata"]);
        assert_eq!(analysis.xmp.as_deref(), Some(xmp_data().as_slice()));
    }

    #[test]
    fn iloc_version_zero_with_base_offsets() {
        let data = heic(Spec {
            v0: true,
            ..Spec::base()
        });
        let (analysis, out) = run(&data);
        assert_eq!(
            out,
            heic(Spec {
                exif: State::Cleaned,
                xmp: State::Cleaned,
                v0: true,
                ..Spec::base()
            })
        );
        assert_eq!(
            labels(&analysis),
            vec!["EXIF metadata block", "XMP metadata"]
        );
        assert!(!contains(&out, b"SECRET"));
    }

    #[test]
    fn data_inside_idat_is_zeroed() {
        let data = heic(Spec {
            xmp: State::Absent,
            place: Place::Idat,
            ..Spec::base()
        });
        let (analysis, out) = run(&data);
        assert_eq!(
            out,
            heic(Spec {
                exif: State::Cleaned,
                xmp: State::Absent,
                place: Place::Idat,
                ..Spec::base()
            })
        );
        assert_eq!(labels(&analysis), vec!["EXIF metadata block"]);
        assert!(!contains(&out, b"SECRET"));
    }

    #[test]
    fn the_color_profile_note_follows_the_option() {
        let data = heic(Spec {
            exif: State::Absent,
            xmp: State::Absent,
            ..Spec::base()
        });
        let options = CleanOptions {
            keep_orientation: true,
            keep_color_profile: false,
        };
        let (analysis, out) = run_with(&data, &options);
        assert!(!analysis.has_changes());
        assert_eq!(
            analysis.kept,
            vec![
                ROTATION_NOTE.to_string(),
                "Color profile (ICC) (cannot be removed from HEIC files)".to_string(),
                "Picture data".to_string(),
            ]
        );
        assert_eq!(out, data);

        let without = heic(Spec {
            exif: State::Absent,
            xmp: State::Absent,
            props: false,
            ..Spec::base()
        });
        let (analysis, _) = run(&without);
        assert_eq!(analysis.kept, vec!["Picture data".to_string()]);
    }

    #[test]
    fn bad_places_are_refused() {
        for place in [Place::Outside, Place::Overlap] {
            let data = heic(Spec {
                xmp: State::Absent,
                place,
                ..Spec::base()
            });
            let error = analyze(
                &mut Cursor::new(data.clone()),
                data.len() as u64,
                &CleanOptions::default(),
            )
            .unwrap_err();
            assert_eq!(error, "unusual HEIC structure");
        }
    }

    fn plain(middle: &[Vec<u8>]) -> Vec<u8> {
        let mut out = bx(b"ftyp", b"heic\0\0\0\0mif1heic");
        for part in middle {
            out.extend_from_slice(part);
        }
        out.extend(bx(b"mdat", &[1, 2, 3, 4]));
        out
    }

    #[test]
    fn broken_structures_are_rejected() {
        let check = |bytes: Vec<u8>| {
            let len = bytes.len() as u64;
            analyze(&mut Cursor::new(bytes), len, &CleanOptions::default())
        };
        assert!(check(Vec::new()).is_err());
        assert!(check(b"hello world, plain text".to_vec()).is_err());
        assert_eq!(
            check(plain(&[])).unwrap_err(),
            "no picture information found in the file"
        );
        let empty_meta = bx(b"meta", &[0, 0, 0, 0]);
        assert_eq!(
            check(plain(&[empty_meta.clone()])).unwrap_err(),
            "unusual HEIC structure"
        );
        assert_eq!(
            check(plain(&[empty_meta.clone(), empty_meta])).unwrap_err(),
            "unusual HEIC structure"
        );
        let table = full(b"iinf", 0, &[0, 0]);
        let wrong_version = bx(
            b"meta",
            &[
                vec![0u8; 4],
                table.clone(),
                full(b"iloc", 3, &[0x44, 0, 0, 0]),
            ]
            .concat(),
        );
        assert_eq!(
            check(plain(&[wrong_version])).unwrap_err(),
            "unusual HEIC structure"
        );
        let odd_size = bx(
            b"meta",
            &[
                vec![0u8; 4],
                table.clone(),
                full(b"iloc", 1, &[0x34, 0, 0, 0]),
            ]
            .concat(),
        );
        assert_eq!(
            check(plain(&[odd_size])).unwrap_err(),
            "unusual HEIC structure"
        );
        let huge_count = bx(
            b"meta",
            &[
                vec![0u8; 4],
                table,
                full(b"iloc", 1, &[0x44, 0, 0xFF, 0xFF]),
            ]
            .concat(),
        );
        assert!(check(plain(&[huge_count])).is_err());
    }

    #[test]
    fn every_truncation_is_rejected() {
        let base = heic(Spec::base());
        let options = CleanOptions::default();
        for length in 0..=base.len() {
            let result = analyze(
                &mut Cursor::new(base[..length].to_vec()),
                length as u64,
                &options,
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
        let base = heic(Spec::base());
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

    /// Needs the sample files of the owner (and the Windows HEIF codecs for
    /// the pixel check). Run by hand:
    /// cargo test live_real_heic -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_real_heic_samples_are_cleaned() {
        use crate::tools::metadata_remover::dispatch::analyze_path;
        use crate::tools::metadata_remover::pipeline::{FileResult, clean_one_file};

        let samples = "C:/dev/erxtools-testdata/Mediatest";
        let out_dir = PathBuf::from("C:/dev/erxtools/target/mr-heic-live-out");
        let _ = std::fs::remove_dir_all(&out_dir);
        std::fs::create_dir_all(&out_dir).unwrap();
        for name in ["b.heic", "c.heic", "d.heic", "e.heic", "f.heic"] {
            let source = PathBuf::from(format!("{samples}/{name}"));
            let before = std::fs::read(&source).unwrap();
            let options = CleanOptions::default();
            let found =
                analyze_path(&source, &options).unwrap_or_else(|error| panic!("{name}: {error}"));
            println!(
                "{name}: format {:?}, {} findings",
                found.format,
                found.findings.len()
            );
            for finding in &found.findings {
                // The location is never printed, only that it exists.
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
            let result =
                clean_one_file(&source, Some(out_dir.as_path()), &options, &mut |_, _| true)
                    .unwrap_or_else(|error| panic!("{name}: {error}"));
            match result {
                FileResult::Cleaned(done) => {
                    println!(
                        "  cleaned: {} -> {} bytes, pixels verified: {}",
                        done.source_size, done.output_size, done.pixels_verified
                    );
                    assert_eq!(done.output_size, done.source_size, "{name}: size changed");
                    assert!(
                        done.pixels_verified,
                        "{name}: pixels were not compared (needs the Windows HEIF codecs)"
                    );
                    let again = analyze_path(&done.output_path, &options).unwrap();
                    assert!(
                        !again.has_changes(),
                        "{name}: a second run still changes things"
                    );
                }
                FileResult::NothingToRemove { .. } => println!("  nothing to remove"),
                FileResult::Cancelled => panic!("{name}: cancelled"),
            }
            assert_eq!(
                std::fs::read(&source).unwrap(),
                before,
                "{name}: the original changed"
            );
        }
        std::fs::remove_dir_all(&out_dir).unwrap();
    }
}
