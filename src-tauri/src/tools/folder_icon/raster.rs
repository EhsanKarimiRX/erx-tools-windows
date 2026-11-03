//! PNG rasterization of the shared folder geometry via `tiny-skia` (pure
//! Rust, ARM64-safe). Renders from the SAME parameters the SVG writer
//! consumes, so the .ico frames and the preview always match.
//!
//! Clipping (the pattern must not leave the front panel) uses a `Mask`,
//! verified against tiny-skia 0.12.0 (`Mask::fill_path` +
//! `fill_path(..., Some(&mask))`).
//!
//! Pattern anti-noise rule: below the frame size where the standard cell
//! would shrink under 2 device pixels, the cell is widened to exactly
//! 2 device pixels and the pattern is REGENERATED for that frame — bars
//! stay crisp instead of dissolving into gray noise at 16/24/32 px.

use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Mask, Paint, PathBuilder, Pixmap, Point, Shader,
    SpreadMode, Transform,
};

use super::colors::Rgb;
use super::core::IconParams;
use super::geometry::{CANVAS, FRONT, Fill, Layer, Segment, pattern_cell};

/// The .ico frame sizes, smallest to largest (spec 4.1).
pub const FRAME_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

/// Minimum pattern cell at small frame sizes, in device pixels (spec 4.1:
/// "enforce a minimum pattern cell of 2 px so the pattern does not turn
/// into noise").
pub const MIN_PATTERN_DEVICE_PX: f64 = 2.0;

/// The pattern cell width in canvas px for a frame of `size` device px:
/// normally the standard cell for the size step, widened to exactly
/// `MIN_PATTERN_DEVICE_PX` device pixels when that would shrink below it.
pub fn pattern_cell_for_frame(params: &IconParams, size: u32) -> f64 {
    let standard = pattern_cell(params.pattern_size);
    let device = standard * f64::from(size) / CANVAS;
    if device >= MIN_PATTERN_DEVICE_PX {
        standard
    } else {
        MIN_PATTERN_DEVICE_PX * CANVAS / f64::from(size)
    }
}

/// Renders the complete icon into a pixmap of `size` × `size` device px.
pub fn render_pixmap(params: &IconParams, size: u32) -> Result<Pixmap, String> {
    let drawing = params.drawing_with_cell(pattern_cell_for_frame(params, size));
    let mut pixmap = Pixmap::new(size, size)
        .ok_or_else(|| "The requested icon size is too large.".to_string())?;
    let scale = f64::from(size) / CANVAS;

    for layer in &drawing.layers {
        match layer {
            Layer::Fill(shape) => {
                let Some(path) = device_path(&shape.segments, scale) else {
                    continue; // empty path — nothing to draw
                };
                let paint = shape_paint(shape, scale)?;
                pixmap.fill_path(
                    &path,
                    &paint,
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
            Layer::ClippedGroup { clip, shapes } => {
                let Some(clip_path) = device_path(clip, scale) else {
                    continue;
                };
                // The clip mask starts fully opaque-black (blocking) and is
                // opened exactly where the front panel lies.
                let mut mask = Mask::new(size, size)
                    .ok_or_else(|| "The requested icon size is too large.".to_string())?;
                mask.fill_path(&clip_path, FillRule::Winding, true, Transform::identity());
                for shape in shapes {
                    let Some(path) = device_path(&shape.segments, scale) else {
                        continue;
                    };
                    let paint = shape_paint(shape, scale)?;
                    pixmap.fill_path(
                        &path,
                        &paint,
                        FillRule::Winding,
                        Transform::identity(),
                        Some(&mask),
                    );
                }
            }
        }
    }
    Ok(pixmap)
}

/// Renders one .ico frame as PNG bytes.
pub fn render_frame_png(params: &IconParams, size: u32) -> Result<Vec<u8>, String> {
    render_pixmap(params, size)?
        .encode_png()
        .map_err(|_| "Failed to encode the icon image.".to_string())
}

/// Builds a `tiny-skia` path in device pixels (coordinates pre-scaled, so
/// the fill transform stays identity and shaders use device coordinates).
fn device_path(segments: &[Segment], scale: f64) -> Option<tiny_skia::Path> {
    let mut builder = PathBuilder::new();
    for segment in segments {
        match *segment {
            Segment::MoveTo(x, y) => {
                builder.move_to((x * scale) as f32, (y * scale) as f32);
            }
            Segment::LineTo(x, y) => {
                builder.line_to((x * scale) as f32, (y * scale) as f32);
            }
            Segment::QuadTo(cx, cy, x, y) => {
                builder.quad_to(
                    (cx * scale) as f32,
                    (cy * scale) as f32,
                    (x * scale) as f32,
                    (y * scale) as f32,
                );
            }
            Segment::Close => builder.close(),
        }
    }
    builder.finish()
}

/// The paint for one shape: a solid color with the shape's opacity, or the
/// front-panel highlight as a vertical linear gradient (device-space y).
fn shape_paint(shape: &super::geometry::Shape, scale: f64) -> Result<Paint<'static>, String> {
    let alpha_byte = |a: f64| (a.clamp(0.0, 1.0) * 255.0).round() as u8;
    match shape.fill {
        Fill::Solid(color) => Ok(Paint {
            anti_alias: true,
            shader: Shader::SolidColor(Color::from_rgba8(
                color.r,
                color.g,
                color.b,
                alpha_byte(shape.opacity),
            )),
            ..Paint::default()
        }),
        Fill::VerticalGradient {
            color: Rgb { r, g, b },
            top_alpha,
            bottom_alpha,
        } => {
            let top = Point::from_xy((FRONT.x * scale) as f32, (FRONT.y * scale) as f32);
            let bottom = Point::from_xy(
                (FRONT.x * scale) as f32,
                ((FRONT.y + FRONT.h) * scale) as f32,
            );
            let shader = LinearGradient::new(
                top,
                bottom,
                vec![
                    GradientStop::new(0.0, Color::from_rgba8(r, g, b, alpha_byte(top_alpha))),
                    GradientStop::new(1.0, Color::from_rgba8(r, g, b, alpha_byte(bottom_alpha))),
                ],
                SpreadMode::Pad,
                Transform::identity(),
            )
            .ok_or_else(|| "The icon highlight could not be built.".to_string())?;
            Ok(Paint {
                anti_alias: true,
                shader,
                ..Paint::default()
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::folder_icon::settings::PatternKind;

    const BASE: Rgb = Rgb {
        r: 0,
        g: 0x78,
        b: 0xd4,
    };
    const BACK: Rgb = Rgb {
        r: 0,
        g: 0x40,
        b: 0x80,
    };
    const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };

    fn params(pattern: PatternKind, pattern_color: Rgb, step: i32, opacity: f64) -> IconParams {
        IconParams {
            base_color: BASE,
            back_color: BACK,
            pattern,
            pattern_color,
            pattern_size: step,
            pattern_opacity: opacity,
        }
    }

    /// The front-panel highlight alpha at canvas y (linear between the
    /// stops 0.20 → 0.02 over the front panel).
    fn highlight_alpha_at(y: f64) -> f64 {
        let t = (y - FRONT.y) / FRONT.h;
        0.20 + (0.02 - 0.20) * t
    }

    fn blend_over(over: Rgb, alpha: f64, under: Rgb) -> [u8; 3] {
        let mix = |o: u8, u: u8| -> u8 {
            (f64::from(o) * alpha + f64::from(u) * (1.0 - alpha)).round() as u8
        };
        [
            mix(over.r, under.r),
            mix(over.g, under.g),
            mix(over.b, under.b),
        ]
    }

    #[test]
    fn all_frame_sizes_render() {
        let p = params(PatternKind::None, BLACK, 1, 1.0);
        for size in FRAME_SIZES {
            let pixmap = render_pixmap(&p, size).expect("renders");
            assert_eq!((pixmap.width(), pixmap.height()), (size, size));
        }
        assert!(render_frame_png(&p, 48).is_ok());
    }

    #[test]
    fn panel_colors_and_transparency_at_256() {
        let p = params(PatternKind::None, BLACK, 1, 1.0);
        let pixmap = render_pixmap(&p, 256).unwrap();

        // Corner: fully transparent.
        let corner = pixmap.pixel(2, 2).unwrap().demultiply();
        assert_eq!(corner.alpha(), 0);

        // Back panel strip (canvas 128, 88): the derived back color.
        let back = pixmap.pixel(128, 88).unwrap().demultiply();
        assert_eq!(
            (back.red(), back.green(), back.blue()),
            (BACK.r, BACK.g, BACK.b),
            "back strip must be the exact back color"
        );

        // Front panel center (canvas 128, 136): base color under the
        // highlight fade.
        let center = pixmap.pixel(128, 136).unwrap().demultiply();
        let expected = blend_over(
            Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
            highlight_alpha_at(136.0),
            BASE,
        );
        assert!(
            (i16::from(center.red()) - i16::from(expected[0])).abs() <= 1
                && (i16::from(center.green()) - i16::from(expected[1])).abs() <= 1
                && (i16::from(center.blue()) - i16::from(expected[2])).abs() <= 1,
            "center {center:?} vs {expected:?}"
        );
    }

    #[test]
    fn pattern_is_clipped_to_the_front_panel() {
        // White bars at full opacity would be unmissable on the back panel.
        let white = Rgb {
            r: 255,
            g: 255,
            b: 255,
        };
        let p = params(PatternKind::Vertical, white, 3, 1.0);
        let pixmap = render_pixmap(&p, 256).unwrap();
        // Inside the front panel some pixel must be the pure pattern color…
        let bar_x = (FRONT.x + pattern_cell(3) / 2.0) as u32; // inside bar 1
        let bar = pixmap.pixel(bar_x, 156).unwrap().demultiply();
        assert_eq!((bar.red(), bar.green(), bar.blue()), (255, 255, 255));
        // …but the back panel strip must stay the pure back color.
        let back = pixmap.pixel(128, 88).unwrap().demultiply();
        assert_eq!(
            (back.red(), back.green(), back.blue()),
            (BACK.r, BACK.g, BACK.b)
        );
    }

    #[test]
    fn pattern_opacity_blends_over_the_base() {
        let white = Rgb {
            r: 255,
            g: 255,
            b: 255,
        };
        let p = params(PatternKind::Vertical, white, 3, 0.5);
        let pixmap = render_pixmap(&p, 256).unwrap();
        let bar_x = (FRONT.x + pattern_cell(3) / 2.0) as u32;
        let bar = pixmap.pixel(bar_x, 156).unwrap().demultiply();
        // White at 50% over the base — pattern, then highlight on top.
        let over_pattern = blend_over(white, 0.5, BASE);
        let expected = blend_over(
            Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
            highlight_alpha_at(156.0),
            Rgb {
                r: over_pattern[0],
                g: over_pattern[1],
                b: over_pattern[2],
            },
        );
        assert!(
            (i16::from(bar.red()) - i16::from(expected[0])).abs() <= 1
                && (i16::from(bar.green()) - i16::from(expected[1])).abs() <= 1
                && (i16::from(bar.blue()) - i16::from(expected[2])).abs() <= 1,
            "bar {bar:?} vs {expected:?}"
        );
    }

    #[test]
    fn small_frames_widen_the_pattern_cell_to_two_device_pixels() {
        let black = Rgb { r: 0, g: 0, b: 0 };
        // Step 1 (4 px canvas cell) at 16 px: 0.25 device px without the
        // rule; the rule must widen it to 2 device px = 32 canvas px.
        let p = params(PatternKind::Vertical, black, 1, 1.0);
        assert!((pattern_cell_for_frame(&p, 16) - 32.0).abs() < 1e-9);
        assert!((pattern_cell_for_frame(&p, 32) - 16.0).abs() < 1e-9);
        // Large frames keep the standard cell.
        assert!((pattern_cell_for_frame(&p, 256) - 4.0).abs() < 1e-9);
        assert!((pattern_cell_for_frame(&p, 128) - 4.0).abs() < 1e-9);

        // The rendered 16 px frame really shows 2 px bars: pixels 2 and 6
        // fall inside bars, pixel 4 in a gap (bar 1: device 1.5–3.5,
        // gap: 3.5–5.5, bar 2: 5.5–7.5). The highlight overlay sits on top,
        // so bars are the pattern color under it.
        let pixmap = render_pixmap(&p, 16).unwrap();
        let white = Rgb {
            r: 255,
            g: 255,
            b: 255,
        };
        let expected_bar = blend_over(white, highlight_alpha_at(136.0), black);
        let bar = pixmap.pixel(2, 8).unwrap().demultiply();
        let gap = pixmap.pixel(4, 8).unwrap().demultiply();
        let bar2 = pixmap.pixel(6, 8).unwrap().demultiply();
        for bar_pixel in [&bar, &bar2] {
            assert!(
                (i16::from(bar_pixel.red()) - i16::from(expected_bar[0])).abs() <= 1
                    && (i16::from(bar_pixel.green()) - i16::from(expected_bar[1])).abs() <= 1
                    && (i16::from(bar_pixel.blue()) - i16::from(expected_bar[2])).abs() <= 1,
                "bar {bar_pixel:?} vs {expected_bar:?}"
            );
        }
        assert!(gap.green() > 100, "gap must show the base color, not black");
    }

    #[test]
    fn large_frames_keep_sharp_pattern_cells() {
        // At 256 the standard cell for step 1 is exactly 4 device px.
        let black = Rgb { r: 0, g: 0, b: 0 };
        let p = params(PatternKind::Vertical, black, 1, 1.0);
        let pixmap = render_pixmap(&p, 256).unwrap();
        // Bar 1: canvas 24–28 → device 24–28. Center of bar and of gap.
        // The highlight overlay sits on top of the pattern.
        let expected_bar = blend_over(
            Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
            highlight_alpha_at(156.0),
            black,
        );
        let bar = pixmap.pixel(26, 156).unwrap().demultiply();
        assert!(
            (i16::from(bar.red()) - i16::from(expected_bar[0])).abs() <= 1
                && (i16::from(bar.green()) - i16::from(expected_bar[1])).abs() <= 1
                && (i16::from(bar.blue()) - i16::from(expected_bar[2])).abs() <= 1,
            "bar {bar:?} vs {expected_bar:?}"
        );
        let gap = pixmap.pixel(30, 156).unwrap().demultiply();
        assert!(gap.green() > 100);
    }
}
