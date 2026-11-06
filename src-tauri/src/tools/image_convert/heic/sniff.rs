//! Pure HEIF/HEIC content sniffing from the first bytes of a file (no I/O).
//!
//! HEIF files are ISO-BMFF: the file starts with an `ftyp` box that names a
//! major brand plus a list of compatible brands. Only the brand decides the
//! kind — the extension is never trusted.

/// What `sniff_heif` decided a file's start bytes are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeifKind {
    /// iPhone-style HEIC (major or compatible brand from the HEIC set).
    Heic,
    /// AVIF image.
    Avif,
    /// A HEIF container (`mif1`/`msf1` major brand) without a HEIC or AVIF
    /// image brand — a variant we do not convert.
    OtherHeif,
    /// Anything else (including MP4, JPEG, PNG, garbage, short input).
    NotHeif,
}

/// Brands that mark a HEIC-coded still image.
const HEIC_BRANDS: [&[u8; 4]; 8] = [
    b"heic", b"heix", b"heim", b"heis", b"hevc", b"hevx", b"hevm", b"hevs",
];

/// Brands that mark an AVIF image.
const AVIF_BRANDS: [&[u8; 4]; 2] = [b"avif", b"avis"];

/// Minimal ISO-BMFF `ftyp` box that can still name a kind: 8-byte box header
/// (or 16 with the 64-bit size form) plus major brand and minor version.
const MIN_BRANDABLE_LEN: usize = 16;

/// Classifies the first bytes of a file. Never panics on short or garbage
/// input — anything undecidable is `NotHeif`.
pub fn sniff_heif(head: &[u8]) -> HeifKind {
    if head.len() < MIN_BRANDABLE_LEN {
        return HeifKind::NotHeif;
    }
    if head[4..8] != *b"ftyp" {
        return HeifKind::NotHeif;
    }

    // Box size: bytes 0..4 big-endian. `1` means a 64-bit size follows at
    // 8..16; `0` means the box extends to the end of the data.
    let mut header_len = 8usize;
    let size32 = u32::from_be_bytes([head[0], head[1], head[2], head[3]]);
    let box_end = if size32 == 1 {
        header_len = 16;
        u64::from_be_bytes([
            head[8], head[9], head[10], head[11], head[12], head[13], head[14], head[15],
        ])
    } else if size32 == 0 {
        head.len() as u64
    } else {
        u64::from(size32)
    };

    // The box must at least fit its own major brand and minor version.
    let needed = header_len + 8;
    if box_end < needed as u64 || head.len() < needed {
        return HeifKind::NotHeif;
    }

    let major = &head[header_len..header_len + 4];
    // Compatible brands start right after the minor version and stop at the
    // box end or the end of the available bytes, whichever comes first. A
    // trailing partial entry is ignored.
    let brand_start = header_len + 8;
    let brand_end = (box_end as usize).min(head.len());
    let brands = &head[brand_start..brand_end.max(brand_start)];

    if brand_is(major, &AVIF_BRANDS) {
        return HeifKind::Avif;
    }
    if brand_is(major, &HEIC_BRANDS) {
        return HeifKind::Heic;
    }
    if major == b"mif1" || major == b"msf1" {
        if brands
            .as_chunks::<4>()
            .0
            .iter()
            .any(|b| brand_is(b, &HEIC_BRANDS))
        {
            return HeifKind::Heic;
        }
        if brands
            .as_chunks::<4>()
            .0
            .iter()
            .any(|b| brand_is(b, &AVIF_BRANDS))
        {
            return HeifKind::Avif;
        }
        return HeifKind::OtherHeif;
    }
    HeifKind::NotHeif
}

/// True when `brand` equals one of `set`.
fn brand_is(brand: &[u8], set: &[&[u8; 4]]) -> bool {
    set.iter().any(|known| *known == brand)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a 32-bit-size `ftyp` box: [size]["ftyp"][major][minor][brands…].
    fn ftyp_box(major: &[u8; 4], minor: &[u8; 4], brands: &[&[u8; 4]]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(major);
        body.extend_from_slice(minor);
        for brand in brands {
            body.extend_from_slice(*brand);
        }
        let mut out = ((8 + body.len()) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(b"ftyp");
        out.extend_from_slice(&body);
        out
    }

    /// Builds the 64-bit-size form: [size=1]["ftyp"][64-bit size][major]…
    fn ftyp_box64(major: &[u8; 4], minor: &[u8; 4], brands: &[&[u8; 4]]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(major);
        body.extend_from_slice(minor);
        for brand in brands {
            body.extend_from_slice(*brand);
        }
        let mut out = 1u32.to_be_bytes().to_vec();
        out.extend_from_slice(b"ftyp");
        out.extend_from_slice(&(16 + body.len() as u64).to_be_bytes());
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn detects_heic_major_brand() {
        let head = ftyp_box(b"heic", b"\0\0\0\0", &[b"mif1", b"heic"]);
        assert_eq!(sniff_heif(&head), HeifKind::Heic);
        let head = ftyp_box(b"heix", b"\0\0\0\0", &[]);
        assert_eq!(sniff_heif(&head), HeifKind::Heic);
    }

    #[test]
    fn detects_heic_via_mif1_compatible_brand() {
        let head = ftyp_box(b"mif1", b"\0\0\0\0", &[b"mif1", b"heic"]);
        assert_eq!(sniff_heif(&head), HeifKind::Heic);
        let head = ftyp_box(b"msf1", b"\0\0\0\0", &[b"hevc"]);
        assert_eq!(sniff_heif(&head), HeifKind::Heic);
    }

    #[test]
    fn detects_avif_major_and_compatible() {
        let head = ftyp_box(b"avif", b"\0\0\0\0", &[b"mif1"]);
        assert_eq!(sniff_heif(&head), HeifKind::Avif);
        let head = ftyp_box(b"mif1", b"\0\0\0\0", &[b"mif1", b"avif"]);
        assert_eq!(sniff_heif(&head), HeifKind::Avif);
        let head = ftyp_box(b"mif1", b"\0\0\0\0", &[b"avis"]);
        assert_eq!(sniff_heif(&head), HeifKind::Avif);
    }

    #[test]
    fn heic_brand_wins_over_avif_in_mif1() {
        let head = ftyp_box(b"mif1", b"\0\0\0\0", &[b"avif", b"heic"]);
        assert_eq!(sniff_heif(&head), HeifKind::Heic);
    }

    #[test]
    fn mif1_without_image_brand_is_other_heif() {
        let head = ftyp_box(b"mif1", b"\0\0\0\0", &[b"mif1"]);
        assert_eq!(sniff_heif(&head), HeifKind::OtherHeif);
        let head = ftyp_box(b"msf1", b"\0\0\0\0", &[]);
        assert_eq!(sniff_heif(&head), HeifKind::OtherHeif);
    }

    #[test]
    fn mp4_and_other_containers_are_not_heif() {
        let head = ftyp_box(b"mp42", b"\0\0\0\0", &[b"isom", b"mp41"]);
        assert_eq!(sniff_heif(&head), HeifKind::NotHeif);
        let head = ftyp_box(b"isom", b"\0\0\0\0", &[b"mif1", b"heic"]);
        assert_eq!(sniff_heif(&head), HeifKind::NotHeif);
        // `isom` is not mif1/msf1, so its compatible brands must not count.
        let head = ftyp_box(b"qt  ", b"\0\0\0\0", &[b"heic"]);
        assert_eq!(sniff_heif(&head), HeifKind::NotHeif);
    }

    #[test]
    fn other_image_formats_are_not_heif() {
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0];
        jpeg.extend([0u8; 20]);
        assert_eq!(sniff_heif(&jpeg), HeifKind::NotHeif);

        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend([0u8; 20]);
        assert_eq!(sniff_heif(&png), HeifKind::NotHeif);
    }

    #[test]
    fn short_and_garbage_input_is_not_heif() {
        assert_eq!(sniff_heif(&[]), HeifKind::NotHeif);
        let short = vec![0u8; 15];
        assert_eq!(sniff_heif(&short), HeifKind::NotHeif);
        let garbage = *b"this is not an image at all.....";
        assert_eq!(sniff_heif(&garbage), HeifKind::NotHeif);
    }

    #[test]
    fn reads_the_64bit_size_form() {
        let head = ftyp_box64(b"mif1", b"\0\0\0\0", &[b"heic"]);
        assert_eq!(sniff_heif(&head), HeifKind::Heic);
        let head = ftyp_box64(b"heic", b"\0\0\0\0", &[]);
        assert_eq!(sniff_heif(&head), HeifKind::Heic);
    }

    #[test]
    fn zero_size_extends_to_end_of_data() {
        let mut head = ftyp_box(b"mif1", b"\0\0\0\0", &[b"heic"]);
        head[0..4].copy_from_slice(&0u32.to_be_bytes());
        assert_eq!(sniff_heif(&head), HeifKind::Heic);
    }

    #[test]
    fn size_larger_than_the_data_is_tolerated() {
        let mut head = ftyp_box(b"mif1", b"\0\0\0\0", &[b"heic"]);
        // Box claims 4 GiB but only 20 bytes exist; brands stop at the end.
        head[0..4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(sniff_heif(&head), HeifKind::Heic);
    }

    #[test]
    fn box_smaller_than_its_own_header_is_not_heif() {
        let mut head = ftyp_box(b"mif1", b"\0\0\0\0", &[b"heic"]);
        head[0..4].copy_from_slice(&12u32.to_be_bytes()); // < header + 8
        assert_eq!(sniff_heif(&head), HeifKind::NotHeif);
        // 64-bit form claiming a too-small box.
        let mut head = ftyp_box64(b"mif1", b"\0\0\0\0", &[b"heic"]);
        head[8..16].copy_from_slice(&20u64.to_be_bytes()); // < 16 + 8
        assert_eq!(sniff_heif(&head), HeifKind::NotHeif);
    }

    #[test]
    fn truncated_compatible_brand_list_is_ignored() {
        // A trailing 2-byte partial brand is simply ignored.
        let mut head = ftyp_box(b"mif1", b"\0\0\0\0", &[b"heic"]);
        head.extend_from_slice(b"he");
        assert_eq!(sniff_heif(&head), HeifKind::Heic);
        // Only a partial brand: no complete HEIC brand anywhere.
        let mut head = ftyp_box(b"mif1", b"\0\0\0\0", &[]);
        head.extend_from_slice(b"he");
        assert_eq!(sniff_heif(&head), HeifKind::OtherHeif);
    }

    #[test]
    fn data_after_the_box_end_does_not_count() {
        let mut head = ftyp_box(b"mif1", b"\0\0\0\0", &[]);
        // "heic" appears after the box end, so it must not be seen.
        head.extend_from_slice(b"heic");
        let box_size = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
        assert_eq!(head.len(), box_size + 4);
        assert_eq!(sniff_heif(&head), HeifKind::OtherHeif);
    }
}
