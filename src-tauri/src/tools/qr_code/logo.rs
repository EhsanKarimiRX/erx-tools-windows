//! Safe logo loading for the QR center. Pure logic, unit tested.
//!
//! The logo never comes from a URL — only from a local file the user
//! picked. It is decoded with the same hardening as the Image Converter
//! (content-based format detection, file-size and pixel-count guards,
//! decoder allocation cap), downscaled into a square transparent canvas
//! and re-encoded as PNG. That PNG is the ONE logo representation the
//! frontend holds and every backend consumes.

use std::io::BufReader;
use std::path::Path;

use base64::Engine as _;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};

/// Hard limit on the logo file, so a huge file never reaches the decoder.
pub const MAX_LOGO_FILE_BYTES: u64 = 10 * 1024 * 1024; // 10 MB
/// Decompression-bomb guard, same as the Image Converter (80 MP).
pub const MAX_LOGO_PIXELS: u64 = 80_000_000;
/// The logo is downscaled to fit a square canvas of this side (in pixels).
pub const LOGO_CANVAS_EDGE: u32 = 512;

/// A loaded logo: downscaled PNG bytes (base64) plus display dimensions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogoData {
    /// Side of the square canvas the logo fits in.
    pub edge: u32,
    /// The PNG (already downscaled) as plain base64, no data-URL prefix.
    pub png_base64: String,
}

/// Loads and prepares a logo from a local file path.
pub fn load_from_path(path: &Path) -> Result<LogoData, String> {
    let meta = std::fs::metadata(path).map_err(|_| "Could not read the logo file.".to_string())?;
    if !meta.is_file() {
        return Err("That path is not a regular file.".to_string());
    }
    if meta.len() > MAX_LOGO_FILE_BYTES {
        return Err(format!(
            "The logo file is larger than {} MB.",
            MAX_LOGO_FILE_BYTES / (1024 * 1024)
        ));
    }

    let file =
        std::fs::File::open(path).map_err(|_| "Could not open the logo file.".to_string())?;
    let reader = ImageReader::new(BufReader::new(file))
        .with_guessed_format()
        .map_err(|_| "Could not read the logo file.".to_string())?;
    // Content-based format check: SVG (XML) and everything else unknown is
    // rejected before any decode attempt (v1 supports PNG/JPEG/WebP logos).
    let format = reader
        .format()
        .ok_or_else(|| "That file is not a recognized image (PNG, JPEG or WebP).".to_string())?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
    ) {
        return Err(format!(
            "{} is not supported for logos in v1. Use PNG, JPEG or WebP.",
            logo_format_label(format)
        ));
    }

    let rgba = decode_to_rgba(reader)?;
    let png = square_thumbnail_png(&rgba);
    let png_base64 = base64::engine::general_purpose::STANDARD.encode(&png);
    Ok(LogoData {
        edge: LOGO_CANVAS_EDGE,
        png_base64,
    })
}

/// Decodes with the size guard and allocation cap, then flattens to RGBA.
fn decode_to_rgba<R: std::io::BufRead + std::io::Seek>(
    reader: ImageReader<R>,
) -> Result<DynamicImage, String> {
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| "Could not decode the logo image.".to_string())?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_LOGO_PIXELS {
        return Err(format!(
            "The logo image has more than the {} MP limit.",
            MAX_LOGO_PIXELS / 1_000_000
        ));
    }
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_LOGO_PIXELS * 4 + 128 * 1024 * 1024);
    let _ = decoder.set_limits(limits);
    // Phone photos arrive rotated; apply the orientation like everywhere.
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder)
        .map_err(|_| "Could not decode the logo image.".to_string())?;
    image.apply_orientation(orientation);
    Ok(image)
}

/// Fits the image into a transparent square canvas (no cropping, aspect
/// preserved, centered) and encodes it as PNG. JPEG logos become
/// transparent-canvas PNGs with full opacity preserved.
fn square_thumbnail_png(image: &DynamicImage) -> Vec<u8> {
    let rgba = image.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    let long_side = width.max(height).max(1);
    let scale = f64::from(LOGO_CANVAS_EDGE) / f64::from(long_side);
    let target_w = ((f64::from(width) * scale).round() as u32).clamp(1, LOGO_CANVAS_EDGE);
    let target_h = ((f64::from(height) * scale).round() as u32).clamp(1, LOGO_CANVAS_EDGE);

    let scaled = image::imageops::thumbnail(&rgba, target_w, target_h);
    let mut canvas = image::RgbaImage::from_pixel(
        LOGO_CANVAS_EDGE,
        LOGO_CANVAS_EDGE,
        image::Rgba([0, 0, 0, 0]),
    );
    let x = (LOGO_CANVAS_EDGE - scaled.width()) / 2;
    let y = (LOGO_CANVAS_EDGE - scaled.height()) / 2;
    image::imageops::overlay(&mut canvas, &scaled, i64::from(x), i64::from(y));

    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(canvas)
        .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
        .expect("PNG encode of our own canvas always works");
    out
}

fn logo_format_label(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Gif => "GIF",
        ImageFormat::Tiff => "TIFF",
        ImageFormat::Bmp => "BMP",
        _ => "That image type",
    }
}

/// The full `data:` URL used in the SVG `<image>` element.
pub fn data_url(png_base64: &str) -> String {
    format!("data:image/png;base64,{png_base64}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("erx-qr-logo-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    fn write_png(path: &Path, width: u32, height: u32, color: [u8; 4]) {
        let img = image::RgbaImage::from_pixel(width, height, image::Rgba(color));
        img.save_with_format(path, ImageFormat::Png)
            .expect("test PNG encodes");
    }

    #[test]
    fn loads_png_and_downscales_to_square_canvas() {
        let dir = temp_dir();
        let path = dir.join("big.png");
        write_png(&path, 2048, 1024, [255, 0, 0, 255]);

        let logo = load_from_path(&path).expect("loads");
        assert_eq!(logo.edge, LOGO_CANVAS_EDGE);
        // Decodes back to a 512x512 PNG.
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&logo.png_base64)
            .expect("valid base64");
        let decoded = image::load_from_memory(&bytes)
            .expect("valid PNG")
            .to_rgba8();
        assert_eq!((decoded.width(), decoded.height()), (512, 512));
        // Center pixel kept the logo color.
        assert_eq!(decoded.get_pixel(256, 256).0, [255, 0, 0, 255]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn jpeg_logo_gets_transparent_canvas() {
        let dir = temp_dir();
        let path = dir.join("logo.jpg");
        let img = image::RgbImage::from_pixel(300, 100, image::Rgb([10, 200, 30]));
        image::DynamicImage::ImageRgb8(img)
            .save_with_format(&path, ImageFormat::Jpeg)
            .expect("test JPEG encodes");

        let logo = load_from_path(&path).expect("loads");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&logo.png_base64)
            .unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap().to_rgba8();
        // Centered band keeps its color; above it the canvas is transparent.
        assert_eq!(decoded.get_pixel(256, 256).0[1], 200);
        assert_eq!(decoded.get_pixel(256, 10).0, [0, 0, 0, 0]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_missing_folder_and_non_images() {
        assert!(load_from_path(Path::new("Z:/definitely/missing/logo.png")).is_err());

        let dir = temp_dir();
        assert!(load_from_path(&dir).is_err(), "folder must be rejected");

        let text = dir.join("text.png");
        std::fs::write(&text, b"this is not an image at all").unwrap();
        assert!(load_from_path(&text).is_err());
        let _ = std::fs::remove_file(&text);

        // A renamed file: content sniffing, not the extension, decides.
        let fake = dir.join("fake.jpg");
        std::fs::write(&fake, b"\x89PNG\r\n\x1a\n not really").unwrap();
        assert!(load_from_path(&fake).is_err());
        let _ = std::fs::remove_file(&fake);
    }

    #[test]
    fn rejects_oversized_file() {
        let dir = temp_dir();
        let path = dir.join("huge.png");
        // A >10 MB file fails before any decode attempt.
        let blob = vec![0u8; (MAX_LOGO_FILE_BYTES + 1) as usize];
        std::fs::write(&path, &blob).unwrap();
        let error = load_from_path(&path).unwrap_err();
        assert!(error.contains("10 MB"), "got: {error}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn data_url_shape() {
        assert_eq!(data_url("QUJD"), "data:image/png;base64,QUJD");
    }
}
