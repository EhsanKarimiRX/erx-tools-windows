//! Source loading for Favicons: content-based format detection, raster
//! decoding with EXIF orientation, SVG parsing/rendering and warnings.
//! Spec: docs/PROMPT_11_SPEC.md sections 5 + 10.

use std::io::Cursor;

use image::ImageDecoder;
use image::ImageFormat;
use image::RgbaImage;

/// Maximum accepted file size (64 MiB).
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Maximum width/height of a raster image.
pub const MAX_SIDE: u32 = 10_000;
/// Maximum pixel count of a raster image.
pub const MAX_PIXELS: u64 = 100_000_000;
/// The rendered SVG "master" is scaled so its longest side is this.
pub const SVG_MASTER_SIZE: u32 = 1024;

/// Only the first N bytes are scanned when sniffing for an SVG.
const SVG_SNIFF_BYTES: usize = 16_384;

/// What kind of source the loaded file was (decided by content, never by
/// extension).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Raster,
    Svg,
}

/// One loaded source image: the decoded/rendered master plus metadata.
#[derive(Debug)]
pub struct LoadedSource {
    pub kind: SourceKind,
    /// "PNG", "JPEG", "WebP", "BMP", "GIF", "TIFF", "ICO" or "SVG".
    pub format_label: String,
    /// For SVG: round(tree size).
    pub original_width: u32,
    pub original_height: u32,
    /// Raster: the decoded pixels. SVG: the rendered master (longest side
    /// 1024 px, transparent background).
    pub master: RgbaImage,
    /// The ORIGINAL file bytes, only for SVG sources (used verbatim for
    /// favicon.svg).
    pub svg_bytes: Option<Vec<u8>>,
    pub warnings: Vec<String>,
}

/// The file-size rule from the spec: not empty, at most 64 MiB.
pub fn check_file_size(len: u64) -> Result<(), String> {
    if len == 0 {
        return Err("The file is empty.".into());
    }
    if len > MAX_FILE_BYTES {
        return Err("The file is too large (maximum 64 MB).".into());
    }
    Ok(())
}

/// The dimension rule: width/height > 0, at most 10000 per side and at most
/// 100 megapixels. Called BEFORE any pixel memory is allocated.
pub fn check_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("The image has an invalid size (width or height is 0).".into());
    }
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err("The image is too large (maximum 10000 px per side).".into());
    }
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("The image is too large (maximum 100 megapixels).".into());
    }
    Ok(())
}

/// Loads a source from disk: metadata size check first, then the bytes.
pub fn load_source(path: &std::path::Path) -> Result<LoadedSource, String> {
    let metadata =
        std::fs::metadata(path).map_err(|_| "The file could not be read.".to_string())?;
    check_file_size(metadata.len())?;
    let bytes = std::fs::read(path).map_err(|_| "The file could not be read.".to_string())?;
    load_source_from_bytes(&bytes)
}

/// Detects and loads a source purely from its bytes (content, never the
/// extension). See spec section 10 for the detection order.
pub fn load_source_from_bytes(bytes: &[u8]) -> Result<LoadedSource, String> {
    check_file_size(bytes.len() as u64)?;
    match image::guess_format(bytes) {
        Ok(format) => match format {
            ImageFormat::Png
            | ImageFormat::Jpeg
            | ImageFormat::WebP
            | ImageFormat::Bmp
            | ImageFormat::Gif
            | ImageFormat::Tiff
            | ImageFormat::Ico => load_raster(bytes, format),
            other => Err(match unsupported_format_label(other) {
                "" => "This image format is not supported. Convert the image to PNG first.".into(),
                label => {
                    format!("{label} images are not supported. Convert the image to PNG first.")
                }
            }),
        },
        Err(_) => detect_unsupported_or_svg(bytes),
    }
}

/// The length of a leading UTF-8 BOM, if any.
fn bom_len(bytes: &[u8]) -> usize {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        3
    } else {
        0
    }
}

/// Known-but-unsupported files (friendly errors) and the SVG sniff.
fn detect_unsupported_or_svg(bytes: &[u8]) -> Result<LoadedSource, String> {
    if bytes.len() >= 12
        && bytes[4..8] == *b"ftyp"
        && matches!(
            &bytes[8..12],
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1"
        )
    {
        return Err("HEIC/HEIF images are not supported here. Convert the image to PNG first (the Image Converter tool can do this).".into());
    }
    if bytes.starts_with(b"%PDF") {
        return Err("PDF files are not supported. Export the logo as PNG or SVG.".into());
    }
    if bytes.starts_with(b"8BPS") {
        return Err(
            "Photoshop (PSD) files are not supported. Export the logo as PNG or SVG.".into(),
        );
    }
    if bytes.starts_with(&[0x1F, 0x8B]) {
        return Err("Compressed files are not supported. Unzip the SVG first.".into());
    }

    let sniff = &bytes[bom_len(bytes)..];
    let sniff = &sniff[..sniff.len().min(SVG_SNIFF_BYTES)];
    if String::from_utf8_lossy(sniff).contains("<svg") {
        return load_svg(bytes);
    }
    Err(
        "This file is not a supported image. Supported: PNG, JPEG, WebP, BMP, GIF, TIFF, ICO, SVG."
            .into(),
    )
}

/// Short display name for a format the `image` crate detects but this tool
/// does not accept. Empty for formats without a crisp name.
fn unsupported_format_label(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Avif => "AVIF",
        ImageFormat::Dds => "DDS",
        ImageFormat::Farbfeld => "Farbfeld",
        ImageFormat::Hdr => "HDR",
        ImageFormat::OpenExr => "OpenEXR",
        ImageFormat::Pnm => "PBM/PGM/PPM",
        ImageFormat::Qoi => "QOI",
        ImageFormat::Tga => "TGA",
        _ => "",
    }
}

/// Short display name for an accepted raster format.
fn format_label(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "PNG",
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::WebP => "WebP",
        ImageFormat::Bmp => "BMP",
        ImageFormat::Gif => "GIF",
        ImageFormat::Tiff => "TIFF",
        ImageFormat::Ico => "ICO",
        _ => "Image",
    }
}

/// Decodes an accepted raster image: header dimensions first (limits are
/// checked before any pixel memory is allocated), then the full decode with
/// EXIF orientation applied.
fn load_raster(bytes: &[u8], format: ImageFormat) -> Result<LoadedSource, String> {
    let decode_error = |error: image::ImageError| {
        eprintln!("image decode error: {error}");
        "The image could not be decoded. The file may be damaged.".to_string()
    };

    let (width, height) = image::ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(decode_error)?;
    check_dimensions(width, height)?;

    let mut decoder = image::ImageReader::with_format(Cursor::new(bytes), format)
        .into_decoder()
        .map_err(decode_error)?;
    let orientation = decoder.orientation().map_err(decode_error)?;
    let mut dynamic = image::DynamicImage::from_decoder(decoder).map_err(decode_error)?;
    dynamic.apply_orientation(orientation);
    let master = dynamic.to_rgba8();

    let mut warnings = Vec::new();
    if width.min(height) < 512 {
        warnings.push(format!(
            "The source is small ({width}x{height} px). Larger icons will look blurry. Use at least 512x512."
        ));
    }
    if matches!(format, ImageFormat::Jpeg | ImageFormat::Bmp) {
        warnings.push(
            "This format has no transparency, so the logo background will stay in the icons."
                .into(),
        );
    }

    Ok(LoadedSource {
        kind: SourceKind::Raster,
        format_label: format_label(format).to_string(),
        original_width: width,
        original_height: height,
        master,
        svg_bytes: None,
        warnings,
    })
}

/// Parses and renders an SVG source: longest side becomes 1024 px on a
/// transparent background; local-file `<image>` hrefs are ignored.
fn load_svg(bytes: &[u8]) -> Result<LoadedSource, String> {
    let without_bom = &bytes[bom_len(bytes)..];
    let text = std::str::from_utf8(without_bom)
        .map_err(|_| "The SVG file is not valid UTF-8 text.".to_string())?;
    let has_text = text.contains("<text");

    let mut options = resvg::usvg::Options::default();
    // Ignore <image> hrefs pointing at local files or URLs (same policy as
    // usvg's own from_data_nested); data: URIs keep the default resolver.
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);

    let tree = resvg::usvg::Tree::from_data(without_bom, &options).map_err(|error| {
        eprintln!("SVG parse error: {error}");
        "The SVG file could not be read. It may be damaged.".to_string()
    })?;

    let size = tree.size();
    let (width, height) = (size.width(), size.height());
    if !(width > 0.0 && height > 0.0 && width.is_finite() && height.is_finite()) {
        return Err("The SVG has an invalid size.".into());
    }
    let scale = SVG_MASTER_SIZE as f32 / width.max(height);
    let pw = ((width * scale).round() as u32).max(1);
    let ph = ((height * scale).round() as u32).max(1);
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(pw, ph).ok_or("The SVG is too large to render.")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    // tiny-skia pixmaps are premultiplied; convert to straight RGBA.
    let mut master = RgbaImage::new(pw, ph);
    for (dst, src) in master.pixels_mut().zip(pixmap.pixels()) {
        let color = src.demultiply();
        *dst = image::Rgba([color.red(), color.green(), color.blue(), color.alpha()]);
    }

    let mut warnings = Vec::new();
    if has_text {
        warnings.push(
            "This SVG contains text. Text is not rendered. Convert text to outlines (paths) in your vector editor."
                .into(),
        );
    }

    Ok(LoadedSource {
        kind: SourceKind::Svg,
        format_label: "SVG".to_string(),
        original_width: width.round() as u32,
        original_height: height.round() as u32,
        master,
        svg_bytes: Some(bytes.to_vec()),
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::DynamicImage;
    use image::Rgba;

    fn encode(format: ImageFormat, img: &DynamicImage) -> Vec<u8> {
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), format)
            .expect("encodes");
        out
    }

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        encode(ImageFormat::Png, &DynamicImage::new_rgba8(width, height))
    }

    fn jpeg_bytes(width: u32, height: u32) -> Vec<u8> {
        encode(ImageFormat::Jpeg, &DynamicImage::new_rgb8(width, height))
    }

    fn bmp_bytes(width: u32, height: u32) -> Vec<u8> {
        encode(ImageFormat::Bmp, &DynamicImage::new_rgb8(width, height))
    }

    #[test]
    fn check_file_size_bounds() {
        assert!(check_file_size(0).is_err());
        assert!(check_file_size(1).is_ok());
        assert!(check_file_size(MAX_FILE_BYTES).is_ok());
        assert!(check_file_size(MAX_FILE_BYTES + 1).is_err());
    }

    #[test]
    fn dimension_limits_are_rejected() {
        assert!(check_dimensions(0, 100).is_err());
        assert!(check_dimensions(100, 0).is_err());
        assert!(check_dimensions(10_001, 10).is_err());
        assert!(check_dimensions(10, 10_001).is_err());
        // Above the megapixel limit, without exceeding a side.
        assert!(check_dimensions(10_000, 10_001).is_err());
        assert!(check_dimensions(20_000, 20_000).is_err());
        // Exactly at the limits: allowed.
        assert!(check_dimensions(10_000, 10_000).is_ok());
        assert!(check_dimensions(600, 400).is_ok());
    }

    #[test]
    fn a_small_png_loads_with_the_small_warning() {
        let source = load_source_from_bytes(&png_bytes(64, 32)).expect("loads");
        assert_eq!(source.kind, SourceKind::Raster);
        assert_eq!(source.format_label, "PNG");
        assert_eq!((source.original_width, source.original_height), (64, 32));
        assert_eq!((source.master.width(), source.master.height()), (64, 32));
        assert!(
            source
                .warnings
                .iter()
                .any(|w| w.contains("The source is small (64x32 px)")),
            "warnings: {:?}",
            source.warnings
        );
        assert!(source.svg_bytes.is_none());
    }

    #[test]
    fn a_600x600_png_has_no_small_warning() {
        let source = load_source_from_bytes(&png_bytes(600, 600)).expect("loads");
        assert!(source.warnings.is_empty());
    }

    #[test]
    fn jpeg_and_bmp_get_the_no_transparency_warning() {
        let jpeg = load_source_from_bytes(&jpeg_bytes(600, 600)).expect("loads");
        assert_eq!(jpeg.format_label, "JPEG");
        assert!(
            jpeg.warnings.iter().any(|w| w.contains("no transparency")),
            "warnings: {:?}",
            jpeg.warnings
        );

        let bmp = load_source_from_bytes(&bmp_bytes(600, 600)).expect("loads");
        assert_eq!(bmp.format_label, "BMP");
        assert!(
            bmp.warnings.iter().any(|w| w.contains("no transparency")),
            "warnings: {:?}",
            bmp.warnings
        );
    }

    #[test]
    fn a_png_file_is_detected_by_content_not_extension() {
        // Wrong extension on purpose: only the bytes may decide.
        let path = std::env::temp_dir().join(format!(
            "erx_site_icons_test_{}_not_really.bin",
            std::process::id()
        ));
        std::fs::write(&path, png_bytes(64, 32)).expect("writes");
        let loaded = load_source(&path);
        let _ = std::fs::remove_file(&path);
        let source = loaded.expect("loads");
        assert_eq!(source.format_label, "PNG");
        assert_eq!((source.original_width, source.original_height), (64, 32));
    }

    #[test]
    fn inline_svg_renders_an_opaque_red_master() {
        let svg =
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="100" height="100" fill="#ff0000"/></svg>"##;
        let source = load_source_from_bytes(svg).expect("loads");
        assert_eq!(source.kind, SourceKind::Svg);
        assert_eq!(source.format_label, "SVG");
        assert_eq!(
            (source.master.width(), source.master.height()),
            (1024, 1024)
        );
        assert_eq!(source.master.get_pixel(512, 512), &Rgba([255, 0, 0, 255]));
        assert_eq!(source.svg_bytes.as_deref(), Some(&svg[..]));
    }

    #[test]
    fn svg_with_bom_prolog_and_comment_loads() {
        let svg = "\u{feff}<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- a comment -->\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"100\"><rect width=\"100\" height=\"100\" fill=\"#ff0000\"/></svg>";
        let source = load_source_from_bytes(svg.as_bytes()).expect("loads");
        assert_eq!(source.kind, SourceKind::Svg);
        assert_eq!(
            (source.master.width(), source.master.height()),
            (1024, 1024)
        );
    }

    #[test]
    fn svg_aspect_ratio_is_kept() {
        let svg =
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><rect width="200" height="100" fill="#00ff00"/></svg>"##;
        let source = load_source_from_bytes(svg).expect("loads");
        assert_eq!((source.master.width(), source.master.height()), (1024, 512));
    }

    #[test]
    fn svg_with_text_gets_the_text_warning() {
        let svg =
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><text x="10" y="10">hi</text></svg>"#;
        let source = load_source_from_bytes(svg).expect("loads");
        assert!(
            source
                .warnings
                .iter()
                .any(|w| w.contains("Text is not rendered")),
            "warnings: {:?}",
            source.warnings
        );
    }

    #[test]
    fn svg_with_zero_size_is_rejected_without_panicking() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="0" height="0"></svg>"#;
        assert!(load_source_from_bytes(svg).is_err());
    }

    #[test]
    fn invalid_svg_is_a_friendly_error_without_panicking() {
        let error = load_source_from_bytes(b"<svg <svg &&&").unwrap_err();
        assert!(error.contains("could not be read"), "{error}");
    }

    #[test]
    fn heic_magic_bytes_get_the_friendly_error() {
        let mut bytes = vec![0u8, 0, 0, 0x18];
        bytes.extend_from_slice(b"ftypheic");
        bytes.extend_from_slice(&[0; 16]);
        let error = load_source_from_bytes(&bytes).unwrap_err();
        assert!(error.contains("HEIC"), "{error}");
    }

    #[test]
    fn pdf_psd_and_gzip_get_friendly_errors() {
        let pdf = load_source_from_bytes(b"%PDF-1.7 fake").unwrap_err();
        assert!(pdf.contains("PDF"), "{pdf}");

        let psd = load_source_from_bytes(b"8BPS....fake").unwrap_err();
        assert!(psd.contains("Photoshop"), "{psd}");

        let gzip = load_source_from_bytes(&[0x1F, 0x8B, 8, 0, 1, 2, 3, 4]).unwrap_err();
        assert!(gzip.contains("Compressed"), "{gzip}");
    }

    #[test]
    fn random_bytes_are_not_a_supported_image() {
        let error = load_source_from_bytes(b"hello world, not an image").unwrap_err();
        assert!(error.contains("not a supported image"), "{error}");
    }
}
