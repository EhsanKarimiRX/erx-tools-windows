//! Date gathering for the Organize Date mode: the photo's own camera date
//! (EXIF, via the pure-Rust `kamadak-exif` crate — offline, no spawning)
//! and the Windows created/modified dates converted to the user's LOCAL
//! time. Cloud-only OneDrive placeholders must NEVER reach the EXIF read —
//! the skip rules in `scan.rs` are evaluated first.

use std::io::BufReader;
use std::path::Path;

use super::modes::parse_exif_datetime;

/// Extensions worth an EXIF attempt (spec 5h-2). Any read failure simply
/// means "no photo date" — the file then goes to "Date not found".
const EXIF_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "tif", "tiff", "png", "webp", "heic", "heif", "avif",
];

pub fn exif_eligible(extension: &str) -> bool {
    EXIF_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
}

fn current_year() -> i32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| {
            let seconds = i64::try_from(duration.as_secs()).unwrap_or(0);
            super::modes::unix_utc_to_utc_ym(seconds).0
        })
        .unwrap_or(2026)
}

/// Reads the photo's own date from EXIF, in this order (spec 5h-2):
/// `DateTimeOriginal` (the shot time), then `DateTimeDigitized`. The plain
/// `DateTime` tag is deliberately NEVER used — it is the last-edit time.
/// The RAW ASCII value is parsed (no display formatting). Any error means
/// "no photo date".
pub fn photo_date_taken(path: &Path) -> Option<(i32, u8)> {
    let file = std::fs::File::open(path).ok()?;
    let mut buffered = BufReader::new(file);
    let exif = exif::Reader::new()
        .read_from_container(&mut buffered)
        .ok()?;

    let wanted = [exif::Tag::DateTimeOriginal, exif::Tag::DateTimeDigitized];
    for tag in wanted {
        for field in exif.fields() {
            if field.tag != tag {
                continue;
            }
            // The raw ASCII bytes (EXIF pads with NULs — the parser trims).
            let raw = match &field.value {
                exif::Value::Ascii(entries) => entries.first().map(|bytes| bytes.as_slice()),
                _ => None,
            };
            if let Some(bytes) = raw {
                let text = String::from_utf8_lossy(bytes);
                if let Some(date) = parse_exif_datetime(&text, current_year()) {
                    return Some(date);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use exif::experimental::Writer;
    use exif::{In, Tag, Value};
    use std::io::Cursor;

    /// Builds a minimal TIFF container carrying one ASCII EXIF tag, using
    /// the crate's own (experimental) writer — verified to exist in 0.6.2.
    fn tiff_with_tag(tag: Tag, text: &str) -> Vec<u8> {
        let mut value_bytes = text.as_bytes().to_vec();
        value_bytes.push(0); // EXIF ASCII is NUL-terminated
        let mut writer = Writer::new();
        let field = exif::Field {
            tag,
            ifd_num: In::PRIMARY,
            value: Value::Ascii(vec![value_bytes]),
        };
        writer.push_field(&field);
        let mut buffer = Cursor::new(Vec::new());
        writer.write(&mut buffer, true).expect("write fixture");
        buffer.into_inner()
    }

    #[test]
    fn exif_eligible_extensions() {
        assert!(exif_eligible("jpg"));
        assert!(exif_eligible("JPEG"));
        assert!(exif_eligible("heic"));
        assert!(!exif_eligible("txt"));
        assert!(!exif_eligible("mp4"));
    }

    #[test]
    fn photo_date_read_from_datetime_original() {
        let dir = std::env::temp_dir().join(format!("erx-exif-{}-orig", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("photo.jpg");
        std::fs::write(
            &path,
            tiff_with_tag(Tag::DateTimeOriginal, "2023:05:14 08:30:00"),
        )
        .expect("write fixture");

        assert_eq!(photo_date_taken(&path), Some((2023, 5)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn digitized_used_when_original_missing() {
        let dir = std::env::temp_dir().join(format!("erx-exif-{}-dig", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("photo.tif");
        std::fs::write(
            &path,
            tiff_with_tag(Tag::DateTimeDigitized, "2019:11:02 14:00:00"),
        )
        .expect("write fixture");

        assert_eq!(photo_date_taken(&path), Some((2019, 11)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn original_beats_digitized() {
        let dir = std::env::temp_dir().join(format!("erx-exif-{}-both", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("photo.jpg");
        // Both fields in one container: Original must win.
        let mut writer = Writer::new();
        let original = exif::Field {
            tag: Tag::DateTimeOriginal,
            ifd_num: In::PRIMARY,
            value: Value::Ascii(vec![b"2021:01:01 00:00:00\0".to_vec()]),
        };
        writer.push_field(&original);
        let digitized = exif::Field {
            tag: Tag::DateTimeDigitized,
            ifd_num: In::PRIMARY,
            value: Value::Ascii(vec![b"2010:01:01 00:00:00\0".to_vec()]),
        };
        writer.push_field(&digitized);
        let mut buffer = Cursor::new(Vec::new());
        writer.write(&mut buffer, true).expect("write");
        std::fs::write(&path, buffer.into_inner()).expect("write fixture");

        assert_eq!(photo_date_taken(&path), Some((2021, 1)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn plain_datetime_tag_is_never_used() {
        let dir = std::env::temp_dir().join(format!("erx-exif-{}-plain", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("photo.jpg");
        // ONLY the plain DateTime tag (the last-edit time) — must be ignored.
        std::fs::write(&path, tiff_with_tag(Tag::DateTime, "2001:01:01 00:00:00"))
            .expect("write fixture");

        assert_eq!(
            photo_date_taken(&path),
            None,
            "DateTime is not the shot time"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn garbage_or_missing_exif_means_no_date() {
        let dir = std::env::temp_dir().join(format!("erx-exif-{}-none", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");

        // A text file pretending to be a photo: unreadable container.
        let fake = dir.join("fake.jpg");
        std::fs::write(&fake, "this is not an image").expect("write");
        assert_eq!(photo_date_taken(&fake), None);

        // A real container but a broken date string.
        let broken = dir.join("broken.jpg");
        std::fs::write(&broken, tiff_with_tag(Tag::DateTimeOriginal, "not a date")).expect("write");
        assert_eq!(photo_date_taken(&broken), None);

        // A missing file.
        assert_eq!(photo_date_taken(&dir.join("nope.jpg")), None);

        std::fs::remove_dir_all(&dir).ok();
    }
}
