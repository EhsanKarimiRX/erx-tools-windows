//! PNG rasterization of the shared QR geometry via `tiny-skia` (pure Rust,
//! ARM64-safe). Renders from the SAME shapes the SVG writer consumes, so
//! the PNG file and the SVG file always match.
//!
//! The requested pixel size is used EXACTLY: the module scale is float
//! (`requested / total_modules`), so the saved PNG is always exactly the
//! size the UI shows, and the centered logo stays exactly centered.

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, PixmapPaint};

use super::colors::Rgb;
use super::geometry::{FillRule as QrFillRule, LogoDraw, LogoPlate, Segment, Shape};

/// Requested PNG side lengths: the allowed range (the UI default of 1080
/// is the frontend's concern; Rust validates the range).
pub const MIN_PNG_SIZE: u32 = 256;
pub const MAX_PNG_SIZE: u32 = 4096;

/// Renders the full code (background + shapes) into PNG bytes at EXACTLY
/// `requested_size` × `requested_size` pixels.
///
/// `total_modules` is the canvas side in module units (grid + margins);
/// the geometry is already offset accordingly. The module scale is float
/// (`requested_size / total_modules`), so module edges are anti-aliased
/// when they do not land on whole pixels. `background` of `None` renders
/// with transparency. The `plate` (Rounded style + logo, opaque
/// background) is drawn over the module paths and under the logo image.
pub fn render_png(
    shapes: &[Shape],
    total_modules: usize,
    foreground: Rgb,
    background: Option<Rgb>,
    requested_size: u32,
    plate: Option<&LogoPlate>,
    logo: Option<&LogoDraw>,
) -> Result<Vec<u8>, String> {
    let scale = requested_size as f32 / total_modules as f32;
    let mut pixmap = Pixmap::new(requested_size, requested_size)
        .ok_or_else(|| "The requested PNG size is too large.".to_string())?;

    if let Some(bg) = background {
        pixmap.fill(Color::from_rgba8(bg.r, bg.g, bg.b, 255));
    }
    // A fresh Pixmap is fully transparent, so `None` needs no clearing.

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

    if let (Some(plate), Some(bg)) = (plate, background) {
        // The rounded plate covers the square hole edge: filled with the
        // background color, over the modules and under the logo. It uses
        // the same rounded-rect geometry the module clearing used.
        let segments =
            super::geometry::rounded_rect(plate.x, plate.y, plate.size, plate.size, plate.radius);
        if let Some(path) = build_path(&segments, scale) {
            let plate_paint = fill_paint(bg);
            pixmap.fill_path(
                &path,
                &plate_paint,
                FillRule::Winding,
                tiny_skia::Transform::identity(),
                None,
            );
        }
    }

    if let Some(logo) = logo {
        draw_logo(&mut pixmap, logo, scale)?;
    }

    pixmap
        .encode_png()
        .map_err(|_| "Failed to encode the PNG image.".to_string())
}

/// Composites the prepared logo PNG (base64) centered on the code. The
/// logo covers `logo.size` module units; its pixels are resampled to that
/// rectangle during drawing. The float module scale keeps it exactly
/// centered: the drawn center is `total_modules / 2 × scale`, i.e. the
/// pixmap center, for any requested size.
fn draw_logo(pixmap: &mut Pixmap, logo: &LogoDraw, module_scale: f32) -> Result<(), String> {
    let bytes =
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &logo.png_base64)
            .map_err(|_| "The logo image could not be read.".to_string())?;
    let source =
        Pixmap::decode_png(&bytes).map_err(|_| "The logo image could not be read.".to_string())?;

    let target = logo.size as f32 * module_scale;
    if target <= 0.0 {
        return Ok(());
    }
    let scale = target / source.width() as f32;
    let px = (logo.x * f64::from(module_scale)) as f32;
    let py = (logo.y * f64::from(module_scale)) as f32;
    // draw_pixmap applies its fill transform to the x/y position too, so
    // the position must be baked into the transform (scale, then shift)
    // and the pixmap drawn at the origin. The default `PixmapPaint` draws
    // the pixmap with its own colors; a solid `Paint` shader would tint it.
    pixmap.draw_pixmap(
        0,
        0,
        source.as_ref(),
        &PixmapPaint::default(),
        tiny_skia::Transform::from_scale(scale, scale).post_translate(px, py),
        None,
    );
    Ok(())
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
        render_png(&shapes, total_modules, BLACK, background, 512, None, None).expect("renders")
    }

    #[test]
    fn png_is_exactly_the_requested_size() {
        let png = sample(29, Some(WHITE));
        let decoded = image::load_from_memory(&png).expect("our PNG decodes");
        assert_eq!((decoded.width(), decoded.height()), (512, 512));

        // Any requested size is honored exactly — no snapping, even when
        // the size does not divide into whole pixels per module.
        let matrix =
            Matrix::from_text("sample", crate::tools::qr_code::matrix::Ecc::Low).expect("fits");
        let shapes = build_shapes(&matrix, QrStyle::Sharp, 4, None);
        let png = render_png(&shapes, 29, BLACK, Some(WHITE), 333, None, None).expect("renders");
        let decoded = image::load_from_memory(&png).expect("our PNG decodes");
        assert_eq!((decoded.width(), decoded.height()), (333, 333));
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
        // its center must be black. With margin 4 the module center sits
        // at 4.5 modules; the float scale maps it to ≈79 px at 512.
        let png = sample(29, Some(WHITE));
        let decoded = image::load_from_memory(&png).unwrap().to_rgba8();
        let center = (4.5 * (512.0 / 29.0)) as u32;
        assert_eq!(decoded.get_pixel(center, center).0, [0, 0, 0, 255]);
    }

    #[test]
    fn rounded_style_renders_curves_without_error() {
        let matrix =
            Matrix::from_text("round", crate::tools::qr_code::matrix::Ecc::Quartile).expect("fits");
        let shapes = build_shapes(&matrix, QrStyle::Round, 4, None);
        let png = render_png(
            &shapes,
            matrix.size() + 8,
            BLACK,
            Some(WHITE),
            512,
            None,
            None,
        )
        .expect("rounded render works");
        assert!(!png.is_empty());
    }

    /// A solid opaque 64×64 logo PNG as base64 (what load_logo produces,
    /// shrunk — only the drawn position matters here).
    fn solid_logo_base64() -> String {
        let img = image::RgbaImage::from_pixel(64, 64, image::Rgba([200, 10, 10, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, png)
    }

    #[test]
    fn logo_stays_centered_at_an_awkward_size() {
        // A logo drawn over an uncleaned grid must land exactly on the
        // pixmap center at a size with a fractional module scale.
        let matrix =
            Matrix::from_text("centered", crate::tools::qr_code::matrix::Ecc::High).expect("fits");
        let margin = 4usize;
        let total = matrix.size() + 2 * margin;
        let shapes = build_shapes(&matrix, QrStyle::Sharp, margin, None);
        let logo = LogoDraw {
            x: total as f64 / 2.0 - 2.0,
            y: total as f64 / 2.0 - 2.0,
            size: 4.0,
            png_base64: solid_logo_base64(),
        };
        let requested = 333u32;
        let png = render_png(
            &shapes,
            total,
            BLACK,
            Some(WHITE),
            requested,
            None,
            Some(&logo),
        )
        .expect("renders");
        let decoded = image::load_from_memory(&png)
            .expect("our PNG decodes")
            .to_rgba8();

        let mid = requested / 2;
        // The logo spans center ± 2 modules (≈ ±20 px here): the middle and
        // points 15 px out on every axis are all logo-colored, so the logo
        // is centered on BOTH axes.
        for (dx, dy) in [(0i32, 0i32), (-15, 0), (15, 0), (0, -15), (0, 15)] {
            let x = (mid as i32 + dx) as u32;
            let y = (mid as i32 + dy) as u32;
            assert_eq!(
                decoded.get_pixel(x, y).0,
                [200, 10, 10, 255],
                "offset ({dx},{dy}) is not logo-colored"
            );
        }
    }
}
