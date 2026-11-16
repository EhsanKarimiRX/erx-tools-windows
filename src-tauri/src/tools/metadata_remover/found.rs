//! Turns the raw EXIF and XMP data that a handler found into readable
//! findings for the "found" list: GPS location, camera, serial numbers,
//! dates, software, authors, comments, thumbnail. EXIF is read with the
//! `kamadak-exif` crate (reading only); XMP is searched for a few well-known
//! names. Nothing in here changes a file or the plan.

use exif::{Context, In, Tag, Value};

use super::model::{Analysis, Category, Finding};

const MAX_DETAIL_CHARS: usize = 120;

fn clean(text: &str) -> String {
    let shown: String = text
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_DETAIL_CHARS)
        .collect();
    shown.trim().to_string()
}

fn push(analysis: &mut Analysis, category: Category, label: &str, detail: Option<String>) {
    let mut finding = Finding::new(category, label);
    if let Some(text) = detail {
        finding = finding.with_detail(&text);
    }
    analysis.add_finding(finding);
}

fn ascii_value(exif: &exif::Exif, tag: Tag) -> Option<String> {
    let field = exif.get_field(tag, In::PRIMARY)?;
    let Value::Ascii(parts) = &field.value else {
        return None;
    };
    let text = clean(&String::from_utf8_lossy(parts.first()?));
    if text.is_empty() { None } else { Some(text) }
}

fn has_field(exif: &exif::Exif, tag: Tag) -> bool {
    exif.get_field(tag, In::PRIMARY).is_some()
}

/// "2023:05:14 08:30:00" becomes "2023-05-14 08:30:00".
pub fn exif_date(text: &str) -> String {
    let mut out = String::new();
    for (index, c) in text.chars().enumerate() {
        if c == ':' && index < 10 {
            out.push('-');
        } else {
            out.push(c);
        }
    }
    out
}

/// A GPS coordinate in decimal degrees (degrees, minutes, seconds as three
/// rationals; the reference letter S or W makes it negative).
fn coordinate(exif: &exif::Exif, value_tag: Tag, ref_tag: Tag) -> Option<f64> {
    let field = exif.get_field(value_tag, In::PRIMARY)?;
    let Value::Rational(parts) = &field.value else {
        return None;
    };
    if parts.len() < 3 {
        return None;
    }
    let mut total = 0.0f64;
    let mut divisor = 1.0f64;
    for part in parts.iter().take(3) {
        if part.denom == 0 {
            return None;
        }
        total += f64::from(part.num) / f64::from(part.denom) / divisor;
        divisor *= 60.0;
    }
    let reference = ascii_value(exif, ref_tag).unwrap_or_default();
    if reference.starts_with('S') || reference.starts_with('W') {
        total = -total;
    }
    Some(total)
}

fn describe_exif(analysis: &mut Analysis, exif: &exif::Exif) {
    let latitude = coordinate(exif, Tag::GPSLatitude, Tag::GPSLatitudeRef);
    let longitude = coordinate(exif, Tag::GPSLongitude, Tag::GPSLongitudeRef);
    match (latitude, longitude) {
        (Some(lat), Some(lon)) if lat.abs() <= 90.0 && lon.abs() <= 180.0 => push(
            analysis,
            Category::Location,
            "GPS location",
            Some(format!("{lat:.5}, {lon:.5}")),
        ),
        _ => {
            if exif.fields().any(|f| f.tag.context() == Context::Gps) {
                push(analysis, Category::Location, "GPS data", None);
            }
        }
    }

    let make = ascii_value(exif, Tag::Make);
    let model = ascii_value(exif, Tag::Model);
    let camera = match (make, model) {
        (Some(make), Some(model)) => {
            if model.starts_with(&make) {
                Some(model)
            } else {
                Some(format!("{make} {model}"))
            }
        }
        (Some(one), None) | (None, Some(one)) => Some(one),
        (None, None) => None,
    };
    if camera.is_some() {
        push(analysis, Category::Camera, "Camera make and model", camera);
    }

    let items: [(Tag, Category, &str); 12] = [
        (Tag::LensModel, Category::Camera, "Lens"),
        (
            Tag::BodySerialNumber,
            Category::Camera,
            "Camera serial number",
        ),
        (
            Tag::LensSerialNumber,
            Category::Camera,
            "Lens serial number",
        ),
        (Tag::CameraOwnerName, Category::Author, "Camera owner name"),
        (Tag::DateTimeOriginal, Category::Date, "Date taken"),
        (Tag::DateTimeDigitized, Category::Date, "Date digitized"),
        (Tag::DateTime, Category::Date, "Date modified"),
        (Tag::Software, Category::Software, "Software"),
        (Tag::Artist, Category::Author, "Artist"),
        (Tag::Copyright, Category::Author, "Copyright"),
        (
            Tag::ImageDescription,
            Category::Comment,
            "Image description",
        ),
        (Tag::ImageUniqueID, Category::Other, "Unique image ID"),
    ];
    for (tag, category, label) in items {
        if let Some(text) = ascii_value(exif, tag) {
            let detail = if category == Category::Date {
                exif_date(&text)
            } else {
                text
            };
            push(analysis, category, label, Some(detail));
        }
    }

    if has_field(exif, Tag::UserComment) {
        push(analysis, Category::Comment, "User comment", None);
    }
    if has_field(exif, Tag::MakerNote) {
        push(
            analysis,
            Category::Other,
            "Maker notes (camera-specific data)",
            None,
        );
    }
    if exif.fields().any(|f| f.ifd_num == In::THUMBNAIL) {
        push(analysis, Category::Thumbnail, "Thumbnail picture", None);
    }
}

/// The text of an XMP name, either as an attribute (name="value") or as an
/// element (<name>value</name>, inner tags removed). None when it is not
/// there or empty.
pub fn xmp_value(text: &str, name: &str) -> Option<String> {
    let attribute = format!("{name}=\"");
    if let Some(index) = text.find(&attribute) {
        let rest = text.get(index + attribute.len()..)?;
        let end = rest.find('"')?;
        let value = clean(rest.get(..end)?);
        if !value.is_empty() {
            return Some(value);
        }
    }
    let open = format!("<{name}");
    let index = text.find(&open)?;
    let after = text.get(index + open.len()..)?;
    let first = after.chars().next()?;
    if !(first == '>' || first == '/' || first.is_whitespace()) {
        return None;
    }
    let content_start = after.find('>')? + 1;
    let inner = after.get(content_start..)?;
    let close = format!("</{name}>");
    let end = inner.find(&close)?;
    let mut words = String::new();
    let mut in_tag = false;
    for c in inner.get(..end)?.chars() {
        match c {
            '<' => {
                in_tag = true;
                words.push(' ');
            }
            '>' => in_tag = false,
            _ if !in_tag => words.push(c),
            _ => {}
        }
    }
    let value = clean(&words.split_whitespace().collect::<Vec<&str>>().join(" "));
    if value.is_empty() { None } else { Some(value) }
}

fn describe_xmp(analysis: &mut Analysis, text: &str) {
    let items: [(&str, Category, &str); 5] = [
        ("xmp:CreatorTool", Category::Software, "XMP: creator tool"),
        ("dc:creator", Category::Author, "XMP: author"),
        ("dc:rights", Category::Author, "XMP: rights"),
        ("xmp:CreateDate", Category::Date, "XMP: creation date"),
        ("exif:GPSLatitude", Category::Location, "XMP: GPS latitude"),
    ];
    for (name, category, label) in items {
        if let Some(value) = xmp_value(text, name) {
            push(analysis, category, label, Some(value));
        }
    }
}

/// Adds the readable findings from the raw EXIF and XMP of `analysis`.
/// Unreadable data is ignored: it is still removed by the plan.
pub fn enrich(analysis: &mut Analysis) {
    if let Some(tiff) = analysis.exif.clone()
        && let Ok(exif) = exif::Reader::new().read_raw(tiff)
    {
        describe_exif(analysis, &exif);
    }
    if let Some(xmp) = analysis.xmp.clone() {
        describe_xmp(analysis, &String::from_utf8_lossy(&xmp));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::metadata_remover::model::FileFormat;

    #[derive(Clone)]
    struct Entry {
        tag: u16,
        kind: u16,
        count: u32,
        data: Vec<u8>,
    }

    fn entry_ascii(tag: u16, text: &str) -> Entry {
        let mut data = text.as_bytes().to_vec();
        data.push(0);
        Entry {
            tag,
            kind: 2,
            count: data.len() as u32,
            data,
        }
    }

    fn entry_rational(tag: u16, values: &[(u32, u32)]) -> Entry {
        let mut data = Vec::new();
        for (num, denom) in values {
            data.extend_from_slice(&num.to_be_bytes());
            data.extend_from_slice(&denom.to_be_bytes());
        }
        Entry {
            tag,
            kind: 5,
            count: values.len() as u32,
            data,
        }
    }

    fn entry_long(tag: u16, value: u32) -> Entry {
        Entry {
            tag,
            kind: 4,
            count: 1,
            data: value.to_be_bytes().to_vec(),
        }
    }

    fn entry_short(tag: u16, value: u16) -> Entry {
        Entry {
            tag,
            kind: 3,
            count: 1,
            data: value.to_be_bytes().to_vec(),
        }
    }

    /// One IFD: entry count, the entries, the offset of the next IFD and the
    /// values that do not fit into 4 bytes. `start` is where the block will
    /// sit inside the TIFF data.
    fn ifd_block(entries: &[Entry], start: u32, next: u32) -> Vec<u8> {
        let table = 2 + 12 * entries.len() as u32 + 4;
        let mut out = Vec::new();
        out.extend_from_slice(&(entries.len() as u16).to_be_bytes());
        let mut extra: Vec<u8> = Vec::new();
        for entry in entries {
            out.extend_from_slice(&entry.tag.to_be_bytes());
            out.extend_from_slice(&entry.kind.to_be_bytes());
            out.extend_from_slice(&entry.count.to_be_bytes());
            if entry.data.len() <= 4 {
                let mut inline = entry.data.clone();
                inline.resize(4, 0);
                out.extend_from_slice(&inline);
            } else {
                out.extend_from_slice(&(start + table + extra.len() as u32).to_be_bytes());
                extra.extend_from_slice(&entry.data);
                if extra.len() % 2 == 1 {
                    extra.push(0);
                }
            }
        }
        out.extend_from_slice(&next.to_be_bytes());
        out.extend_from_slice(&extra);
        out
    }

    /// A big-endian TIFF block: IFD0, an optional GPS IFD (IFD0 gets the
    /// pointer to it) and an optional IFD1 (the thumbnail directory).
    fn tiff_with(ifd0: &[Entry], gps: &[Entry], ifd1: &[Entry]) -> Vec<u8> {
        let mut first: Vec<Entry> = ifd0.to_vec();
        if !gps.is_empty() {
            first.push(entry_long(0x8825, 0));
        }
        let first_len = ifd_block(&first, 8, 0).len() as u32;
        let gps_start = 8 + first_len;
        let gps_block = if gps.is_empty() {
            Vec::new()
        } else {
            ifd_block(gps, gps_start, 0)
        };
        let ifd1_start = gps_start + gps_block.len() as u32;
        if !gps.is_empty()
            && let Some(last) = first.last_mut()
        {
            last.data = gps_start.to_be_bytes().to_vec();
        }
        let next = if ifd1.is_empty() { 0 } else { ifd1_start };
        let mut out = b"MM\0\x2A".to_vec();
        out.extend_from_slice(&8u32.to_be_bytes());
        out.extend_from_slice(&ifd_block(&first, 8, next));
        out.extend_from_slice(&gps_block);
        if !ifd1.is_empty() {
            out.extend_from_slice(&ifd_block(ifd1, ifd1_start, 0));
        }
        out
    }

    fn gps_entries(lat_ref: &str, lat: [(u32, u32); 3], lon_ref: &str) -> Vec<Entry> {
        vec![
            entry_ascii(0x0001, lat_ref),
            entry_rational(0x0002, &lat),
            entry_ascii(0x0003, lon_ref),
            entry_rational(0x0004, &[(8, 1), (15, 1), (0, 1)]),
        ]
    }

    fn run(tiff: Vec<u8>) -> Vec<Finding> {
        let mut analysis = Analysis::new(FileFormat::Jpeg, 0);
        analysis.exif = Some(tiff);
        enrich(&mut analysis);
        analysis.findings
    }

    #[test]
    fn gps_camera_and_software_are_found() {
        let ifd0 = vec![
            entry_ascii(0x010F, "Canon"),
            entry_ascii(0x0110, "EOS 5D"),
            entry_ascii(0x0131, "Paint"),
        ];
        let gps = gps_entries("N", [(47, 1), (30, 1), (0, 1)], "E");
        let findings = run(tiff_with(&ifd0, &gps, &[]));
        assert_eq!(
            findings,
            vec![
                Finding::new(Category::Location, "GPS location").with_detail("47.50000, 8.25000"),
                Finding::new(Category::Camera, "Camera make and model").with_detail("Canon EOS 5D"),
                Finding::new(Category::Software, "Software").with_detail("Paint"),
            ]
        );
    }

    #[test]
    fn south_and_west_are_negative() {
        let gps = gps_entries("S", [(47, 1), (30, 1), (0, 1)], "W");
        let findings = run(tiff_with(&[], &gps, &[]));
        assert_eq!(
            findings,
            vec![
                Finding::new(Category::Location, "GPS location").with_detail("-47.50000, -8.25000")
            ]
        );
    }

    #[test]
    fn broken_gps_data_is_reported_without_coordinates() {
        let gps = gps_entries("N", [(47, 0), (30, 1), (0, 1)], "E");
        let findings = run(tiff_with(&[], &gps, &[]));
        assert_eq!(findings, vec![Finding::new(Category::Location, "GPS data")]);
    }

    #[test]
    fn thumbnail_is_reported() {
        let ifd0 = vec![entry_ascii(0x010F, "Canon")];
        let ifd1 = vec![entry_short(0x0103, 6)];
        let findings = run(tiff_with(&ifd0, &[], &ifd1));
        assert_eq!(
            findings,
            vec![
                Finding::new(Category::Camera, "Camera make and model").with_detail("Canon"),
                Finding::new(Category::Thumbnail, "Thumbnail picture"),
            ]
        );
    }

    #[test]
    fn garbage_exif_is_ignored() {
        assert!(run(b"garbage".to_vec()).is_empty());
        assert!(run(Vec::new()).is_empty());
    }

    #[test]
    fn xmp_attributes_and_elements_are_found() {
        let text = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF><rdf:Description xmp:CreatorTool=\"Adobe Photoshop\" xmp:CreateDate=\"2023-05-14T08:30:00\"><dc:creator><rdf:Seq><rdf:li>Ehsan</rdf:li></rdf:Seq></dc:creator></rdf:Description></rdf:RDF></x:xmpmeta>";
        let mut analysis = Analysis::new(FileFormat::Jpeg, 0);
        analysis.xmp = Some(text.as_bytes().to_vec());
        enrich(&mut analysis);
        assert_eq!(
            analysis.findings,
            vec![
                Finding::new(Category::Software, "XMP: creator tool")
                    .with_detail("Adobe Photoshop"),
                Finding::new(Category::Author, "XMP: author").with_detail("Ehsan"),
                Finding::new(Category::Date, "XMP: creation date")
                    .with_detail("2023-05-14T08:30:00"),
            ]
        );
    }

    #[test]
    fn xmp_values_are_cleaned_and_names_must_match_exactly() {
        let nested =
            "<dc:creator><rdf:Seq><rdf:li>A</rdf:li><rdf:li>B</rdf:li></rdf:Seq></dc:creator>";
        assert_eq!(xmp_value(nested, "dc:creator").as_deref(), Some("A B"));
        assert_eq!(
            xmp_value("<dc:creatorX>no</dc:creatorX>", "dc:creator"),
            None
        );
        assert_eq!(xmp_value("<other>x</other>", "dc:creator"), None);
        assert_eq!(xmp_value("<dc:creator></dc:creator>", "dc:creator"), None);
        assert_eq!(xmp_value("", "dc:creator"), None);
    }

    #[test]
    fn exif_dates_are_formatted() {
        assert_eq!(exif_date("2023:05:14 08:30:00"), "2023-05-14 08:30:00");
        assert_eq!(exif_date("garbage"), "garbage");
    }
}
