//! The one folder design as drawable geometry: a small set of fill shapes
//! that both backends (SVG writer and, in 4B, the tiny-skia PNG rasterizer)
//! consume, so the preview and the final .ico are always drawn from exactly
//! the same description — mirroring the QR tool's geometry/svg/raster split.
//!
//! Canvas: 256 × 256. The folder is wider than tall: a back panel with a
//! tab on the upper left, and a larger front panel overlapping the lower
//! part. All coordinates are our own design (soft, rounded, Windows 11-like
//! general look; no traced artwork).
//!
//! Layer order (bottom to top): back panel, front panel, pattern (clipped
//! to the front panel), top highlight (a white vertical fade over the front
//! panel).

/// Canvas side in px; the .ico frames scale this proportionally.
pub const CANVAS: f64 = 256.0;

/// One path segment. Deliberately tiny: it maps 1:1 to both SVG path
/// commands and `tiny-skia` `PathSegment`s.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    MoveTo(f64, f64),
    LineTo(f64, f64),
    /// Control point, then end point.
    QuadTo(f64, f64, f64, f64),
    Close,
}

/// A closed path plus its fill. `opacity` covers the pattern transparency
/// (1.0 for the panels).
#[derive(Debug, Clone)]
pub struct Shape {
    pub segments: Vec<Segment>,
    pub fill: Fill,
    pub opacity: f64,
}

/// How a shape is painted. A vertical gradient runs in user-space from the
/// shape's own top to its bottom (used for the front-panel highlight).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fill {
    Solid(super::colors::Rgb),
    VerticalGradient {
        color: super::colors::Rgb,
        top_alpha: f64,
        bottom_alpha: f64,
    },
}

/// One drawable layer. The pattern must be clipped to the front panel, so a
/// layer is either a plain shape or a group of shapes clipped to one path.
#[derive(Debug, Clone)]
pub enum Layer {
    Fill(Shape),
    /// Shapes drawn only where they intersect the clip path.
    ClippedGroup {
        clip: Vec<Segment>,
        shapes: Vec<Shape>,
    },
}

/// The complete drawing: everything both backends need, no user strings.
#[derive(Debug, Clone)]
pub struct Drawing {
    pub layers: Vec<Layer>,
}

/// Front panel rectangle (also the pattern clip region).
pub const FRONT: Rect = Rect {
    x: 24.0,
    y: 106.0,
    w: 208.0,
    h: 100.0,
};

/// Corner radius shared by both panels.
const PANEL_RADIUS: f64 = 14.0;

/// An axis-aligned rectangle.
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Pattern bar/square width in canvas px for the 5 size steps (thin →
/// thick). Stripes use it as the bar width with an equal gap (period = 2 ×
/// width); checkerboard uses it as the square side.
pub fn pattern_cell(size_step: i32) -> f64 {
    match size_step {
        1 => 4.0,
        2 => 6.0,
        3 => 9.0,
        4 => 13.0,
        _ => 18.0,
    }
}

/// Builds the complete drawing for the given colors. `pattern_cell_px` is
/// the bar/square width in canvas px (callers resolve the size step; the
/// rasterizer passes a larger, device-adjusted cell for small frames).
/// Pattern shapes carry `pattern_opacity` (0..=1) and are clipped to the
/// front panel.
pub fn build_drawing(
    base_color: super::colors::Rgb,
    back_color: super::colors::Rgb,
    pattern: super::settings::PatternKind,
    pattern_color: super::colors::Rgb,
    pattern_cell_px: f64,
    pattern_opacity: f64,
) -> Drawing {
    let front_path = rounded_rect(FRONT.x, FRONT.y, FRONT.w, FRONT.h, PANEL_RADIUS);
    let mut layers = vec![
        Layer::Fill(Shape {
            segments: back_panel(),
            fill: Fill::Solid(back_color),
            opacity: 1.0,
        }),
        Layer::Fill(Shape {
            segments: front_path.clone(),
            fill: Fill::Solid(base_color),
            opacity: 1.0,
        }),
    ];

    let cells = pattern_shapes(pattern, pattern_cell_px, pattern_color, pattern_opacity);
    if !cells.is_empty() {
        layers.push(Layer::ClippedGroup {
            clip: front_path.clone(),
            shapes: cells,
        });
    }

    // Subtle top highlight on the front panel: a white vertical fade.
    layers.push(Layer::Fill(Shape {
        segments: front_path,
        fill: Fill::VerticalGradient {
            color: super::colors::Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
            top_alpha: 0.20,
            bottom_alpha: 0.02,
        },
        opacity: 1.0,
    }));

    Drawing { layers }
}

/// The back panel with its tab, as ONE closed path (single fill = no seams
/// between tab and body). Clockwise from the tab's top-left corner.
fn back_panel() -> Vec<Segment> {
    let left = FRONT.x;
    let right = FRONT.x + FRONT.w;
    let bottom = FRONT.y + FRONT.h;
    let body_top = 70.0;
    let tab_top = 50.0;
    let tab_end = 98.0; // where the tab's top edge ends and slopes down
    let slope_end = 126.0; // where the slope meets the back body top edge

    let mut p = vec![Segment::MoveTo(left + 12.0, tab_top)];
    p.push(Segment::LineTo(tab_end, tab_top));
    p.push(Segment::QuadTo(
        tab_end + 14.0,
        tab_top,
        slope_end,
        body_top,
    ));
    p.push(Segment::LineTo(right - PANEL_RADIUS, body_top));
    p.push(Segment::QuadTo(
        right,
        body_top,
        right,
        body_top + PANEL_RADIUS,
    ));
    p.push(Segment::LineTo(right, bottom - PANEL_RADIUS));
    p.push(Segment::QuadTo(right, bottom, right - PANEL_RADIUS, bottom));
    p.push(Segment::LineTo(left + PANEL_RADIUS, bottom));
    p.push(Segment::QuadTo(left, bottom, left, bottom - PANEL_RADIUS));
    p.push(Segment::LineTo(left, tab_top + 12.0));
    p.push(Segment::QuadTo(left, tab_top, left + 12.0, tab_top));
    p.push(Segment::Close);
    p
}

/// One rounded rectangle as a closed subpath.
pub fn rounded_rect(x: f64, y: f64, w: f64, h: f64, r: f64) -> Vec<Segment> {
    let r = r.clamp(0.0, w.min(h) / 2.0);
    let right = x + w;
    let bottom = y + h;
    let mut p = vec![Segment::MoveTo(x + r, y)];
    p.push(Segment::LineTo(right - r, y));
    p.push(Segment::QuadTo(right, y, right, y + r));
    p.push(Segment::LineTo(right, bottom - r));
    p.push(Segment::QuadTo(right, bottom, right - r, bottom));
    p.push(Segment::LineTo(x + r, bottom));
    p.push(Segment::QuadTo(x, bottom, x, bottom - r));
    p.push(Segment::LineTo(x, y + r));
    p.push(Segment::QuadTo(x, y, x + r, y));
    p.push(Segment::Close);
    p
}

/// One solid-color rectangle as a closed subpath.
fn rect(x: f64, y: f64, w: f64, h: f64, color: super::colors::Rgb, opacity: f64) -> Shape {
    Shape {
        segments: vec![
            Segment::MoveTo(x, y),
            Segment::LineTo(x + w, y),
            Segment::LineTo(x + w, y + h),
            Segment::LineTo(x, y + h),
            Segment::Close,
        ],
        fill: Fill::Solid(color),
        opacity,
    }
}

/// The pattern cells covering the front panel's bounding box (the renderer
/// clips them to the panel's rounded shape). `cell_px` is the bar/square
/// width in canvas px. `None` yields no shapes.
pub fn pattern_shapes(
    pattern: super::settings::PatternKind,
    cell_px: f64,
    color: super::colors::Rgb,
    opacity: f64,
) -> Vec<Shape> {
    use super::settings::PatternKind;
    let mut shapes = Vec::new();
    let w = cell_px.max(0.5);
    match pattern {
        PatternKind::None => {}
        PatternKind::Vertical | PatternKind::Horizontal => {
            let vertical = pattern == PatternKind::Vertical;
            // Bars of width w with an equal gap, starting at the panel edge.
            let span = if vertical { FRONT.w } else { FRONT.h };
            let count = ((span / (2.0 * w)).ceil() as usize).max(1);
            for i in 0..count {
                let start = if vertical { FRONT.x } else { FRONT.y } + (i as f64) * 2.0 * w;
                shapes.push(if vertical {
                    rect(start, FRONT.y, w, FRONT.h, color, opacity)
                } else {
                    rect(FRONT.x, start, FRONT.w, w, color, opacity)
                });
            }
        }
        PatternKind::Checker => {
            let cols = (FRONT.w / w).ceil() as usize;
            let rows = (FRONT.h / w).ceil() as usize;
            for j in 0..rows {
                for i in 0..cols {
                    if (i + j) % 2 == 0 {
                        shapes.push(rect(
                            FRONT.x + (i as f64) * w,
                            FRONT.y + (j as f64) * w,
                            w,
                            w,
                            color,
                            opacity,
                        ));
                    }
                }
            }
        }
    }
    shapes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::folder_icon::colors::Rgb;
    use crate::tools::folder_icon::settings::PatternKind;

    const WHITE: Rgb = Rgb {
        r: 255,
        g: 255,
        b: 255,
    };
    const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };
    const RED: Rgb = Rgb {
        r: 232,
        g: 17,
        b: 35,
    };

    fn is_move(s: &Segment) -> bool {
        matches!(s, Segment::MoveTo(..))
    }
    fn is_close(s: &Segment) -> bool {
        matches!(s, Segment::Close)
    }

    #[test]
    fn drawing_has_the_expected_layers() {
        let drawing = build_drawing(WHITE, BLACK, PatternKind::None, RED, pattern_cell(3), 0.5);
        // Back, front, (no pattern group), highlight.
        assert_eq!(drawing.layers.len(), 3);
        assert!(matches!(drawing.layers[0], Layer::Fill(..)));
        assert!(matches!(drawing.layers[1], Layer::Fill(..)));
        assert!(matches!(drawing.layers[2], Layer::Fill(..)));

        let drawing = build_drawing(
            WHITE,
            BLACK,
            PatternKind::Checker,
            RED,
            pattern_cell(3),
            0.5,
        );
        assert_eq!(drawing.layers.len(), 4);
        assert!(matches!(&drawing.layers[2], Layer::ClippedGroup { .. }));
    }

    #[test]
    fn panels_are_closed_single_paths() {
        let drawing = build_drawing(WHITE, BLACK, PatternKind::None, RED, pattern_cell(1), 0.5);
        for layer in &drawing.layers {
            let Layer::Fill(shape) = layer else {
                continue;
            };
            assert!(matches!(shape.segments[0], Segment::MoveTo(..)));
            assert!(matches!(shape.segments.last(), Some(Segment::Close)));
            assert_eq!(shape.segments.iter().filter(|s| is_move(s)).count(), 1);
            assert_eq!(shape.segments.iter().filter(|s| is_close(s)).count(), 1);
        }
    }

    #[test]
    fn all_coordinates_stay_inside_the_canvas() {
        for step in 1..=5 {
            for kind in [
                PatternKind::None,
                PatternKind::Vertical,
                PatternKind::Horizontal,
                PatternKind::Checker,
            ] {
                let drawing = build_drawing(WHITE, BLACK, kind, RED, pattern_cell(step), 0.5);
                for layer in &drawing.layers {
                    let shapes: Vec<&Shape> = match layer {
                        Layer::Fill(shape) => vec![shape],
                        Layer::ClippedGroup { clip, shapes } => {
                            clip.iter().for_each(|_| {});
                            shapes.iter().collect()
                        }
                    };
                    for shape in shapes {
                        for segment in &shape.segments {
                            match *segment {
                                Segment::MoveTo(x, y)
                                | Segment::LineTo(x, y)
                                | Segment::QuadTo(_, _, x, y) => {
                                    assert!(
                                        (0.0..=CANVAS).contains(&x) && (0.0..=CANVAS).contains(&y),
                                        "coordinate {x},{y} outside canvas"
                                    );
                                }
                                Segment::Close => {}
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn pattern_shape_counts_scale_with_size() {
        let counts: Vec<usize> = (1..=5)
            .map(|step| pattern_shapes(PatternKind::Vertical, pattern_cell(step), RED, 0.5).len())
            .collect();
        // Thicker bars mean fewer bars.
        for pair in counts.windows(2) {
            assert!(pair[0] > pair[1], "counts descending: {counts:?}");
        }
        // Thin vertical bars at the front panel width of 208, period 8.
        assert_eq!(counts[0], (208.0_f64 / 8.0).ceil() as usize);

        let checkers: Vec<usize> = (1..=5)
            .map(|step| pattern_shapes(PatternKind::Checker, pattern_cell(step), RED, 0.5).len())
            .collect();
        for pair in checkers.windows(2) {
            assert!(pair[0] > pair[1], "checker counts descending: {checkers:?}");
        }

        assert!(pattern_shapes(PatternKind::None, pattern_cell(3), RED, 0.5).is_empty());
    }

    #[test]
    fn horizontal_pattern_is_transposed_vertical() {
        let v = pattern_shapes(PatternKind::Vertical, pattern_cell(3), RED, 0.5);
        let h = pattern_shapes(PatternKind::Horizontal, pattern_cell(3), RED, 0.5);
        // The panel is 208 wide but only 100 tall, so the bar COUNTS differ;
        // what must match is the bar thickness and the covering orientation.
        // Corner 2 is (x+w, y), corner 3 is (x+w, y+h).
        let (Some(Segment::LineTo(v1x, _)), Some(Segment::LineTo(_v2x, v2y))) =
            (v[0].segments.get(1), v[0].segments.get(2))
        else {
            panic!("expected rect corners");
        };
        // Vertical bars: thin in x (9 px), full height in y.
        assert!(((v1x - FRONT.x) - 9.0).abs() < 1e-9);
        assert!(((v2y - FRONT.y) - FRONT.h).abs() < 1e-9);
        // Horizontal bars: full width in x, thin in y.
        let (Some(Segment::LineTo(h1x, _)), Some(Segment::LineTo(_h2x, h2y))) =
            (h[0].segments.get(1), h[0].segments.get(2))
        else {
            panic!("expected rect corners");
        };
        assert!(((h1x - FRONT.x) - FRONT.w).abs() < 1e-9);
        assert!(((h2y - FRONT.y) - 9.0).abs() < 1e-9);
    }

    #[test]
    fn checker_alternates_cells() {
        let shapes = pattern_shapes(PatternKind::Checker, pattern_cell(5), RED, 1.0);
        // With w=18 over 208×100: cols=12, rows=6, half filled = 36.
        assert_eq!(shapes.len(), 36);
        // Opacity and color flow through.
        assert_eq!(shapes[0].opacity, 1.0);
        assert_eq!(shapes[0].fill, Fill::Solid(RED));
    }

    #[test]
    fn pattern_opacity_flows_through() {
        let shapes = pattern_shapes(PatternKind::Vertical, pattern_cell(3), RED, 0.37);
        assert!(shapes.iter().all(|s| (s.opacity - 0.37).abs() < 1e-9));
    }

    #[test]
    fn pattern_cell_covers_all_steps() {
        for step in 1..=5 {
            assert!(pattern_cell(step) > 0.0);
        }
        assert!(pattern_cell(0) > 0.0 && pattern_cell(9) > 0.0); // total match arms
    }
}
