//! PNG rasterization of the shared QR geometry via `tiny-skia` (pure Rust,
//! ARM64-safe). Renders from the SAME shapes the SVG writer consumes, so
//! the PNG file and the SVG file always match.
//!
//! To keep edges perfectly sharp, the requested pixel size is snapped DOWN
//! to a whole number of pixels per module.

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap};

use super::colors::Rgb;
use super::geometry::{FillRule as QrFillRule, Segment, Shape};

/// Requested PNG side lengths: the allowed range (the UI default of 1024
/// is the frontend's concern; Rust validates the range).
pub const MIN_PNG_SIZE: u32 = 256;
pub const MAX_PNG_SIZE: u32 = 4096;

/// The exact pixel size a render will use: a whole number of pixels per
/// module, snapped down from the requested size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnappedSize {
    pub pixels: u32,
    pub pixels_per_module: u32,
}

/// Snaps a requested PNG size to whole pixels per module. Always at least
/// 1 pixel per module, so even a version-40 code renders (smaller than
/// requested, but valid and sharp; the UI shows the exact size).
pub fn snap_size(total_modules: usize, requested: u32) -> SnappedSize {
    let per_module = (requested as usize / total_modules).max(1);
    SnappedSize {
        pixels: (per_module * total_modules) as u32,
        pixels_per_module: per_module as u32,
    }
}

/// Renders the full code (background + shapes) into PNG bytes.
///
/// `total_modules` is the canvas side in module units (grid + margins);
/// the geometry is already offset accordingly. `background` of `None`
/// renders with transparency.
pub fn render_png(
    shapes: &[Shape],
    total_modules: usize,
    foreground: Rgb,
    background: Option<Rgb>,
    requested_size: u32,
) -> Result<Vec<u8>, String> {
    let snapped = snap_size(total_modules, requested_size);
    let mut pixmap = Pixmap::new(snapped.pixels, snapped.pixels)
        .ok_or_else(|| "The requested PNG size is too large.".to_string())?;

    if let Some(bg) = background {
        pixmap.fill(Color::from_rgba8(bg.r, bg.g, bg.b, 255));
    }
    // A fresh Pixmap is fully transparent, so `None` needs no clearing.

    let scale = snapped.pixels_per_module as f32;
    let paint = fill_paint(foreground);

    for shape in shapes {
        let Some(path) = build_path(&shape.segments, scale) else {
            continue; // empty shape — nothing to draw
        };
        let rule = match shape.fill_rule {
            QrFillRule::NonZero => FillRule::Winding,
            QrFillRule::EvenOdd => FillRule::EvenOdd,
        };
        pixmap.fill_path(&path, &paint, rule, tiny_skia::Transform::identity(), None);
    }

    pixmap
        .encode_png()
        .map_err(|_| "Failed to encode the PNG image.".to_string())
}

/// Solid, anti-aliased paint for the foreground color.
fn fill_paint(color: Rgb) -> Paint<'static> {
    Paint {
        anti_alias: true,
        shader: tiny_skia::Shader::SolidColor(Color::from_rgba8(color.r, color.g, color.b, 255)),
        ..Paint::default()
    }
}
/// Builds a `tiny-skia` path from our segments, scaling module units to
/// pixels. Returns `None` for an empty segment list.
fn build_path(segments: &[Segment], scale: f32) -> Option<tiny_skia::Path> {
    if segments.is_empty() {
        return None;
    }
    let mut builder = PathBuilder::new();
    for segment in segments {
        match *segment {
            Segment::MoveTo(x, y) => builder.move_to(x as f32 * scale, y as f32 * scale),
            Segment::LineTo(x, y) => builder.line_to(x as f32 * scale, y as f32 * scale),
            Segment::QuadTo(cx, cy, x, y) => builder.quad_to(
                cx as f32 * scale,
                cy as f32 * scale,
                x as f32 * scale,
                y as f32 * scale,
            ),
            Segment::Close => builder.close(),
        }
    }
    builder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::qr_code::geometry::{QrStyle, build_shapes};
    use crate::tools::qr_code::matrix::Matrix;

    const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };
    const WHITE: Rgb = Rgb {
        r: 255,
        g: 255,
        b: 255,
    };

    fn sample(total_modules: usize, background: Option<Rgb>) -> Vec<u8> {
        let matrix =
            Matrix::from_text("sample", crate::tools::qr_code::matrix::Ecc::Low).expect("fits");
        let shapes = build_shapes(&matrix, QrStyle::Sharp, 4, None);
        render_png(&shapes, total_modules, BLACK, background, 512).expect("renders")
    }

    #[test]
    fn snapping_is_whole_modules_and_never_exceeds_request() {
        // 29 modules (21 + 2*4 margin): 512/29 = 17.6 -> 17 px per module.
        let snapped = snap_size(29, 512);
        assert_eq!(snapped.pixels_per_module, 17);
        assert_eq!(snapped.pixels, 29 * 17);
        assert!(snapped.pixels <= 512);

        // Exact division needs no change.
        assert_eq!(snap_size(32, 512).pixels, 512);

        // Tiny request still renders one pixel per module.
        let tiny = snap_size(29, 10);
        assert_eq!(tiny.pixels_per_module, 1);
        assert_eq!(tiny.pixels, 29);
    }

    #[test]
    fn png_has_the_snapped_dimensions() {
        let png = sample(29, Some(WHITE));
        let decoded = image::load_from_memory(&png).expect("our PNG decodes");
        assert_eq!((decoded.width(), decoded.height()), (29 * 17, 29 * 17));
    }

    #[test]
    fn background_pixel_is_bg_or_transparent() {
        // Corner pixel (inside the margin): opaque white with a background,
        // fully transparent without one.
        let opaque = image::load_from_memory(&sample(29, Some(WHITE)))
            .unwrap()
            .to_rgba8();
        assert_eq!(opaque.get_pixel(2, 2).0, [255, 255, 255, 255]);

        let transparent = image::load_from_memory(&sample(29, None))
            .unwrap()
            .to_rgba8();
        assert_eq!(transparent.get_pixel(2, 2).0, [0, 0, 0, 0]);
    }

    #[test]
    fn module_center_is_foreground() {
        // The first module (0,0) of a QR is always dark (finder corner);
        // its center must be black at the snapped scale (margin 4 -> pixel
        // (4.5 * 17) ~ 76).
        let png = sample(29, Some(WHITE));
        let decoded = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(decoded.get_pixel(76, 76).0, [0, 0, 0, 255]);
    }

    #[test]
    fn rounded_style_renders_curves_without_error() {
        let matrix =
            Matrix::from_text("round", crate::tools::qr_code::matrix::Ecc::Quartile).expect("fits");
        let shapes = build_shapes(&matrix, QrStyle::Round, 4, None);
        let png = render_png(&shapes, matrix.size() + 8, BLACK, Some(WHITE), 512)
            .expect("rounded render works");
        assert!(!png.is_empty());
    }
}
