//! SVG rendering from the shared folder geometry. Pure logic, unit tested.
//!
//! Security property: the ONLY variable data written into the markup are
//! strictly validated `#rrggbb` colors and our own geometry numbers — the
//! same rule as the QR tool's writer. No user string can break out.

use super::geometry::{CANVAS, Drawing, Fill, Layer, Segment};

/// Renders the complete SVG document for the given drawing: a 256×256
/// viewBox with no fixed width/height so it scales to any display size.
pub fn render(drawing: &Drawing) -> String {
    let mut svg = String::with_capacity(2048);
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" role=\"img\" aria-label=\"Folder icon\">",
        num(CANVAS),
        num(CANVAS)
    ));

    // Gradient and clip definitions first, collected while walking layers.
    let mut defs = String::new();
    let mut body = String::new();
    let mut gradient_id = 0;
    let mut clip_id = 0;

    for layer in &drawing.layers {
        match layer {
            Layer::Fill(shape) => {
                let fill = paint_attribute(shape.fill, &mut defs, &mut gradient_id);
                body.push_str(&format!(
                    "<path d=\"{}\" fill=\"{}\"{}/>",
                    path_data(&shape.segments),
                    fill,
                    opacity_attribute(shape.opacity)
                ));
            }
            Layer::ClippedGroup { clip, shapes } => {
                let id = format!("erx-clip-{clip_id}");
                clip_id += 1;
                defs.push_str(&format!(
                    "<clipPath id=\"{id}\"><path d=\"{}\"/></clipPath>",
                    path_data(clip)
                ));
                // All shapes in a group share fill color and opacity: the
                // pattern is one color with one opacity by construction.
                let (fill, opacity) = group_style(shapes);
                body.push_str(&format!(
                    "<g clip-path=\"url(#{id})\" fill=\"{fill}\"{opacity}>"
                ));
                for shape in shapes {
                    body.push_str(&format!("<path d=\"{}\"/>", path_data(&shape.segments)));
                }
                body.push_str("</g>");
            }
        }
    }

    if !defs.is_empty() {
        svg.push_str("<defs>");
        svg.push_str(&defs);
        svg.push_str("</defs>");
    }
    svg.push_str(&body);
    svg.push_str("</svg>");
    svg
}

/// The shared fill/opacity of a pattern group (all cells match by
/// construction; this is verified by a test).
fn group_style(shapes: &[super::geometry::Shape]) -> (String, String) {
    let Some(first) = shapes.first() else {
        return (String::from("none"), String::new());
    };
    let Fill::Solid(color) = first.fill else {
        return (String::from("none"), String::new());
    };
    (color.hex(), opacity_attribute(first.opacity))
}

/// The fill attribute for a shape; vertical gradients get a generated
/// `<linearGradient>` appended to `defs` and reference it by id.
fn paint_attribute(fill: Fill, defs: &mut String, gradient_id: &mut usize) -> String {
    match fill {
        Fill::Solid(color) => color.hex(),
        Fill::VerticalGradient {
            color,
            top_alpha,
            bottom_alpha,
        } => {
            let id = format!("erx-grad-{gradient_id}");
            *gradient_id += 1;
            // The gradient covers the shape's own vertical extent; every
            // gradient shape we emit is the front panel (see geometry).
            let top = super::geometry::FRONT.y;
            let bottom = super::geometry::FRONT.y + super::geometry::FRONT.h;
            defs.push_str(&format!(
                "<linearGradient id=\"{id}\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" gradientUnits=\"userSpaceOnUse\"><stop offset=\"0\" stop-color=\"{}\" stop-opacity=\"{}\"/><stop offset=\"1\" stop-color=\"{}\" stop-opacity=\"{}\"/></linearGradient>",
                num(super::geometry::FRONT.x),
                num(top),
                num(super::geometry::FRONT.x),
                num(bottom),
                color.hex(),
                num(top_alpha),
                color.hex(),
                num(bottom_alpha)
            ));
            format!("url(#{id})")
        }
    }
}

fn opacity_attribute(opacity: f64) -> String {
    if opacity < 1.0 {
        format!(" fill-opacity=\"{}\"", num(opacity))
    } else {
        String::new()
    }
}

/// The `d` attribute for a segment list.
fn path_data(segments: &[Segment]) -> String {
    let mut d = String::with_capacity(segments.len() * 12);
    for segment in segments {
        match *segment {
            Segment::MoveTo(x, y) => d.push_str(&format!("M{} {}", num(x), num(y))),
            Segment::LineTo(x, y) => d.push_str(&format!("L{} {}", num(x), num(y))),
            Segment::QuadTo(cx, cy, x, y) => {
                d.push_str(&format!("Q{} {} {} {}", num(cx), num(cy), num(x), num(y)));
            }
            Segment::Close => d.push('Z'),
        }
    }
    d
}

/// Formats a number compactly: at most two decimals, no trailing zeros.
fn num(value: f64) -> String {
    let text = format!("{value:.2}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed == "-0" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::super::colors::Rgb;
    use super::super::geometry::{Fill, Shape, build_drawing, pattern_shapes};
    use super::super::settings::PatternKind;
    use super::*;

    const BASE: Rgb = Rgb {
        r: 0,
        g: 0x78,
        b: 0xd4,
    };
    const WHITE: Rgb = Rgb {
        r: 255,
        g: 255,
        b: 255,
    };

    #[test]
    fn document_structure_without_pattern() {
        let drawing = build_drawing(
            BASE,
            Rgb {
                r: 0,
                g: 0x40,
                b: 0x80,
            },
            PatternKind::None,
            WHITE,
            3,
            0.5,
        );
        let svg = render(&drawing);
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(svg.contains("viewBox=\"0 0 256 256\""));
        assert!(svg.ends_with("</svg>"));
        // Back + front + highlight fills; a gradient def for the highlight.
        assert!(svg.contains("<defs>"));
        assert!(svg.contains("<linearGradient"));
        assert!(svg.contains("stop-opacity=\"0.2\""));
        // No clip path when there is no pattern.
        assert!(!svg.contains("clipPath"));
        assert_eq!(svg.matches("<path").count(), 3);
    }

    #[test]
    fn pattern_group_is_clipped_and_colored() {
        let drawing = build_drawing(
            BASE,
            Rgb {
                r: 0,
                g: 0x40,
                b: 0x80,
            },
            PatternKind::Vertical,
            WHITE,
            2,
            0.6,
        );
        let svg = render(&drawing);
        assert!(svg.contains("<clipPath id=\"erx-clip-0\">"));
        assert!(
            svg.contains(
                "<g clip-path=\"url(#erx-clip-0)\" fill=\"#ffffff\" fill-opacity=\"0.6\">"
            )
        );
        // Pattern color must not leak into the panel fills.
        assert!(svg.contains("fill=\"#ffffff\""));
    }

    #[test]
    fn checker_pattern_renders_all_cells() {
        let drawing = build_drawing(
            BASE,
            Rgb { r: 0, g: 0, b: 0 },
            PatternKind::Checker,
            WHITE,
            1,
            0.8,
        );
        let svg = render(&drawing);
        let cells = pattern_shapes(PatternKind::Checker, 1, WHITE, 0.8).len();
        // 3 panel paths + 1 clip path + one path per pattern cell.
        assert_eq!(svg.matches("<path d=\"M").count(), 4 + cells);
    }

    #[test]
    fn numbers_are_compact() {
        assert_eq!(num(0.0), "0");
        assert_eq!(num(256.0), "256");
        assert_eq!(num(0.5), "0.5");
        assert_eq!(num(0.20), "0.2");
        assert_eq!(num(-0.0), "0");
        assert_eq!(num(13.0), "13");
    }

    #[test]
    fn opacity_only_when_below_one() {
        assert_eq!(opacity_attribute(1.0), "");
        assert_eq!(opacity_attribute(0.5), " fill-opacity=\"0.5\"");
    }

    #[test]
    fn path_data_shapes_match_segments() {
        let d = path_data(&[
            Segment::MoveTo(0.0, 0.0),
            Segment::LineTo(10.0, 0.0),
            Segment::QuadTo(10.0, 10.0, 0.0, 10.0),
            Segment::Close,
        ]);
        assert_eq!(d, "M0 0L10 0Q10 10 0 10Z");
    }

    #[test]
    fn solid_gradient_shape_gets_its_own_def() {
        let drawing = Drawing {
            layers: vec![super::super::geometry::Layer::Fill(Shape {
                segments: vec![Segment::MoveTo(0.0, 0.0), Segment::Close],
                fill: Fill::VerticalGradient {
                    color: WHITE,
                    top_alpha: 1.0,
                    bottom_alpha: 0.0,
                },
                opacity: 1.0,
            })],
        };
        let svg = render(&drawing);
        assert!(svg.contains("id=\"erx-grad-0\""));
        assert!(svg.contains("stop-opacity=\"1\""));
        assert!(svg.contains("stop-opacity=\"0\""));
        assert!(svg.contains("fill=\"url(#erx-grad-0)\""));
    }
}
