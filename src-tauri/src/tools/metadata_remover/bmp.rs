//! BMP handler: a BMP file has no metadata fields worth removing, so this
//! handler only checks that the file really is a BMP and plans an unchanged
//! copy. The tool then reports "This file has no metadata." and makes no
//! new file.

use std::io::{Read, Seek, SeekFrom};

use super::model::{Analysis, CleanOptions, FileFormat};

fn damaged() -> String {
    "damaged or incomplete BMP file".to_string()
}

/// Analyzes a BMP file of `len` bytes.
pub fn analyze<R: Read + Seek>(
    src: &mut R,
    len: u64,
    _options: &CleanOptions,
) -> Result<Analysis, String> {
    if len < 26 {
        return Err(damaged());
    }
    let mut head = [0u8; 18];
    src.seek(SeekFrom::Start(0)).map_err(|_| damaged())?;
    src.read_exact(&mut head).map_err(|_| damaged())?;
    if &head[0..2] != b"BM" {
        return Err("not a valid BMP file".to_string());
    }
    let header_size = u32::from_le_bytes([head[14], head[15], head[16], head[17]]);
    if !matches!(header_size, 12 | 40 | 52 | 56 | 64 | 108 | 124) {
        return Err("unusual BMP structure".to_string());
    }
    let mut analysis = Analysis::new(FileFormat::Bmp, len);
    analysis.plan.copy(0, len);
    Ok(analysis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::bmp::BmpEncoder;
    use image::{ExtendedColorType, ImageEncoder};
    use std::io::Cursor;

    fn real_bmp() -> Vec<u8> {
        let image = image::RgbImage::from_fn(4, 4, |x, y| image::Rgb([x as u8, y as u8, 9]));
        let mut out = Cursor::new(Vec::new());
        BmpEncoder::new(&mut out)
            .write_image(image.as_raw(), 4, 4, ExtendedColorType::Rgb8)
            .unwrap();
        out.into_inner()
    }

    fn check(bytes: Vec<u8>) -> Result<Analysis, String> {
        let len = bytes.len() as u64;
        analyze(&mut Cursor::new(bytes), len, &CleanOptions::default())
    }

    #[test]
    fn a_real_bmp_has_nothing_to_remove() {
        let data = real_bmp();
        let analysis = check(data.clone()).unwrap();
        assert!(analysis.findings.is_empty());
        assert!(analysis.kept.is_empty());
        assert!(!analysis.has_changes());
        assert_eq!(analysis.source_len, data.len() as u64);
        assert!(analysis.plan.is_identity(data.len() as u64));
    }

    #[test]
    fn other_files_are_rejected() {
        assert!(check(Vec::new()).is_err());
        assert!(check(b"hello world, this is no bmp at all".to_vec()).is_err());
        let mut wrong_signature = real_bmp();
        wrong_signature[0] = b'X';
        assert!(check(wrong_signature).is_err());
    }

    #[test]
    fn short_files_are_rejected_at_every_length() {
        let data = real_bmp();
        for length in 0..26 {
            assert!(check(data[..length].to_vec()).is_err(), "length {length}");
        }
    }

    #[test]
    fn an_unusual_header_size_is_rejected() {
        let mut data = real_bmp();
        data[14..18].copy_from_slice(&5000u32.to_le_bytes());
        assert!(check(data).is_err());
    }
}
