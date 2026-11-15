//! Reading and writing the EXIF Orientation tag (0x0112) without a full EXIF
//! parser. Used so that phone photos keep their rotation when all other
//! metadata is removed: a fresh minimal EXIF block that holds only this tag.

/// At most this many directory entries of the first IFD are looked at.
const MAX_ENTRIES: usize = 512;

fn u16_at(data: &[u8], offset: usize, little: bool) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let bytes: [u8; 2] = data.get(offset..end)?.try_into().ok()?;
    Some(if little {
        u16::from_le_bytes(bytes)
    } else {
        u16::from_be_bytes(bytes)
    })
}

fn u32_at(data: &[u8], offset: usize, little: bool) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let bytes: [u8; 4] = data.get(offset..end)?.try_into().ok()?;
    Some(if little {
        u32::from_le_bytes(bytes)
    } else {
        u32::from_be_bytes(bytes)
    })
}

/// Reads the Orientation value (1 to 8) from the first IFD of a TIFF block
/// (the structure that starts with "II" or "MM"). None when the block is
/// damaged, has no orientation, or the value is not 1 to 8.
pub fn read_orientation(tiff: &[u8]) -> Option<u16> {
    let little = match tiff.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    if u16_at(tiff, 2, little)? != 42 {
        return None;
    }
    let ifd = usize::try_from(u32_at(tiff, 4, little)?).ok()?;
    let count = usize::from(u16_at(tiff, ifd, little)?);
    let first = ifd.checked_add(2)?;
    for index in 0..count.min(MAX_ENTRIES) {
        let entry = first.checked_add(index.checked_mul(12)?)?;
        if u16_at(tiff, entry, little)? != 0x0112 {
            continue;
        }
        let kind = u16_at(tiff, entry.checked_add(2)?, little)?;
        let items = u32_at(tiff, entry.checked_add(4)?, little)?;
        if kind != 3 || items != 1 {
            return None;
        }
        let value = u16_at(tiff, entry.checked_add(8)?, little)?;
        return if (1..=8).contains(&value) {
            Some(value)
        } else {
            None
        };
    }
    None
}

/// A complete 26-byte TIFF block (big-endian) with one IFD that holds only
/// the Orientation tag.
pub fn minimal_tiff(orientation: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(26);
    out.extend_from_slice(b"MM\x00\x2A"); // byte order and the magic number 42
    out.extend_from_slice(&8u32.to_be_bytes()); // the IFD starts at offset 8
    out.extend_from_slice(&1u16.to_be_bytes()); // one entry
    out.extend_from_slice(&0x0112u16.to_be_bytes()); // tag: Orientation
    out.extend_from_slice(&3u16.to_be_bytes()); // type: SHORT
    out.extend_from_slice(&1u32.to_be_bytes()); // count: 1
    out.extend_from_slice(&orientation.to_be_bytes()); // the value
    out.extend_from_slice(&[0, 0]); // padding of the 4-byte value field
    out.extend_from_slice(&0u32.to_be_bytes()); // no next IFD
    out
}

/// A complete JPEG APP1 segment (36 bytes: marker, length, "Exif" header and
/// the minimal TIFF block) that holds only the Orientation tag.
pub fn jpeg_exif_segment(orientation: u16) -> Vec<u8> {
    let tiff = minimal_tiff(orientation);
    let length = (2 + 6 + tiff.len()) as u16;
    let mut out = vec![0xFF, 0xE1];
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(b"Exif\0\0");
    out.extend_from_slice(&tiff);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_blocks_have_the_expected_size_and_read_back() {
        for value in 1..=8u16 {
            let tiff = minimal_tiff(value);
            assert_eq!(tiff.len(), 26);
            assert_eq!(read_orientation(&tiff), Some(value));
        }
        let segment = jpeg_exif_segment(6);
        assert_eq!(segment.len(), 36);
        assert_eq!(&segment[0..4], &[0xFF, 0xE1, 0x00, 0x22]);
        assert_eq!(&segment[4..10], b"Exif\0\0");
        assert_eq!(read_orientation(&segment[10..]), Some(6));
    }

    #[test]
    fn little_endian_blocks_are_read() {
        let bytes: Vec<u8> = vec![
            b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0,
            0,
        ];
        assert_eq!(bytes.len(), 26);
        assert_eq!(read_orientation(&bytes), Some(6));
    }

    #[test]
    fn extra_bytes_after_the_block_are_ignored() {
        let mut tiff = minimal_tiff(3);
        tiff.extend_from_slice(b"JUNK-JUNK-JUNK");
        assert_eq!(read_orientation(&tiff), Some(3));
    }

    #[test]
    fn invalid_values_and_types_are_rejected() {
        assert_eq!(read_orientation(&minimal_tiff(0)), None);
        assert_eq!(read_orientation(&minimal_tiff(9)), None);
        assert_eq!(read_orientation(&minimal_tiff(65535)), None);

        let mut wrong_type = minimal_tiff(6);
        wrong_type[13] = 4; // type LONG instead of SHORT
        assert_eq!(read_orientation(&wrong_type), None);

        let mut wrong_count = minimal_tiff(6);
        wrong_count[17] = 2; // count 2 instead of 1
        assert_eq!(read_orientation(&wrong_count), None);

        let mut wrong_magic = minimal_tiff(6);
        wrong_magic[3] = 43;
        assert_eq!(read_orientation(&wrong_magic), None);

        assert_eq!(read_orientation(b"hello world, this is no tiff"), None);
        assert_eq!(read_orientation(&[]), None);
    }

    #[test]
    fn every_prefix_is_handled_without_panicking() {
        let tiff = minimal_tiff(6);
        for length in 0..20 {
            assert_eq!(read_orientation(&tiff[..length]), None, "prefix {length}");
        }
        assert_eq!(read_orientation(&tiff[..20]), Some(6));
    }

    #[test]
    fn hostile_offsets_and_counts_are_safe() {
        let mut far_away = minimal_tiff(6);
        far_away[4..8].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(read_orientation(&far_away), None);

        // A huge entry count whose first entry is not the orientation: the
        // walk runs off the end of the block and stops.
        let mut many = minimal_tiff(6);
        many[8] = 0xFF;
        many[9] = 0xFF;
        many[10] = 0x01;
        many[11] = 0x00;
        assert_eq!(read_orientation(&many), None);
    }
}
