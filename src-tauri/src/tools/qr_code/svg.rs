//! SVG rendering from the shared geometry. Pure logic, unit tested.
//!
//! Security property: the ONLY variable data written into the markup are
//! strictly validated `#RRGGBB` colors and our own geometry numbers. QR
//! content never appears as text in the SVG (it lives in the module
//! pattern), so no user string can break out of the markup.

use super::colors::Rgb;
use super::geometry::{FillRule, Segment, Shape};

/// Renders the complete SVG document for the given shapes.
///
/// `total` is the canvas side in module units (grid + both margins). The
/// document has a `viewBox` but no fixed width/height, so it scales to any
/// display size (preview) and file size (export). `background` is omitted
/// entirely when transparent.
pub fn render(shapes: &[Shape], total: f64, foreground: Rgb, background: Option<Rgb>) -> String {
    let mut svg = String::with_capacity(2048 + shapes.len() * 256);
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" role=\"img\" aria-label=\"QR code\">",
        num(total),
        num(total)
    ));

    if let Some(background) = background {
        svg.push_str(&format!(
            "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
            num(total),
            num(total),
            background.hex()
        ));
    }

    for shape in shapes {
        svg.push_str("<path d=\"");
        for segment in &shape.segments {
            match *segment {
                Segment::MoveTo(x, y) => svg.push_str(&format!("M{} {}", num(x), num(y))),
                Segment::LineTo(x, y) => svg.push_str(&format!("L{} {}", num(x), num(y))),
                Segment::QuadTo(cx, cy, x, y) => {
                    svg.push_str(&format!("Q{} {} {} {}", num(cx), num(cy), num(x), num(y)));
                }
                Segment::Close => svg.push('Z'),
            }
        }
        // `nonzero` is the SVG default, so only even-odd gets an attribute.
        let rule = match shape.fill_rule {
            FillRule::NonZero => "",
            FillRule::EvenOdd => " fill-rule=\"evenodd\"",
        };
        svg.push_str(&format!("\" fill=\"{}\"{}/>", foreground.hex(), rule));
    }

    svg.push_str("</svg>");
    svg
}

/// Formats a number compactly: at most two decimals, no trailing zeros
/// (module geometry only produces halves, so this stays exact).
fn num(value: f64) -> String {
    let text = format!("{value:.2}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    // Guard against "-0" for a negative zero.
    if trimmed == "-0" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::qr_code::geometry::{QrStyle, build_shapes};
    use crate::tools::qr_code::matrix::{Ecc, Matrix};

    fn sample_svg(background: Option<Rgb>) -> String {
        let matrix = Matrix::from_text("X", Ecc::Low).expect("fits");
        let shapes = build_shapes(&matrix, QrStyle::Sharp, 4, None);
        render(&shapes, 29.0, Rgb::parse("#112233").unwrap(), background)
    }

    #[test]
    fn document_has_xmlns_viewbox_and_colors() {
        let svg = sample_svg(Some(Rgb::parse("#AABBCC").unwrap()));
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(svg.contains("viewBox=\"0 0 29 29\""));
        assert!(svg.contains("fill=\"#aabbcc\"")); // background rect
        assert!(svg.contains("fill=\"#112233\"")); // modules
        assert!(svg.ends_with("</svg>"));
    }

    #[test]
    fn transparent_background_omits_the_rect() {
        let svg = sample_svg(None);
        assert!(!svg.contains("<rect"));
    }

    #[test]
    fn content_text_never_appears_in_markup() {
        // The payload must never be embedded as text in the SVG.
        let secret = "SUPER-SECRET-payload-123";
        let matrix = Matrix::from_text(secret, Ecc::Low).expect("fits");
        let shapes = build_shapes(&matrix, QrStyle::Sharp, 4, None);
        let svg = render(&shapes, 29.0, Rgb::parse("#000000").unwrap(), None);
        assert!(!svg.contains(secret));
        // Even escaped fragments of it must not appear.
        assert!(!svg.contains("SECRET"));
    }

    #[test]
    fn evenodd_gets_attribute_nonzero_does_not() {
        let matrix = Matrix::from_text("ab", Ecc::Low).expect("fits");
        let shapes = build_shapes(&matrix, QrStyle::Round, 4, None);
        let svg = render(&shapes, 29.0, Rgb::parse("#000000").unwrap(), None);
        assert_eq!(svg.matches("fill-rule=\"evenodd\"").count(), 3); // 3 rings
        assert_eq!(svg.matches("fill=\"#000000\"").count(), 7); // all shapes
    }

    #[test]
    fn numbers_are_compact() {
        assert_eq!(num(0.0), "0");
        assert_eq!(num(1.0), "1");
        assert_eq!(num(0.5), "0.5");
        assert_eq!(num(2.25), "2.25");
        assert_eq!(num(-0.0), "0");
        assert_eq!(num(29.0), "29");
    }

    #[test]
    fn rounded_geometry_produces_quadratic_curves() {
        let matrix = Matrix::from_text("a", Ecc::Low).expect("fits");
        let shapes = build_shapes(&matrix, QrStyle::Round, 4, None);
        let svg = render(&shapes, 29.0, Rgb::parse("#000000").unwrap(), None);
        assert!(svg.contains('Q'), "rounded style must contain quad curves");
    }
}
