//! Video handler for MP4 and MOV (the ISO base media / QuickTime boxes).
//!
//! Nothing is re-encoded or moved. Metadata boxes are overwritten IN PLACE
//! with "free" boxes of the same size, so every offset stays valid and the
//! file keeps its size:
//! - user data (udta), metadata (meta: location, make, model, software,
//!   creation date, ...), uuid boxes (XMP and others), preview pictures;
//! - timed-metadata tracks (handler "meta": location, motion or camera data)
//!   AND their samples inside mdat, which are overwritten with zeros;
//! - creation and modification times of the movie, tracks and media are set
//!   to zero.
//!
//! Video, audio and subtitle tracks are never touched. In fragmented files
//! (moof / mvex) tracks are left in place.

use std::io::{Read, Seek, SeekFrom};

use super::model::{Analysis, Category, CleanOptions, FileFormat, Finding};
use super::patch::{Patches, free_head};

/// Total number of boxes looked at in one file.
const MAX_BOXES: u32 = 200_000;
const MAX_KEYS: u32 = 200;
const MAX_KEYS_BYTES: u64 = 65_536;
const MAX_CHUNKS: u32 = 2_000_000;
const MAX_SAMPLES: u32 = 5_000_000;
const MAX_STSC: u32 = 100_000;
/// The uuid that marks an XMP packet.
const XMP_UUID: [u8; 16] = [
    0xBE, 0x7A, 0xCF, 0xCB, 0x97, 0xA9, 0x42, 0xE8, 0x9C, 0x71, 0x99, 0x94, 0x91, 0xE3, 0xAF, 0xAC,
];

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

fn be32(data: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    Some(u32::from_be_bytes(data.get(at..end)?.try_into().ok()?))
}

fn be64(data: &[u8], at: usize) -> Option<u64> {
    let end = at.checked_add(8)?;
    Some(u64::from_be_bytes(data.get(at..end)?.try_into().ok()?))
}

/// One box: its type, where it starts and ends in the file, and how long
/// its header is (8 bytes, or 16 with a 64-bit size).
#[derive(Debug, Clone, Copy)]
pub struct BoxInfo {
    pub kind: [u8; 4],
    pub start: u64,
    pub end: u64,
    pub header_len: u64,
}

impl BoxInfo {
    pub fn payload(&self) -> u64 {
        self.start + self.header_len
    }

    pub fn size(&self) -> u64 {
        self.end - self.start
    }

    pub fn has_payload(&self) -> bool {
        self.end > self.payload()
    }
}

/// Reads the box that starts at `pos` inside a container that ends at
/// `limit`. None when fewer than 8 bytes are left.
fn read_box<R: Read + Seek>(src: &mut R, pos: u64, limit: u64) -> Result<Option<BoxInfo>, String> {
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
    Ok(Some(BoxInfo {
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
) -> Result<Vec<BoxInfo>, String> {
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

fn find_child<R: Read + Seek>(
    src: &mut R,
    budget: &mut u32,
    parent: &BoxInfo,
    kind: &[u8; 4],
) -> Result<Option<BoxInfo>, String> {
    let kids = children(src, budget, parent.payload(), parent.end)?;
    Ok(kids.into_iter().find(|found| &found.kind == kind))
}

/// How the sizes of the samples of a track are stored.
enum Sizes {
    Uniform { size: u64, total: u64 },
    Table(Vec<u32>),
}

fn read_sizes<R: Read + Seek>(src: &mut R, b: &BoxInfo) -> Result<Sizes, String> {
    let p = b.payload();
    if b.end < p + 12 {
        return Err(damaged());
    }
    let mut head = [0u8; 12];
    read_at(src, p, &mut head)?;
    let [_, _, _, _, a0, a1, a2, a3, c0, c1, c2, c3] = head;
    let size = u32::from_be_bytes([a0, a1, a2, a3]);
    let count = u32::from_be_bytes([c0, c1, c2, c3]);
    if count > MAX_SAMPLES {
        return Err(unusual());
    }
    if size != 0 {
        return Ok(Sizes::Uniform {
            size: u64::from(size),
            total: u64::from(count),
        });
    }
    let need = u64::from(count) * 4;
    if p + 12 + need > b.end {
        return Err(damaged());
    }
    let raw = read_vec(src, p + 12, need)?;
    let mut table = Vec::with_capacity(count as usize);
    for index in 0..count as usize {
        table.push(be32(&raw, index * 4).ok_or_else(damaged)?);
    }
    Ok(Sizes::Table(table))
}

/// (first chunk, samples per chunk) entries of the sample-to-chunk table.
fn read_stsc<R: Read + Seek>(src: &mut R, b: &BoxInfo) -> Result<Vec<(u32, u32)>, String> {
    let p = b.payload();
    if b.end < p + 8 {
        return Err(damaged());
    }
    let mut head = [0u8; 8];
    read_at(src, p, &mut head)?;
    let [_, _, _, _, c0, c1, c2, c3] = head;
    let count = u32::from_be_bytes([c0, c1, c2, c3]);
    if count > MAX_STSC {
        return Err(unusual());
    }
    let need = u64::from(count) * 12;
    if p + 8 + need > b.end {
        return Err(damaged());
    }
    let raw = read_vec(src, p + 8, need)?;
    let mut out = Vec::with_capacity(count as usize);
    for index in 0..count as usize {
        let first = be32(&raw, index * 12).ok_or_else(damaged)?;
        let per_chunk = be32(&raw, index * 12 + 4).ok_or_else(damaged)?;
        out.push((first, per_chunk));
    }
    Ok(out)
}

/// The chunk offsets of a stco (width 4) or co64 (width 8) box.
fn read_offsets<R: Read + Seek>(src: &mut R, b: &BoxInfo, width: u64) -> Result<Vec<u64>, String> {
    let p = b.payload();
    if b.end < p + 8 {
        return Err(damaged());
    }
    let mut head = [0u8; 8];
    read_at(src, p, &mut head)?;
    let [_, _, _, _, c0, c1, c2, c3] = head;
    let count = u32::from_be_bytes([c0, c1, c2, c3]);
    if count > MAX_CHUNKS {
        return Err(unusual());
    }
    let need = u64::from(count) * width;
    if p + 8 + need > b.end {
        return Err(damaged());
    }
    let raw = read_vec(src, p + 8, need)?;
    let mut out = Vec::with_capacity(count as usize);
    for index in 0..count as usize {
        if width == 4 {
            out.push(u64::from(be32(&raw, index * 4).ok_or_else(damaged)?));
        } else {
            out.push(be64(&raw, index * 8).ok_or_else(damaged)?);
        }
    }
    Ok(out)
}

/// (offset, length) of every chunk of a track.
fn chunk_ranges(
    sizes: &Sizes,
    entries: &[(u32, u32)],
    offsets: &[u64],
) -> Result<Vec<(u64, u64)>, String> {
    let mut out = Vec::new();
    if offsets.is_empty() {
        return Ok(out);
    }
    if entries.is_empty() {
        return Err(unusual());
    }
    let mut entry = 0usize;
    let mut sample = 0u64;
    for (index, offset) in offsets.iter().enumerate() {
        let chunk_no = index as u64 + 1;
        while let Some(next) = entries.get(entry + 1) {
            if u64::from(next.0) <= chunk_no {
                entry += 1;
            } else {
                break;
            }
        }
        let current = entries.get(entry).ok_or_else(unusual)?;
        if u64::from(current.0) > chunk_no {
            return Err(unusual());
        }
        let count = u64::from(current.1);
        let length = match sizes {
            Sizes::Uniform { size, total } => {
                if sample.saturating_add(count) > *total {
                    return Err(unusual());
                }
                size.saturating_mul(count)
            }
            Sizes::Table(table) => {
                let from = usize::try_from(sample).map_err(|_| unusual())?;
                let to = usize::try_from(sample.saturating_add(count)).map_err(|_| unusual())?;
                let part = table.get(from..to).ok_or_else(unusual)?;
                part.iter().map(|value| u64::from(*value)).sum::<u64>()
            }
        };
        sample = sample.saturating_add(count);
        if length > 0 {
            out.push((*offset, length));
        }
    }
    Ok(out)
}

/// Where the samples of a track are stored in the file. Empty when the
/// track has no sample tables.
fn sample_ranges<R: Read + Seek>(
    src: &mut R,
    budget: &mut u32,
    trak: &BoxInfo,
) -> Result<Vec<(u64, u64)>, String> {
    let Some(mdia) = find_child(src, budget, trak, b"mdia")? else {
        return Ok(Vec::new());
    };
    let Some(minf) = find_child(src, budget, &mdia, b"minf")? else {
        return Ok(Vec::new());
    };
    let Some(stbl) = find_child(src, budget, &minf, b"stbl")? else {
        return Ok(Vec::new());
    };
    let kids = children(src, budget, stbl.payload(), stbl.end)?;
    let get = |name: &[u8; 4]| kids.iter().find(|found| &found.kind == name).copied();
    let (Some(stsz), Some(stsc)) = (get(b"stsz"), get(b"stsc")) else {
        return Ok(Vec::new());
    };
    let offsets = if let Some(stco) = get(b"stco") {
        read_offsets(src, &stco, 4)?
    } else if let Some(co64) = get(b"co64") {
        read_offsets(src, &co64, 8)?
    } else {
        return Ok(Vec::new());
    };
    let sizes = read_sizes(src, &stsz)?;
    let entries = read_stsc(src, &stsc)?;
    chunk_ranges(&sizes, &entries, &offsets)
}

/// The names in a "keys" box (lower case).
fn key_names(data: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let Some(count) = be32(data, 4) else {
        return out;
    };
    let mut at = 8usize;
    for _ in 0..count.min(MAX_KEYS) {
        let Some(size) = be32(data, at) else {
            break;
        };
        let size = size as usize;
        if size < 8 {
            break;
        }
        let Some(end) = at.checked_add(size) else {
            break;
        };
        let Some(name) = data.get(at + 8..end) else {
            break;
        };
        out.push(String::from_utf8_lossy(name).to_lowercase());
        at = end;
    }
    out
}

fn key_finding(name: &str) -> Option<Finding> {
    if name.contains("location") {
        Some(Finding::new(Category::Location, "GPS location"))
    } else if name.contains("lens") {
        Some(Finding::new(Category::Camera, "Lens information"))
    } else if name.contains("make") || name.contains("model") {
        Some(Finding::new(Category::Camera, "Camera make and model"))
    } else if name.contains("software") {
        Some(Finding::new(Category::Software, "Software"))
    } else if name.contains("creationdate") {
        Some(Finding::new(Category::Date, "Creation date"))
    } else if name.contains("android.version") {
        Some(Finding::new(Category::Software, "Android version"))
    } else {
        None
    }
}

struct Ctx {
    analysis: Analysis,
    patches: Patches,
    mdats: Vec<(u64, u64)>,
    budget: u32,
    fragmented: bool,
    times_found: bool,
    tracks: u32,
    track_bytes: u64,
}

impl Ctx {
    fn inside_mdat(&self, start: u64, end: u64) -> bool {
        self.mdats
            .iter()
            .any(|(from, to)| start >= *from && end <= *to)
    }

    /// Sets the creation and modification times of a mvhd, tkhd or mdhd box
    /// to zero (only when they are not zero already).
    fn zero_times<R: Read + Seek>(&mut self, src: &mut R, b: &BoxInfo) -> Result<(), String> {
        let p = b.payload();
        if b.end < p.saturating_add(4) {
            return Ok(());
        }
        let mut version = [0u8; 1];
        read_at(src, p, &mut version)?;
        let width: u64 = if version[0] == 1 { 16 } else { 8 };
        let start = p + 4;
        let end = start + width;
        if end > b.end {
            return Ok(());
        }
        let bytes = read_vec(src, start, width)?;
        if bytes.iter().any(|value| *value != 0) {
            self.patches.zero(start, end);
            self.times_found = true;
        }
        Ok(())
    }

    fn describe_udta<R: Read + Seek>(&mut self, src: &mut R, b: &BoxInfo) {
        let kids = children(src, &mut self.budget, b.payload(), b.end).unwrap_or_default();
        for kid in &kids {
            let finding = match &kid.kind {
                [0xA9, b'x', b'y', b'z'] => Some(Finding::new(Category::Location, "GPS location")),
                [0xA9, b'm', b'a', b'k'] | [0xA9, b'm', b'o', b'd'] => {
                    Some(Finding::new(Category::Camera, "Camera make and model"))
                }
                [0xA9, b's', b'w', b'r'] => Some(Finding::new(Category::Software, "Software")),
                [0xA9, b'd', b'a', b'y'] => Some(Finding::new(Category::Date, "Creation date")),
                b"auth" => Some(Finding::new(Category::Author, "Author")),
                _ => None,
            };
            if let Some(finding) = finding {
                self.analysis.add_finding(finding);
            }
        }
        self.analysis
            .add_finding(Finding::new(Category::Other, "User data block"));
    }

    fn describe_meta<R: Read + Seek>(&mut self, src: &mut R, b: &BoxInfo) {
        let mut inner = b.payload();
        if b.end.saturating_sub(inner) >= 8 {
            let mut probe = [0u8; 8];
            // QuickTime puts the first child right at the start; the ISO
            // form has 4 bytes of version and flags in front of it.
            if read_at(src, inner, &mut probe).is_ok() && probe.get(4..8) != Some(&b"hdlr"[..]) {
                inner = inner.saturating_add(4).min(b.end);
            }
        } else {
            inner = b.end;
        }
        let kids = children(src, &mut self.budget, inner, b.end).unwrap_or_default();
        if let Some(keys) = kids.iter().find(|found| &found.kind == b"keys") {
            let want = keys.end.saturating_sub(keys.payload()).min(MAX_KEYS_BYTES);
            if let Ok(data) = read_vec(src, keys.payload(), want) {
                for name in key_names(&data) {
                    if let Some(finding) = key_finding(&name) {
                        self.analysis.add_finding(finding);
                    }
                }
            }
        }
        self.analysis
            .add_finding(Finding::new(Category::Other, "Metadata block"));
    }

    /// Plans the replacement of a metadata box by a free box and records
    /// what it held. Empty boxes are left alone.
    fn remove_box<R: Read + Seek>(&mut self, src: &mut R, b: &BoxInfo) -> Result<(), String> {
        if !b.has_payload() {
            return Ok(());
        }
        let head = free_head(b.header_len, b.size()).ok_or_else(unusual)?;
        match &b.kind {
            b"udta" => self.describe_udta(src, b),
            b"meta" => self.describe_meta(src, b),
            b"uuid" => {
                let mut id = [0u8; 16];
                let is_xmp = b.size() >= b.header_len + 16
                    && read_at(src, b.payload(), &mut id).is_ok()
                    && id == XMP_UUID;
                if is_xmp {
                    self.analysis
                        .add_finding(Finding::new(Category::Xmp, "XMP metadata"));
                } else {
                    self.analysis
                        .add_finding(Finding::new(Category::Other, "Extra data block (uuid)"));
                }
            }
            b"XMP_" => self
                .analysis
                .add_finding(Finding::new(Category::Xmp, "XMP metadata")),
            _ => self
                .analysis
                .add_finding(Finding::new(Category::Thumbnail, "Preview picture")),
        }
        self.patches.replace(b.start, b.end, head);
        Ok(())
    }

    /// The handler type (for example "vide", "soun", "meta") of a track.
    fn handler_of<R: Read + Seek>(
        &mut self,
        src: &mut R,
        mdia: &BoxInfo,
    ) -> Result<Option<[u8; 4]>, String> {
        let Some(hdlr) = find_child(src, &mut self.budget, mdia, b"hdlr")? else {
            return Ok(None);
        };
        if hdlr.size() < hdlr.header_len + 12 {
            return Ok(None);
        }
        let mut kind = [0u8; 4];
        read_at(src, hdlr.payload() + 8, &mut kind)?;
        Ok(Some(kind))
    }

    /// Replaces a timed-metadata track by a free box and zeroes its samples.
    fn remove_track<R: Read + Seek>(&mut self, src: &mut R, trak: &BoxInfo) -> Result<(), String> {
        let head = free_head(trak.header_len, trak.size()).ok_or_else(unusual)?;
        let ranges = sample_ranges(src, &mut self.budget, trak)?;
        for (start, length) in ranges {
            let end = start.checked_add(length).ok_or_else(unusual)?;
            if !self.inside_mdat(start, end) {
                return Err(unusual());
            }
            self.track_bytes = self.track_bytes.saturating_add(length);
            self.patches.zero(start, end);
        }
        self.patches.replace(trak.start, trak.end, head);
        self.tracks += 1;
        Ok(())
    }

    fn process_trak<R: Read + Seek>(&mut self, src: &mut R, trak: &BoxInfo) -> Result<(), String> {
        let kids = children(src, &mut self.budget, trak.payload(), trak.end)?;
        let mdia = kids.iter().find(|found| &found.kind == b"mdia").copied();
        let handler = match &mdia {
            Some(found) => self.handler_of(src, found)?,
            None => None,
        };
        if handler == Some(*b"meta") {
            if !self.fragmented {
                return self.remove_track(src, trak);
            }
            self.analysis
                .add_kept("Fragmented video: tracks were left in place");
        }
        for kid in &kids {
            match &kid.kind {
                b"tkhd" => self.zero_times(src, kid)?,
                b"udta" | b"meta" | b"uuid" => self.remove_box(src, kid)?,
                _ => {}
            }
        }
        if let Some(found) = mdia {
            let media = children(src, &mut self.budget, found.payload(), found.end)?;
            for kid in &media {
                if &kid.kind == b"mdhd" {
                    self.zero_times(src, kid)?;
                }
            }
        }
        Ok(())
    }

    fn process_moov<R: Read + Seek>(&mut self, src: &mut R, moov: &BoxInfo) -> Result<(), String> {
        let kids = children(src, &mut self.budget, moov.payload(), moov.end)?;
        if kids.iter().any(|found| &found.kind == b"mvex") {
            self.fragmented = true;
        }
        for kid in &kids {
            match &kid.kind {
                b"mvhd" => self.zero_times(src, kid)?,
                b"udta" | b"meta" | b"uuid" => self.remove_box(src, kid)?,
                b"trak" => self.process_trak(src, kid)?,
                _ => {}
            }
        }
        Ok(())
    }
}

/// Analyzes an MP4 or MOV file of `len` bytes.
pub fn analyze<R: Read + Seek>(
    src: &mut R,
    len: u64,
    format: FileFormat,
    _options: &CleanOptions,
) -> Result<Analysis, String> {
    let mut ctx = Ctx {
        analysis: Analysis::new(format, len),
        patches: Patches::new(),
        mdats: Vec::new(),
        budget: 0,
        fragmented: false,
        times_found: false,
        tracks: 0,
        track_bytes: 0,
    };
    let top = children(src, &mut ctx.budget, 0, len)?;
    if top.is_empty() {
        return Err("not a valid video file".to_string());
    }
    for b in &top {
        match &b.kind {
            b"mdat" => ctx.mdats.push((b.payload(), b.end)),
            b"moof" => ctx.fragmented = true,
            _ => {}
        }
    }
    if !top.iter().any(|b| &b.kind == b"moov") {
        return Err("no video information found in the file".to_string());
    }
    for b in &top {
        match &b.kind {
            b"moov" => ctx.process_moov(src, b)?,
            b"meta" | b"uuid" | b"udta" | b"XMP_" | b"thum" | b"PICT" | b"pnot" => {
                ctx.remove_box(src, b)?
            }
            _ => {}
        }
    }
    if ctx.times_found {
        ctx.analysis.add_finding(Finding::new(
            Category::Date,
            "Creation and modification times",
        ));
    }
    if ctx.tracks > 0 {
        let detail = format!(
            "{} track{}, {} bytes",
            ctx.tracks,
            if ctx.tracks == 1 { "" } else { "s" },
            ctx.track_bytes
        );
        ctx.analysis.add_finding(
            Finding::new(Category::Other, "Timed metadata tracks").with_detail(&detail),
        );
    }
    ctx.analysis.add_kept("Video and audio data");
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
    use std::io::Cursor;

    fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(payload);
        out
    }

    /// A box with the 16-byte header (64-bit size).
    fn bx_large(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0, 0, 0, 1];
        out.extend_from_slice(kind);
        out.extend_from_slice(&((payload.len() + 16) as u64).to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
        parts.concat()
    }

    /// A header box (mvhd, tkhd, mdhd) with the creation and modification
    /// time set to `times`.
    fn times_box(kind: &[u8; 4], payload_len: usize, times: u32) -> Vec<u8> {
        let mut payload = vec![0u8; payload_len];
        payload[4..8].copy_from_slice(&times.to_be_bytes());
        payload[8..12].copy_from_slice(&times.to_be_bytes());
        bx(kind, &payload)
    }

    fn mvhd(times: u32) -> Vec<u8> {
        times_box(b"mvhd", 100, times)
    }

    fn tkhd(times: u32) -> Vec<u8> {
        times_box(b"tkhd", 84, times)
    }

    fn mdhd(times: u32) -> Vec<u8> {
        times_box(b"mdhd", 24, times)
    }

    fn hdlr(handler: &[u8; 4]) -> Vec<u8> {
        let mut payload = vec![0u8; 26];
        payload[8..12].copy_from_slice(handler);
        bx(b"hdlr", &payload)
    }

    fn stsz_uniform(size: u32, count: u32) -> Vec<u8> {
        let mut payload = vec![0u8; 4];
        payload.extend_from_slice(&size.to_be_bytes());
        payload.extend_from_slice(&count.to_be_bytes());
        bx(b"stsz", &payload)
    }

    fn stsz_table(sizes: &[u32]) -> Vec<u8> {
        let mut payload = vec![0u8; 4];
        payload.extend_from_slice(&0u32.to_be_bytes());
        payload.extend_from_slice(&(sizes.len() as u32).to_be_bytes());
        for size in sizes {
            payload.extend_from_slice(&size.to_be_bytes());
        }
        bx(b"stsz", &payload)
    }

    fn stsc(entries: &[(u32, u32)]) -> Vec<u8> {
        let mut payload = vec![0u8; 4];
        payload.extend_from_slice(&(entries.len() as u32).to_be_bytes());
        for (first, per_chunk) in entries {
            payload.extend_from_slice(&first.to_be_bytes());
            payload.extend_from_slice(&per_chunk.to_be_bytes());
            payload.extend_from_slice(&1u32.to_be_bytes());
        }
        bx(b"stsc", &payload)
    }

    fn stco(offsets: &[u32]) -> Vec<u8> {
        let mut payload = vec![0u8; 4];
        payload.extend_from_slice(&(offsets.len() as u32).to_be_bytes());
        for offset in offsets {
            payload.extend_from_slice(&offset.to_be_bytes());
        }
        bx(b"stco", &payload)
    }

    fn stbl(sizes: Vec<u8>, map: Vec<u8>, offsets: Vec<u8>) -> Vec<u8> {
        bx(b"stbl", &cat(&[sizes, map, offsets]))
    }

    /// One chunk with one sample of `size` bytes at `offset`.
    fn simple_stbl(offset: u32, size: u32) -> Vec<u8> {
        stbl(stsz_uniform(size, 1), stsc(&[(1, 1)]), stco(&[offset]))
    }

    fn trak(handler: &[u8; 4], times: u32, extra: &[Vec<u8>], stbl_box: Vec<u8>) -> Vec<u8> {
        let mdia = bx(
            b"mdia",
            &cat(&[mdhd(times), hdlr(handler), bx(b"minf", &stbl_box)]),
        );
        let mut parts = vec![tkhd(times), mdia];
        parts.extend_from_slice(extra);
        bx(b"trak", &cat(&parts))
    }

    /// A QuickTime style meta box (no version and flags) with a keys box.
    fn qt_meta(keys: &[&str]) -> Vec<u8> {
        let mut handler_payload = vec![0u8; 24];
        handler_payload[8..12].copy_from_slice(b"mdta");
        let mut key_payload = vec![0u8; 4];
        key_payload.extend_from_slice(&(keys.len() as u32).to_be_bytes());
        for key in keys {
            key_payload.extend_from_slice(&((key.len() + 8) as u32).to_be_bytes());
            key_payload.extend_from_slice(b"mdta");
            key_payload.extend_from_slice(key.as_bytes());
        }
        bx(
            b"meta",
            &cat(&[
                bx(b"hdlr", &handler_payload),
                bx(b"keys", &key_payload),
                bx(b"ilst", b"SECRET-VALUES"),
            ]),
        )
    }

    /// ftyp (20 bytes), mdat (108 bytes: its data are the bytes 28 to 127 of
    /// the file), moov, then the boxes in `after`.
    fn video(moov_kids: &[Vec<u8>], after: &[Vec<u8>]) -> Vec<u8> {
        let mut out = bx(b"ftyp", b"isom\0\0\0\0isom");
        let data: Vec<u8> = (0..100).map(|i| (i % 250 + 1) as u8).collect();
        out.extend(bx(b"mdat", &data));
        out.extend(bx(b"moov", &cat(moov_kids)));
        for part in after {
            out.extend_from_slice(part);
        }
        out
    }

    fn run_as(data: &[u8], format: FileFormat) -> (Analysis, Vec<u8>) {
        let analysis = analyze(
            &mut Cursor::new(data.to_vec()),
            data.len() as u64,
            format,
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

    fn run(data: &[u8]) -> (Analysis, Vec<u8>) {
        run_as(data, FileFormat::Mp4)
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

    fn assert_clean(data: &[u8]) {
        let (again, again_out) = run(data);
        assert!(again.findings.is_empty(), "{:?}", labels(&again));
        assert!(!again.has_changes());
        assert_eq!(again_out, data);
    }

    #[test]
    fn a_clean_file_is_left_alone() {
        let data = video(&[mvhd(0), trak(b"vide", 0, &[], simple_stbl(28, 50))], &[]);
        let (analysis, out) = run(&data);
        assert!(analysis.findings.is_empty());
        assert!(!analysis.has_changes());
        assert!(analysis.kept.contains(&"Video and audio data".to_string()));
        assert_eq!(out, data);
    }

    #[test]
    fn creation_and_modification_times_are_zeroed() {
        let data = video(
            &[mvhd(1000), trak(b"vide", 2000, &[], simple_stbl(28, 50))],
            &[],
        );
        let expected = video(&[mvhd(0), trak(b"vide", 0, &[], simple_stbl(28, 50))], &[]);
        let (analysis, out) = run(&data);
        assert_eq!(out, expected);
        assert_eq!(labels(&analysis), vec!["Creation and modification times"]);
        assert_eq!(analysis.findings[0].category, Category::Date);
    }

    #[test]
    fn metadata_boxes_are_replaced_by_free_boxes() {
        let udta = bx(
            b"udta",
            &bx(&[0xA9, b'x', b'y', b'z'], b"+47.5+008.2/SECRET"),
        );
        let meta = qt_meta(&[
            "com.apple.quicktime.make",
            "com.apple.quicktime.location.ISO6709",
        ]);
        let mut uuid_payload = vec![7u8; 16];
        uuid_payload.extend_from_slice(b"SECRET-UUID");
        let data = video(&[mvhd(0), udta, meta], &[bx(b"uuid", &uuid_payload)]);
        let (analysis, out) = run(&data);
        assert_eq!(out.len(), data.len());
        assert!(!contains(&out, b"SECRET"));
        assert!(!contains(&out, b"com.apple"));
        assert_eq!(
            labels(&analysis),
            vec![
                "GPS location",
                "User data block",
                "Camera make and model",
                "Metadata block",
                "Extra data block (uuid)",
            ]
        );
        assert_clean(&out);
    }

    #[test]
    fn an_xmp_uuid_box_is_recognized() {
        let mut payload = XMP_UUID.to_vec();
        payload.extend_from_slice(b"<x:xmpmeta/>");
        let data = video(&[mvhd(0)], &[bx(b"uuid", &payload)]);
        let (analysis, out) = run(&data);
        assert_eq!(labels(&analysis), vec!["XMP metadata"]);
        assert!(!contains(&out, b"xmpmeta"));
        assert_clean(&out);
    }

    #[test]
    fn a_timed_metadata_track_is_removed_with_its_samples() {
        let data = video(
            &[
                mvhd(0),
                trak(b"vide", 0, &[], simple_stbl(28, 50)),
                trak(b"meta", 0, &[], simple_stbl(78, 30)),
            ],
            &[],
        );
        let (analysis, out) = run(&data);
        assert_eq!(out.len(), data.len());
        assert_eq!(labels(&analysis), vec!["Timed metadata tracks"]);
        assert_eq!(
            analysis.findings[0].detail.as_deref(),
            Some("1 track, 30 bytes")
        );
        for index in 28..128 {
            if (78..108).contains(&index) {
                assert_eq!(out[index], 0, "byte {index} should be zero");
            } else {
                assert_eq!(out[index], data[index], "byte {index} should be unchanged");
            }
        }
        assert!(!contains(&out, b"meta"));
        assert_clean(&out);
    }

    #[test]
    fn tracks_are_left_in_place_in_fragmented_files() {
        let data = video(
            &[mvhd(0), trak(b"meta", 0, &[], simple_stbl(78, 30))],
            &[bx(b"moof", &[])],
        );
        let (analysis, out) = run(&data);
        assert!(analysis.findings.is_empty());
        assert!(!analysis.has_changes());
        assert!(
            analysis
                .kept
                .contains(&"Fragmented video: tracks were left in place".to_string())
        );
        assert_eq!(out, data);
    }

    #[test]
    fn sample_tables_with_several_chunks_are_followed() {
        let table = stbl(
            stsz_table(&[1, 2, 3, 4, 5, 6, 7]),
            stsc(&[(1, 1), (2, 3)]),
            stco(&[30, 40, 60]),
        );
        let data = video(&[mvhd(0), trak(b"meta", 0, &[], table)], &[]);
        let (analysis, out) = run(&data);
        assert_eq!(
            analysis.findings[0].detail.as_deref(),
            Some("1 track, 28 bytes")
        );
        let zeroed = [(30usize, 31usize), (40, 49), (60, 78)];
        for index in 28..128 {
            if zeroed
                .iter()
                .any(|(from, to)| index >= *from && index < *to)
            {
                assert_eq!(out[index], 0, "byte {index} should be zero");
            } else {
                assert_eq!(out[index], data[index], "byte {index} should be unchanged");
            }
        }
    }

    #[test]
    fn samples_outside_mdat_are_refused() {
        let data = video(&[mvhd(0), trak(b"meta", 0, &[], simple_stbl(300, 30))], &[]);
        let error = analyze(
            &mut Cursor::new(data.clone()),
            data.len() as u64,
            FileFormat::Mp4,
            &CleanOptions::default(),
        )
        .unwrap_err();
        assert_eq!(error, "unusual video structure");
    }

    #[test]
    fn metadata_inside_a_kept_track_is_removed() {
        let lens = qt_meta(&["com.apple.quicktime.camera.lens_model"]);
        let data = video(
            &[mvhd(0), trak(b"vide", 0, &[lens], simple_stbl(28, 50))],
            &[],
        );
        let (analysis, out) = run_as(&data, FileFormat::Mov);
        assert_eq!(analysis.format, FileFormat::Mov);
        assert_eq!(
            labels(&analysis),
            vec!["Lens information", "Metadata block"]
        );
        assert!(!contains(&out, b"SECRET"));
        assert_clean(&out);
    }

    #[test]
    fn large_and_zero_size_boxes_are_replaced_with_the_same_header() {
        let mut zero_size = vec![0, 0, 0, 0];
        zero_size.extend_from_slice(b"udta");
        zero_size.extend_from_slice(b"SECRET-ZERO");
        let data = video(&[mvhd(0)], &[bx_large(b"udta", b"SECRET-LARGE"), zero_size]);
        let (analysis, out) = run(&data);
        assert_eq!(out.len(), data.len());
        assert!(!contains(&out, b"SECRET"));
        assert_eq!(labels(&analysis), vec!["User data block"]);
        assert_clean(&out);
    }

    fn damage_base() -> Vec<u8> {
        video(
            &[
                mvhd(1000),
                bx(b"udta", &bx(&[0xA9, b'x', b'y', b'z'], b"x")),
                qt_meta(&["com.apple.quicktime.make"]),
                trak(b"meta", 5, &[], simple_stbl(78, 30)),
                trak(b"vide", 6, &[], simple_stbl(28, 50)),
            ],
            &[],
        )
    }

    #[test]
    fn every_truncation_is_rejected() {
        let base = damage_base();
        let options = CleanOptions::default();
        for length in 0..=base.len() {
            let result = analyze(
                &mut Cursor::new(base[..length].to_vec()),
                length as u64,
                FileFormat::Mp4,
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
        let base = damage_base();
        let options = CleanOptions::default();
        let mut rng = TestRng::new(0x5DEE_CE66_D1CE_4E5B);
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
                FileFormat::Mp4,
                &options,
            ) {
                assert_plan_sane(&analysis);
            }
        }
    }

    /// Needs the sample files of the owner. Run by hand:
    /// cargo test live_real_samples -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_real_samples_are_cleaned() {
        for (name, format) in [("A.MOV", FileFormat::Mov), ("A.mp4", FileFormat::Mp4)] {
            let path = format!("C:/dev/erxtools-testdata/Mediatest/{name}");
            let data = std::fs::read(&path).expect("sample file");
            let (analysis, out) = run_as(&data, format);
            println!("{name}: {} findings", analysis.findings.len());
            for finding in &analysis.findings {
                println!(
                    "  {:?} | {} | {:?}",
                    finding.category, finding.label, finding.detail
                );
            }
            for note in &analysis.kept {
                println!("  kept: {note}");
            }
            assert!(analysis.has_changes(), "{name}: nothing was found");
            assert_eq!(out.len(), data.len(), "{name}: the size must not change");
            let again = analyze(
                &mut Cursor::new(out.clone()),
                out.len() as u64,
                format,
                &CleanOptions::default(),
            )
            .unwrap();
            assert!(
                !again.has_changes(),
                "{name}: a second run still changes things"
            );
            assert!(
                again.findings.is_empty(),
                "{name}: a second run still finds things"
            );
        }
    }
}
