//! Multi-size .ico writer for Favicons: every frame stored as PNG
//! inside one container. Spec: docs/PROMPT_11_SPEC.md sections 5 + 10.

use image::ExtendedColorType;
use image::RgbaImage;
use image::codecs::ico::{IcoEncoder, IcoFrame};

/// Builds one .ico file from square RGBA frames (1..=256 px per side).
/// Every frame is stored PNG-compressed, in the order given; the favicon
/// package passes 16/32/48.
pub fn build_ico(frames: &[RgbaImage]) -> Result<Vec<u8>, String> {
    let mut ico_frames = Vec::with_capacity(frames.len());
    for frame in frames {
        let (width, height) = (frame.width(), frame.height());
        if width != height || !(1..=256).contains(&width) {
            return Err("Invalid icon size.".into());
        }
        let frame = IcoFrame::as_png(frame.as_raw(), width, height, ExtendedColorType::Rgba8)
            .map_err(|error| {
                eprintln!("ICO frame error: {error}");
                "The icon file could not be built.".to_string()
            })?;
        ico_frames.push(frame);
    }
    let mut out = Vec::new();
    IcoEncoder::new(&mut out)
        .encode_images(&ico_frames)
        .map_err(|error| {
            eprintln!("ICO container error: {error}");
            "The icon file could not be built.".to_string()
        })?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(size: u32, rgba: [u8; 4]) -> RgbaImage {
        let mut img = RgbaImage::new(size, size);
        for p in img.pixels_mut() {
            *p = image::Rgba(rgba);
        }
        img
    }

    #[test]
    fn three_solid_frames_produce_a_valid_icondir() {
        let ico = build_ico(&[
            solid(16, [1, 0, 0, 255]),
            solid(32, [2, 0, 0, 255]),
            solid(48, [3, 0, 0, 255]),
        ])
        .expect("builds");
        // ICONDIR: reserved 0, type 1 (icon), count 3 — all little-endian.
        assert_eq!(&ico[0..2], &[0, 0]);
        assert_eq!(&ico[2..4], &[1, 0]);
        assert_eq!(&ico[4..6], &[3, 0]);
        // Directory entries start at offset 6, 16 bytes each; the first
        // entry byte is the width (1..=256 fits one byte).
        let entries = [&ico[6..22], &ico[22..38], &ico[38..54]];
        let widths: Vec<u8> = entries.iter().map(|e| e[0]).collect();
        assert_eq!(widths, vec![16, 32, 48]);
        for entry in &entries {
            assert_eq!(entry[1], entry[0], "height byte equals width");
        }
    }

    #[test]
    fn an_empty_slice_is_rejected_by_the_encoder() {
        // image's IcoEncoder requires 1..=u16::MAX images; our wrapper turns
        // that into the friendly message. Consistent behaviour, tested here.
        let result = build_ico(&[]);
        assert_eq!(result.unwrap_err(), "The icon file could not be built.");
    }

    #[test]
    fn an_oversized_frame_is_rejected() {
        let result = build_ico(&[solid(300, [0, 0, 0, 255])]);
        assert_eq!(result.unwrap_err(), "Invalid icon size.");
    }
}
