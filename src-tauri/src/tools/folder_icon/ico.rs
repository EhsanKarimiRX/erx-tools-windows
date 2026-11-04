//! Multi-size .ico building: one icon file containing all frame sizes.
//!
//! Container: the `image` crate's `IcoEncoder` (already a project
//! dependency; the new `ico` feature pulls no further crates — it is just
//! bmp+png, both already enabled). Frames:
//! - 16–128 px: 32bpp BITMAPINFOHEADER DIB (the classic BMP-in-ICO layout
//!   per Microsoft's icon format docs), built here from the rendered
//!   pixmap: bottom-up BGRA XOR data plus a 1bpp AND mask.
//! - 256 px: PNG-compressed (allowed by the format and the modern default;
//!   spec 4.1 "256 may be PNG-compressed").
//!
//! Read-back tests decode the result with `image`'s own independent ICO
//! decoder, so builder and verifier are separate implementations.

use image::ExtendedColorType;
use image::codecs::ico::{IcoEncoder, IcoFrame};
use tiny_skia::Pixmap;

use super::core::IconParams;
use super::raster::FRAME_SIZES;

/// Builds the complete multi-size .ico file for the given icon.
/// Unused until 4B-3 wires the apply command (same pattern as error.rs).
#[allow(dead_code)]
pub fn build_ico(params: &IconParams) -> Result<Vec<u8>, String> {
    let mut frames = Vec::with_capacity(FRAME_SIZES.len());
    for &size in FRAME_SIZES.iter() {
        let pixmap = super::raster::render_pixmap(params, size)?;
        let data = if size == 256 {
            pixmap
                .encode_png()
                .map_err(|_| "Failed to encode the icon image.".to_string())?
        } else {
            dib_from_pixmap(&pixmap)
        };
        let frame = IcoFrame::with_encoded(data, size, size, ExtendedColorType::Rgba8).map_err(
            |error| {
                eprintln!("ICO frame error: {error}");
                "The icon file could not be built.".to_string()
            },
        )?;
        frames.push(frame);
    }
    let mut out = Vec::new();
    IcoEncoder::new(&mut out)
        .encode_images(&frames)
        .map_err(|error| {
            eprintln!("ICO container error: {error}");
            "The icon file could not be built.".to_string()
        })?;
    Ok(out)
}

/// Encodes a pixmap as a 32bpp BMP-in-ICO DIB: BITMAPINFOHEADER, bottom-up
/// BGRA pixel rows (XOR data), then a 1bpp AND mask whose bits are set only
/// where the pixel is fully transparent (the alpha channel does the work;
/// the mask is the belt-and-braces some old renderers look at).
fn dib_from_pixmap(pixmap: &Pixmap) -> Vec<u8> {
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    // AND mask rows are padded to 32-bit boundaries.
    let and_row_bytes = w.div_ceil(32) * 4;

    let mut dib = Vec::with_capacity(40 + w * h * 4 + and_row_bytes * h);
    dib.extend_from_slice(&40_u32.to_le_bytes()); // biSize
    dib.extend_from_slice(&(w as u32).to_le_bytes()); // biWidth
    dib.extend_from_slice(&((h as u32) * 2).to_le_bytes()); // biHeight: XOR + AND
    dib.extend_from_slice(&1_u16.to_le_bytes()); // biPlanes
    dib.extend_from_slice(&32_u16.to_le_bytes()); // biBitCount
    dib.extend_from_slice(&0_u32.to_le_bytes()); // biCompression = BI_RGB
    dib.extend_from_slice(&((w * h * 4 + and_row_bytes * h) as u32).to_le_bytes());
    dib.extend_from_slice(&[0; 16]); // biX/Y size, biClrUsed, biClrImportant

    // XOR data: bottom-up rows of BGRA.
    for y in (0..h).rev() {
        for x in 0..w {
            let c = pixmap
                .pixel(x as u32, y as u32)
                .expect("pixel within bounds")
                .demultiply();
            dib.extend_from_slice(&[c.blue(), c.green(), c.red(), c.alpha()]);
        }
    }
    // AND mask: bottom-up, bit set (1) = transparent.
    for y in (0..h).rev() {
        let row_start = dib.len();
        dib.extend(std::iter::repeat_n(0_u8, and_row_bytes));
        for x in 0..w {
            let alpha = pixmap
                .pixel(x as u32, y as u32)
                .expect("pixel within bounds")
                .alpha();
            if alpha == 0 {
                dib[row_start + x / 8] |= 0x80 >> (x % 8);
            }
        }
    }
    dib
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::folder_icon::settings::PatternKind;

    const BASE: crate::tools::folder_icon::colors::Rgb = crate::tools::folder_icon::colors::Rgb {
        r: 0,
        g: 0x78,
        b: 0xd4,
    };
    const BACK: crate::tools::folder_icon::colors::Rgb = crate::tools::folder_icon::colors::Rgb {
        r: 0,
        g: 0x40,
        b: 0x80,
    };
    const BLACK: crate::tools::folder_icon::colors::Rgb =
        crate::tools::folder_icon::colors::Rgb { r: 0, g: 0, b: 0 };

    fn params() -> IconParams {
        IconParams {
            base_color: BASE,
            back_color: BACK,
            pattern: PatternKind::None,
            pattern_color: BLACK,
            pattern_size: 1,
            pattern_opacity: 1.0,
        }
    }

    /// Reads the ICO directory: (width, height, data_size, offset) triples.
    fn directory(ico: &[u8]) -> Vec<(u32, u32, u32, u32)> {
        let read_u16 = |at: usize| u16::from_le_bytes([ico[at], ico[at + 1]]);
        let read_u32 =
            |at: usize| u32::from_le_bytes([ico[at], ico[at + 1], ico[at + 2], ico[at + 3]]);
        assert_eq!(read_u16(0), 0, "reserved");
        assert_eq!(read_u16(2), 1, "type must be icon");
        let count = read_u16(4) as usize;
        (0..count)
            .map(|i| {
                let base = 6 + i * 16;
                // 0 in a direntry means 256 px.
                let width = if ico[base] == 0 {
                    256
                } else {
                    u32::from(ico[base])
                };
                let height = if ico[base + 1] == 0 {
                    256
                } else {
                    u32::from(ico[base + 1])
                };
                let size = read_u32(base + 8);
                let offset = read_u32(base + 12);
                (width, height, size, offset)
            })
            .collect()
    }

    #[test]
    fn ico_contains_all_frame_sizes_in_order() {
        let ico = build_ico(&params()).expect("builds");
        let entries = directory(&ico);
        let sizes: Vec<u32> = entries.iter().map(|e| e.0).collect();
        assert_eq!(sizes, vec![16, 24, 32, 48, 64, 128, 256]);
        assert!(entries.iter().all(|e| e.0 == e.1), "square entries");
        // Offsets and sizes must tile the file exactly, in order.
        let mut expected_offset = 6 + 16 * entries.len() as u32;
        for (width, _, size, offset) in &entries {
            assert_eq!(*offset, expected_offset, "offset for {width}");
            expected_offset += size;
        }
        assert_eq!(expected_offset as usize, ico.len());
    }

    /// The front-panel highlight alpha at canvas y (must mirror the
    /// rasterizer's gradient stops).
    fn highlight_alpha_at(y: f64) -> f64 {
        let front = super::super::geometry::FRONT;
        0.20 + (0.02 - 0.20) * ((y - front.y) / front.h)
    }

    #[test]
    fn image_crate_decodes_the_container_and_256_png_entry() {
        let ico = build_ico(&params()).expect("builds");
        let img = image::load_from_memory(&ico).expect("image crate must decode the ico");
        // best_entry = the largest frame.
        assert_eq!((img.width(), img.height()), (256, 256));
        let rgba = img.to_rgba8();
        // Front center: base color under the highlight fade.
        let center = rgba.get_pixel(128, 160);
        assert!(center[3] == 255);
        let alpha = highlight_alpha_at(160.0);
        let expected_green = (f64::from(0x78) * (1.0 - alpha) + 255.0 * alpha).round() as i16;
        assert!(
            (i16::from(center[1]) - expected_green).abs() <= 2,
            "green {center:?} vs {expected_green}"
        );
        // Corner transparent.
        assert_eq!(rgba.get_pixel(2, 2)[3], 0);
    }

    #[test]
    fn image_crate_decodes_a_bmp_only_ico_independently() {
        // An ICO with ONLY a BMP frame, decoded through image's own ICO+BMP
        // decoder — an independent check of our DIB bytes.
        let pixmap = super::super::raster::render_pixmap(&params(), 32).expect("renders");
        let dib = dib_from_pixmap(&pixmap);
        let frame =
            IcoFrame::with_encoded(dib, 32, 32, ExtendedColorType::Rgba8).expect("frame builds");
        let mut out = Vec::new();
        IcoEncoder::new(&mut out)
            .encode_images(&[frame])
            .expect("ico writes");
        let img = image::load_from_memory(&out).expect("decodes");
        assert_eq!((img.width(), img.height()), (32, 32));
        let rgba = img.to_rgba8();
        // Front panel center (device 16, 17 ≈ canvas 136): base under fade.
        let center = rgba.get_pixel(16, 20);
        assert_eq!(center[3], 255);
        let alpha = highlight_alpha_at(160.0);
        let expected_green = (f64::from(0x78) * (1.0 - alpha) + 255.0 * alpha).round() as i16;
        assert!(
            (i16::from(center[1]) - expected_green).abs() <= 2,
            "green {center:?} vs {expected_green}"
        );
        // Corner transparent (AND mask + alpha both honor it).
        assert_eq!(rgba.get_pixel(0, 0)[3], 0);
    }

    #[test]
    fn dib_header_matches_the_documented_layout() {
        let pixmap = super::super::raster::render_pixmap(&params(), 16).expect("renders");
        let dib = dib_from_pixmap(&pixmap);
        let read_u32 =
            |at: usize| u32::from_le_bytes([dib[at], dib[at + 1], dib[at + 2], dib[at + 3]]);
        let read_u16 = |at: usize| u16::from_le_bytes([dib[at], dib[at + 1]]);
        assert_eq!(read_u32(0), 40, "biSize");
        assert_eq!(read_u32(4), 16, "biWidth");
        assert_eq!(read_u32(8), 32, "biHeight = 2 × height");
        assert_eq!(read_u16(12), 1, "biPlanes");
        assert_eq!(read_u16(14), 32, "biBitCount");
        assert_eq!(read_u32(16), 0, "biCompression = BI_RGB");
        // 16 px: XOR = 16*16*4 = 1024; AND row = 4 bytes → 64.
        assert_eq!(read_u32(20), 1024 + 64, "biSizeImage");
        assert_eq!(dib.len(), 40 + 1024 + 64);
    }
}
