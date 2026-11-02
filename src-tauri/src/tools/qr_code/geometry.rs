//! Turns a QR matrix into drawing geometry: a small set of fill paths that
//! both backends (SVG string writer and PNG rasterizer) consume, so the
//! preview, the SVG file and the PNG file are always drawn from exactly the
//! same shapes.
//!
//! Coordinate system: one module = one unit, with the quiet-zone margin
//! already included as an offset. Sharp style draws every dark module as a
//! square merged into ONE path (no hairline seams between neighbors);
//! rounded style additionally rounds each module corner where both
//! orthogonal neighbors touching that corner are light, and replaces the
//! three finder patterns with smooth rounded shapes.

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

/// Fill rule for a shape. `EvenOdd` is only used for finder rings (outer
/// square minus hole).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

/// One closed fill shape in module coordinates.
#[derive(Debug, Clone)]
pub struct Shape {
    pub segments: Vec<Segment>,
    pub fill_rule: FillRule,
}

/// Visual style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QrStyle {
    Sharp,
    Round,
}

impl QrStyle {
    /// Maps the UI token; anything unknown is an error.
    pub fn from_ui(value: &str) -> Result<Self, String> {
        match value {
            "sharp" => Ok(QrStyle::Sharp),
            "round" => Ok(QrStyle::Round),
            other => Err(format!("Unknown style: {other}")),
        }
    }
}

/// Area in module coordinates (inside the QR grid, without margin) whose
/// dark modules are removed — used to clear space for a centered logo.
#[derive(Debug, Clone, Copy)]
pub struct ClearRect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl ClearRect {
    /// True when the module with the given top-left corner lies fully
    /// inside the area. Modules are only removed whole, so partially
    /// covered edge modules stay and keep the outline clean.
    fn contains_module(&self, x: usize, y: usize) -> bool {
        let (mx, my) = (x as f64, y as f64);
        mx >= self.x0 && my >= self.y0 && (mx + 1.0) <= self.x1 && (my + 1.0) <= self.y1
    }
}

/// Corner rounding radius for module corners, in module units. Half a
/// module is the maximum that keeps opposite corners from overlapping.
const MODULE_ROUND: f64 = 0.5;

/// Builds all fill shapes for the code: background first (as plain rect
/// coordinates, handled by the backends), then the module path and, for
/// rounded style, the three finder-pattern shapes.
pub fn build_shapes(
    matrix: &super::matrix::Matrix,
    style: QrStyle,
    margin: usize,
    clear: Option<ClearRect>,
) -> Vec<Shape> {
    let offset = margin as f64;
    let mut shapes = Vec::with_capacity(4);

    let mut modules: Vec<Segment> = Vec::new();
    let size = matrix.size();
    for y in 0..size {
        for x in 0..size {
            if !matrix.is_dark(x as isize, y as isize) {
                continue;
            }
            if clear.is_some_and(|area| area.contains_module(x, y)) {
                continue;
            }
            // Rounded style replaces the three 7x7 finder blocks with
            // smooth shapes drawn below.
            if style == QrStyle::Round && is_finder_block(x, y, size) {
                continue;
            }
            let radii = match style {
                QrStyle::Sharp => [0.0; 4],
                QrStyle::Round => corner_radii(matrix, x, y),
            };
            append_closed_path(
                &mut modules,
                module_corners(x as f64 + offset, y as f64 + offset),
                radii,
            );
        }
    }
    if !modules.is_empty() {
        shapes.push(Shape {
            segments: modules,
            fill_rule: FillRule::NonZero,
        });
    }

    if style == QrStyle::Round && size >= 7 {
        for (fx, fy) in finder_origins(size) {
            // Outer ring: outer rounded square with an inner hole
            // (even-odd), plus the solid center block. The ring keeps the
            // 1-module stroke width and the 3x3 center intact, so the
            // finder proportions the scanners rely on stay unchanged.
            // Radii are scan-tested (see read-back tests): the outer
            // radius must stay small — 1.5 already makes some decoders
            // misread the format info around the eyes at some sizes, while
            // 1.25 passes every level and size we test.
            let mut ring = rounded_rect(fx as f64 + offset, fy as f64 + offset, 7.0, 7.0, 1.25);
            ring.extend(rounded_rect(
                fx as f64 + offset + 1.0,
                fy as f64 + offset + 1.0,
                5.0,
                5.0,
                0.25,
            ));
            shapes.push(Shape {
                segments: ring,
                fill_rule: FillRule::EvenOdd,
            });
            shapes.push(Shape {
                segments: rounded_rect(
                    fx as f64 + offset + 2.0,
                    fy as f64 + offset + 2.0,
                    3.0,
                    3.0,
                    1.0,
                ),
                fill_rule: FillRule::NonZero,
            });
        }
    }

    shapes
}

/// Where and how to draw the centered logo, in module coordinates of the
/// total canvas. Both backends consume the same prepared PNG.
#[derive(Debug, Clone, PartialEq)]
pub struct LogoDraw {
    /// Top-left corner in module coordinates.
    pub x: f64,
    pub y: f64,
    /// Logo side in module coordinates.
    pub size: f64,
    /// Plain base64 of the prepared PNG (no data-URL prefix).
    pub png_base64: String,
}

/// Top-left corners of the three 7x7 finder blocks.
fn finder_origins(size: usize) -> [(usize, usize); 3] {
    [(0, 0), (size - 7, 0), (0, size - 7)]
}

/// True when (x, y) lies inside one of the three 7x7 finder blocks.
fn is_finder_block(x: usize, y: usize, size: usize) -> bool {
    if size < 7 {
        return false;
    }
    finder_origins(size)
        .iter()
        .any(|&(fx, fy)| x >= fx && x < fx + 7 && y >= fy && y < fy + 7)
}

/// Rounding radius for each module corner (TL, TR, BR, BL): rounded only
/// when BOTH orthogonal neighbors touching that corner are light (out of
/// the grid counts as light), so touching dark modules stay joined.
fn corner_radii(matrix: &super::matrix::Matrix, x: usize, y: usize) -> [f64; 4] {
    let (x, y) = (x as isize, y as isize);
    let dark = |dx: isize, dy: isize| matrix.is_dark(x + dx, y + dy);
    [
        if dark(-1, 0) || dark(0, -1) {
            0.0
        } else {
            MODULE_ROUND
        }, // TL
        if dark(1, 0) || dark(0, -1) {
            0.0
        } else {
            MODULE_ROUND
        }, // TR
        if dark(1, 0) || dark(0, 1) {
            0.0
        } else {
            MODULE_ROUND
        }, // BR
        if dark(-1, 0) || dark(0, 1) {
            0.0
        } else {
            MODULE_ROUND
        }, // BL
    ]
}

/// The four corners of one module in clockwise order.
fn module_corners(x: f64, y: f64) -> [(f64, f64); 4] {
    [(x, y), (x + 1.0, y), (x + 1.0, y + 1.0), (x, y + 1.0)]
}

/// Appends one closed path for a rectangle given its four corners (TL, TR,
/// BR, BL, clockwise) and per-corner radii (same order, 0 = square).
///
/// Walking the corners in order, each rounded corner emits a line to the
/// point `r` before it and a quadratic curve (control = the corner) to the
/// point `r` after it; square corners emit a plain line. The very first
/// emitted point becomes the MoveTo, so callers can append many subpaths
/// into one segment list.
fn append_closed_path(out: &mut Vec<Segment>, corners: [(f64, f64); 4], radii: [f64; 4]) {
    let before = out.len();
    for i in 0..4 {
        let (cx, cy) = corners[i];
        let (px, py) = corners[(i + 3) % 4]; // incoming edge neighbor
        let (nx, ny) = corners[(i + 1) % 4]; // outgoing edge neighbor
        let r = radii[i]; // callers pass sane radii (0..=half edge)
        if r <= 0.0 {
            out.push(Segment::LineTo(cx, cy));
        } else {
            // Edge directions are axis-aligned: the neighbor differs from
            // the corner on exactly one axis, and stepping must happen on
            // that axis only (f64::signum would move +r even for a zero
            // delta, so compare explicitly).
            let towards = |corner: f64, neighbor: f64| {
                if neighbor == corner {
                    corner
                } else {
                    corner + (neighbor - corner).signum() * r
                }
            };
            out.push(Segment::LineTo(towards(cx, px), towards(cy, py)));
            out.push(Segment::QuadTo(cx, cy, towards(cx, nx), towards(cy, ny)));
        }
    }
    // Rewrite the first emitted point as the subpath start.
    if let Segment::LineTo(x, y) = out[before] {
        out[before] = Segment::MoveTo(x, y);
    }
    out.push(Segment::Close);
}

/// One rounded rectangle as a closed subpath, appended to `out`.
fn rounded_rect(x: f64, y: f64, w: f64, h: f64, r: f64) -> Vec<Segment> {
    let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
    let r = r.clamp(0.0, w.min(h) / 2.0);
    let mut out = Vec::with_capacity(10);
    append_closed_path(&mut out, corners, [r; 4]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::qr_code::matrix::{Ecc, Matrix};

    fn count_kind(segments: &[Segment], kind: fn(&Segment) -> bool) -> usize {
        segments.iter().filter(|s| kind(s)).count()
    }

    const IS_MOVE: fn(&Segment) -> bool = |s| matches!(s, Segment::MoveTo(..));
    const IS_QUAD: fn(&Segment) -> bool = |s| matches!(s, Segment::QuadTo(..));
    const IS_CLOSE: fn(&Segment) -> bool = |s| matches!(s, Segment::Close);

    /// Synthetic matrix with only the listed dark modules.
    fn grid(size: usize, dark: &[(usize, usize)]) -> Matrix {
        let mut cells = vec![false; size * size];
        for &(x, y) in dark {
            cells[y * size + x] = true;
        }
        Matrix::from_dark_grid(size, cells)
    }

    #[test]
    fn sharp_style_draws_one_merged_path() {
        let matrix = grid(3, &[(0, 0), (1, 1)]);
        let shapes = build_shapes(&matrix, QrStyle::Sharp, 4, None);
        // Sharp style: exactly one shape, all corners square (no quads).
        assert_eq!(shapes.len(), 1);
        assert_eq!(shapes[0].fill_rule, FillRule::NonZero);
        assert_eq!(count_kind(&shapes[0].segments, IS_QUAD), 0);
        // One MoveTo + Close per dark module.
        assert_eq!(count_kind(&shapes[0].segments, IS_MOVE), 2);
        assert_eq!(count_kind(&shapes[0].segments, IS_CLOSE), 2);
    }

    #[test]
    fn isolated_module_gets_all_corners_rounded() {
        let matrix = grid(3, &[(1, 1)]);
        assert_eq!(corner_radii(&matrix, 1, 1), [0.5, 0.5, 0.5, 0.5]);
        // Its subpath: MoveTo, 4 quads, Close.
        let shapes = build_shapes(&matrix, QrStyle::Round, 0, None);
        assert_eq!(shapes.len(), 1); // too small for finder shapes
        let segments = &shapes[0].segments;
        assert!(matches!(segments[0], Segment::MoveTo(..)));
        assert_eq!(count_kind(segments, IS_QUAD), 4);
        assert_eq!(count_kind(segments, IS_CLOSE), 1);
    }

    #[test]
    fn adjacent_modules_keep_shared_edge_square() {
        // Two horizontally adjacent dark modules.
        let matrix = grid(2, &[(0, 0), (1, 0)]);
        // Left module: top-left and bottom-left rounded; right-side
        // corners square because the right neighbor is dark.
        assert_eq!(corner_radii(&matrix, 0, 0), [0.5, 0.0, 0.0, 0.5]);
        // Right module: mirrored.
        assert_eq!(corner_radii(&matrix, 1, 0), [0.0, 0.5, 0.5, 0.0]);

        // Out of the grid counts as light: a module in the grid corner is
        // fully rounded.
        let single = grid(1, &[(0, 0)]);
        assert_eq!(corner_radii(&single, 0, 0), [0.5, 0.5, 0.5, 0.5]);
    }

    #[test]
    fn diagonal_and_edge_neighbors_round_normally() {
        // Vertical pair shares left/right edges, not top/bottom corners.
        let matrix = grid(2, &[(0, 0), (0, 1)]);
        // Top module: TL/TR rounded (top light), BR square (bottom dark),
        // BL square (bottom dark).
        assert_eq!(corner_radii(&matrix, 0, 0), [0.5, 0.5, 0.0, 0.0]);
        // Diagonal-only neighbors never block rounding: module (0,0) with
        // only a dark (1,1) neighbor still has all four corners rounded.
        let diag = grid(2, &[(0, 0), (1, 1)]);
        assert_eq!(corner_radii(&diag, 0, 0), [0.5, 0.5, 0.5, 0.5]);
    }

    #[test]
    fn rounded_style_excludes_finder_modules_and_adds_shapes() {
        let matrix = Matrix::from_text("ab", Ecc::Low).expect("fits");
        let size = matrix.size();
        let sharp = build_shapes(&matrix, QrStyle::Sharp, 4, None);
        let round = build_shapes(&matrix, QrStyle::Round, 4, None);
        // Sharp: 1 shape. Round: modules + 3 rings + 3 centers.
        assert_eq!(sharp.len(), 1);
        assert_eq!(round.len(), 7);
        assert_eq!(round[1].fill_rule, FillRule::EvenOdd);
        assert_eq!(round[2].fill_rule, FillRule::NonZero);

        // MoveTos: sharp has one per dark module; round's module path has
        // one per dark module outside the finder blocks.
        let moves = |shape: &Shape| count_kind(&shape.segments, IS_MOVE);
        let dark_total: usize = sharp.iter().map(moves).sum();
        let dark_round: usize = moves(&round[0]);
        let finder_modules = finder_origins(size)
            .iter()
            .flat_map(|&(fx, fy)| (fy..fy + 7).flat_map(move |y| (fx..fx + 7).map(move |x| (x, y))))
            .filter(|&(x, y)| matrix.is_dark(x as isize, y as isize))
            .count();
        assert_eq!(dark_total - dark_round, finder_modules);
    }

    #[test]
    fn margin_offsets_all_geometry() {
        let matrix = grid(3, &[(1, 1)]);
        let with_margin = build_shapes(&matrix, QrStyle::Sharp, 4, None);
        let no_margin = build_shapes(&matrix, QrStyle::Sharp, 0, None);
        let first_move = |shapes: &[Shape]| match shapes[0].segments[0] {
            Segment::MoveTo(x, y) => (x, y),
            _ => panic!("expected MoveTo first"),
        };
        let (x4, y4) = first_move(&with_margin);
        let (x0, y0) = first_move(&no_margin);
        assert!(((x4 - x0) - 4.0).abs() < 1e-9);
        assert!(((y4 - y0) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn clear_rect_removes_whole_covered_modules() {
        // 5x5 all dark; clear the 2x2 area [2,4)x[2,4) -> 4 modules gone.
        let matrix = grid(
            5,
            &[
                (0, 0),
                (1, 0),
                (2, 0),
                (3, 0),
                (4, 0), //
                (0, 1),
                (1, 1),
                (2, 1),
                (3, 1),
                (4, 1), //
                (0, 2),
                (1, 2),
                (2, 2),
                (3, 2),
                (4, 2), //
                (0, 3),
                (1, 3),
                (2, 3),
                (3, 3),
                (4, 3), //
                (0, 4),
                (1, 4),
                (2, 4),
                (3, 4),
                (4, 4),
            ],
        );
        let clear = ClearRect {
            x0: 2.0,
            y0: 2.0,
            x1: 4.0,
            y1: 4.0,
        };
        let with = build_shapes(&matrix, QrStyle::Sharp, 0, Some(clear));
        let without = build_shapes(&matrix, QrStyle::Sharp, 0, None);
        let dark_count = |shapes: &[Shape]| {
            shapes
                .iter()
                .map(|s| count_kind(&s.segments, IS_MOVE))
                .sum::<usize>()
        };
        assert_eq!(dark_count(&without), 25);
        assert_eq!(dark_count(&with), 21);
        // A partially covered edge module stays (modules are removed whole).
        let partial = ClearRect {
            x0: 4.5,
            y0: 4.5,
            x1: 5.5,
            y1: 5.5,
        };
        assert_eq!(
            dark_count(&build_shapes(&matrix, QrStyle::Sharp, 0, Some(partial))),
            25
        );
    }

    #[test]
    fn path_from_corners_starts_with_move_and_stays_in_bounds() {
        let mut out = Vec::new();
        append_closed_path(
            &mut out,
            [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
            [0.5; 4],
        );
        assert!(matches!(out[0], Segment::MoveTo(..)));
        assert_eq!(count_kind(&out, IS_QUAD), 4);
        assert_eq!(count_kind(&out, IS_CLOSE), 1);
        for segment in &out {
            match *segment {
                Segment::MoveTo(x, y) | Segment::LineTo(x, y) => {
                    assert!(
                        (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y),
                        "out of bounds: {x},{y}"
                    );
                }
                Segment::QuadTo(cx, cy, _, _) => {
                    assert!(
                        (0.0..=1.0).contains(&cx) && (0.0..=1.0).contains(&cy),
                        "ctrl out of bounds: {cx},{cy}"
                    );
                }
                Segment::Close => {}
            }
        }
    }

    #[test]
    fn square_path_has_no_quads() {
        let rect = rounded_rect(0.0, 0.0, 7.0, 7.0, 0.0);
        assert_eq!(count_kind(&rect, IS_QUAD), 0);
        assert!(matches!(rect[0], Segment::MoveTo(0.0, 0.0)));
        // Radius is clamped to half the shorter side: the path starts
        // half-way up the left edge and all edges are 3 units.
        let clamped = rounded_rect(0.0, 0.0, 3.0, 3.0, 5.0);
        assert!(matches!(clamped[0], Segment::MoveTo(0.0, 1.5)));
    }
}
