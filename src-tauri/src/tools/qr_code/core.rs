//! The pure build pipeline shared by every command: validate all inputs,
//! build the payload, generate the matrix, render the geometry and SVG.
//! No Tauri types here, so the whole pipeline is unit testable.

use super::colors::{self, Rgb};
use super::geometry::{self, ClearRect, LogoDraw, LogoPlate, QrStyle, Shape};
use super::matrix::{self, Ecc, Matrix};
use super::payload::{self, WifiOptions, WifiSecurity};
use super::raster;
use super::svg;

/// Quiet-zone margin in modules: the allowed range (the default of 4 is
/// the frontend's concern; Rust only validates the range).
pub const MIN_MARGIN: usize = 1;
pub const MAX_MARGIN: usize = 10;

/// Requested PNG export sizes: the allowed range (default 1080 lives in
/// the UI).
pub const MIN_PNG_SIZE: i32 = raster::MIN_PNG_SIZE as i32;
pub const MAX_PNG_SIZE: i32 = raster::MAX_PNG_SIZE as i32;

/// Logo size as a fraction of the total QR width: the allowed range (the
/// UI slider stays inside it) and the fraction at which the "near the
/// maximum" warning shows.
pub const MIN_LOGO_FRACTION: f64 = 0.05;
pub const MAX_LOGO_FRACTION: f64 = 0.20;
pub const LOGO_NEAR_MAX: f64 = 0.18;
/// One module of extra clearing on every side around the logo.
pub const LOGO_CLEAR_PADDING_MODULES: f64 = 1.0;
/// Corner radius of the cleared area (and the plate drawn over it) as a
/// fraction of its side, Rounded style only; clamped to at most half the
/// side so opposite corners can never overlap.
pub const LOGO_PLATE_RADIUS_FRACTION: f64 = 0.22;

/// What the user wants to encode, as raw (unvalidated) UI input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentInput {
    Text(String),
    Website(String),
    Wifi {
        ssid: String,
        password: String,
        security: String,
        hidden: bool,
    },
}

impl ContentInput {
    /// Builds the exact string that gets encoded into the QR matrix.
    pub fn payload(&self) -> Result<String, String> {
        match self {
            ContentInput::Text(text) => {
                if payload::is_blank(text) {
                    Err("Enter some text first.".into())
                } else {
                    Ok(payload::text_payload(text))
                }
            }
            ContentInput::Website(url) => payload::normalize_url(url),
            ContentInput::Wifi {
                ssid,
                password,
                security,
                hidden,
            } => {
                let security = WifiSecurity::from_ui(security)?;
                payload::wifi_payload(&WifiOptions {
                    ssid: ssid.clone(),
                    password: password.clone(),
                    security,
                    hidden: *hidden,
                })
            }
        }
    }
}

/// An optional centered logo, as sent back by the UI (which got the
/// prepared PNG from the `load_logo` command).
#[derive(Debug, Clone, PartialEq)]
pub struct LogoInput {
    /// Plain base64 of the prepared PNG (no data-URL prefix).
    pub png_base64: String,
    /// Logo side as a fraction of the total QR width (0.05..=0.20).
    pub fraction: f64,
}

/// All options for one QR build, straight from the UI (unvalidated).
#[derive(Debug, Clone)]
pub struct BuildRequest {
    pub content: ContentInput,
    pub style: String,
    /// Strict `#RRGGBB`.
    pub foreground: String,
    /// Strict `#RRGGBB`; ignored when the background is transparent.
    pub background: String,
    pub transparent: bool,
    /// "L", "M", "Q" or "H".
    pub error_correction: String,
    pub margin: i32,
    /// Requested PNG side length in pixels (used exactly for the export).
    pub png_size: i32,
    pub logo: Option<LogoInput>,
}

/// The result of one successful build.
#[derive(Debug, Clone)]
pub struct BuiltQr {
    /// Complete SVG document (also used verbatim for the .svg export).
    pub svg: String,
    /// The fill shapes behind that SVG — the PNG rasterizer draws exactly
    /// these, so both exports always match.
    pub shapes: Vec<Shape>,
    pub foreground: Rgb,
    pub background: Option<Rgb>,
    /// The logo overlay for both backends, if any.
    pub logo: Option<LogoDraw>,
    /// The rounded plate drawn behind the logo (Rounded style with an
    /// opaque background only), in canvas coordinates; both renderers
    /// draw it over the modules and under the logo.
    pub plate: Option<LogoPlate>,
    /// QR side length in modules (without the quiet zone).
    pub size_modules: usize,
    /// QR side length including both margins.
    pub total_modules: usize,
    /// Side length of the saved PNG (the requested size, used exactly).
    pub png_pixels: u32,
    /// Level actually used (differs from the request when a logo forced H).
    pub level_used: Ecc,
    /// Payload length in characters, for the live counter.
    pub char_count: usize,
    /// Characters that would fit at this level, for the live counter.
    pub max_chars: usize,
    /// Non-blocking color/contrast warnings for the UI.
    pub warnings: Vec<String>,
}

/// Runs the full pipeline: payload → matrix → geometry → SVG.
pub fn build(request: &BuildRequest) -> Result<BuiltQr, String> {
    let payload_text = request.content.payload()?;
    let style = QrStyle::from_ui(&request.style)?;
    let foreground = Rgb::parse(&request.foreground)?;
    let background = if request.transparent {
        None
    } else {
        Some(Rgb::parse(&request.background)?)
    };
    // A logo covers the center, so error correction is forced to H — the
    // only level with enough spare codewords for that.
    let logo = match &request.logo {
        Some(input) => {
            validate_logo(input)?;
            Some(input)
        }
        None => None,
    };
    let ecc = if logo.is_some() {
        Ecc::High
    } else {
        Ecc::from_ui(&request.error_correction)?
    };
    let margin = validate_margin(request.margin)?;
    let png_size = validate_png_size(request.png_size)?;
    if payload_text.is_empty() {
        return Err("Enter some content first.".into());
    }

    let matrix = Matrix::from_text(&payload_text, ecc)
        .map_err(|too_long| matrix::too_long_message(too_long.max_chars))?;
    let total_modules = matrix.size() + 2 * margin;

    // The logo rect lives in CANVAS coordinates (the rendered square with
    // the quiet zone; its center is total_modules/2). The cleared module
    // area is slightly larger than the logo (one module of padding on
    // every side) and lives in GRID coordinates (the matrix alone, origin
    // at its top-left module), because build_shapes checks modules by
    // their grid positions. Both centers are the same point of the code:
    // size/2 + margin = total/2. With a background the cleared area fills
    // with the background color automatically; transparent backgrounds
    // simply stay empty behind the logo.
    let logo_draw = logo.map(|input| {
        let size = input.fraction * total_modules as f64;
        let center = total_modules as f64 / 2.0;
        LogoDraw {
            x: center - size / 2.0,
            y: center - size / 2.0,
            size,
            png_base64: input.png_base64.clone(),
        }
    });
    let clear = logo_draw
        .as_ref()
        .map(|draw| logo_clear_rect(draw.size, matrix.size(), style));

    // Rounded style with an opaque background: the plate is the cleared
    // area's rounded rectangle shifted into canvas coordinates (+ margin),
    // drawn by both renderers over the modules and under the logo so the
    // hole's edge is clean and really rounded. Transparent backgrounds get
    // no plate (a plate would paint a color that does not exist); square
    // styles keep the plain hole with no plate at all.
    let plate = match (style, background) {
        (QrStyle::Round, Some(_)) => clear.as_ref().map(|area| LogoPlate {
            x: area.x0 + margin as f64,
            y: area.y0 + margin as f64,
            size: area.x1 - area.x0,
            radius: area.radius,
        }),
        _ => None,
    };

    let shapes = geometry::build_shapes(&matrix, style, margin, clear);
    let svg_text = svg::render(
        &shapes,
        total_modules as f64,
        foreground,
        background,
        plate.as_ref(),
        logo_draw.as_ref(),
    );

    let mut warnings = colors::color_warnings(foreground, background);
    if request
        .logo
        .as_ref()
        .is_some_and(|input| input.fraction >= LOGO_NEAR_MAX)
    {
        warnings.push(
            "The logo is close to the maximum size. Test scanning with your phone before using it."
                .into(),
        );
    }

    Ok(BuiltQr {
        char_count: payload_text.chars().count(),
        max_chars: matrix::max_fitting_chars(&payload_text, ecc),
        svg: svg_text,
        shapes,
        foreground,
        background,
        logo: logo_draw,
        plate,
        size_modules: matrix.size(),
        total_modules,
        png_pixels: png_size,
        level_used: ecc,
        warnings,
    })
}

/// The cleared area around a centered logo, in GRID coordinates: the
/// matrix's top-left module is (0,0) and the quiet-zone margin is NOT
/// included (canvas coordinates — used by `LogoDraw` — shift everything
/// by `margin`). `build_shapes` checks modules by their grid positions,
/// so this rect must use the same system: centered on the GRID center
/// (`matrix_size / 2`) it lands exactly under the logo, whose canvas
/// center is `total_modules / 2 = matrix_size / 2 + margin`.
///
/// Rounded style rounds the area's corners (`LOGO_PLATE_RADIUS_FRACTION`
/// of the side); square styles keep the plain rectangle.
fn logo_clear_rect(logo_size: f64, matrix_size: usize, style: QrStyle) -> ClearRect {
    let side = logo_size + 2.0 * LOGO_CLEAR_PADDING_MODULES;
    let center = matrix_size as f64 / 2.0;
    let radius = match style {
        QrStyle::Round => (side * LOGO_PLATE_RADIUS_FRACTION).min(side / 2.0),
        QrStyle::Sharp => 0.0,
    };
    ClearRect {
        x0: center - side / 2.0,
        y0: center - side / 2.0,
        x1: center + side / 2.0,
        y1: center + side / 2.0,
        radius,
    }
}

/// Validates the logo input from the UI (never trusted): the fraction must
/// be in range and the base64 must decode into a real PNG of the exact
/// size the `load_logo` command produces.
fn validate_logo(input: &LogoInput) -> Result<(), String> {
    if !(MIN_LOGO_FRACTION..=MAX_LOGO_FRACTION).contains(&input.fraction) {
        return Err(format!(
            "The logo size must be between {}% and {}% of the QR width.",
            (MIN_LOGO_FRACTION * 100.0) as u32,
            (MAX_LOGO_FRACTION * 100.0) as u32
        ));
    }
    let bytes = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        &input.png_base64,
    )
    .map_err(|_| "The logo image could not be read.".to_string())?;
    let decoded = image::load_from_memory(&bytes)
        .map_err(|_| "The logo image could not be read.".to_string())?;
    let (width, height) = (decoded.width(), decoded.height());
    if width != super::logo::LOGO_CANVAS_EDGE || height != super::logo::LOGO_CANVAS_EDGE {
        return Err("The logo image has an unexpected size.".to_string());
    }
    Ok(())
}

/// Validates the quiet-zone margin range.
pub fn validate_margin(margin: i32) -> Result<usize, String> {
    if margin < MIN_MARGIN as i32 || margin > MAX_MARGIN as i32 {
        return Err(format!(
            "The margin must be between {MIN_MARGIN} and {MAX_MARGIN} modules. Small margins can hurt scanning."
        ));
    }
    Ok(margin as usize)
}

/// Validates the requested PNG export size.
pub fn validate_png_size(size: i32) -> Result<u32, String> {
    if !(MIN_PNG_SIZE..=MAX_PNG_SIZE).contains(&size) {
        return Err(format!(
            "The PNG size must be between {MIN_PNG_SIZE} and {MAX_PNG_SIZE} pixels."
        ));
    }
    Ok(size as u32)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::geometry::Segment;
    use super::*;

    fn text_request(text: &str) -> BuildRequest {
        BuildRequest {
            content: ContentInput::Text(text.into()),
            style: "sharp".into(),
            foreground: "#000000".into(),
            background: "#ffffff".into(),
            transparent: false,
            error_correction: "Q".into(),
            margin: 4,
            png_size: 1080,
            logo: None,
        }
    }

    #[test]
    fn builds_svg_with_defaults() {
        let built = build(&text_request("Hello")).expect("builds");
        assert!(built.svg.starts_with("<svg"));
        assert_eq!(built.level_used, Ecc::Quartile);
        assert!(built.size_modules >= 21);
        assert_eq!(built.total_modules, built.size_modules + 8);
        assert_eq!(built.char_count, 5);
        assert!(built.max_chars >= built.char_count);
        // Black on white: no warnings.
        assert!(built.warnings.is_empty());
    }

    #[test]
    fn blank_text_is_a_friendly_error() {
        assert_eq!(
            build(&text_request("   ")).unwrap_err(),
            "Enter some text first."
        );
    }

    #[test]
    fn website_payload_is_normalized() {
        let request = BuildRequest {
            content: ContentInput::Website("example.com".into()),
            ..text_request("x")
        };
        let built = build(&request).expect("builds");
        assert_eq!(built.char_count, "https://example.com".chars().count());
    }

    #[test]
    fn wifi_payload_goes_through_rust_escaping() {
        let content = ContentInput::Wifi {
            ssid: "net;work".into(),
            password: "p:a,s\\s".into(),
            security: "wpa".into(),
            hidden: true,
        };
        let request = BuildRequest {
            content: content.clone(),
            ..text_request("x")
        };
        let built = build(&request).expect("builds");
        // The counter matches the exact payload that was encoded.
        assert_eq!(built.char_count, content.payload().unwrap().chars().count());
    }

    #[test]
    fn invalid_options_give_friendly_errors() {
        let mut request = text_request("hi");
        request.style = "dotted".into();
        assert_eq!(build(&request).unwrap_err(), "Unknown style: dotted");

        let mut request = text_request("hi");
        request.foreground = "red".into();
        assert!(build(&request).unwrap_err().contains("#RRGGBB"));

        let mut request = text_request("hi");
        request.error_correction = "X".into();
        assert_eq!(
            build(&request).unwrap_err(),
            "Unknown error correction level: X"
        );

        let mut request = text_request("hi");
        request.margin = 0;
        assert!(build(&request).unwrap_err().contains("between 1 and 10"));
        request.margin = 11;
        assert!(build(&request).unwrap_err().contains("between 1 and 10"));
    }

    #[test]
    fn transparent_background_warns() {
        let mut request = text_request("hi");
        request.transparent = true;
        let built = build(&request).expect("builds");
        assert!(built.warnings.iter().any(|w| w.contains("transparent")));
    }

    #[test]
    fn too_long_content_reports_the_limit() {
        let request = text_request(&"a".repeat(4000));
        let message = build(&request).unwrap_err();
        assert!(message.contains("too long"));
        assert!(message.contains("At most"));
    }

    /// A minimal valid prepared-PNG logo (512x512, as load_logo produces).
    fn test_logo_base64() -> String {
        let img = image::RgbaImage::from_pixel(512, 512, image::Rgba([200, 10, 10, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, png)
    }

    fn with_logo(mut request: BuildRequest, fraction: f64) -> BuildRequest {
        request.logo = Some(LogoInput {
            png_base64: test_logo_base64(),
            fraction,
        });
        request
    }

    #[test]
    fn logo_forces_level_h_and_clears_center() {
        let mut request = text_request("Hello with logo");
        request.error_correction = "Q".into();
        let request = with_logo(request, 0.15);
        let built = build(&request).expect("builds");
        assert_eq!(built.level_used, Ecc::High);
        assert!(built.svg.contains("<image"));

        // Fewer module subpaths than the same build without a logo.
        let mut plain = text_request("Hello with logo");
        plain.error_correction = "H".into();
        let plain_built = build(&plain).expect("builds");
        let count = |shapes: &[Shape]| {
            shapes
                .iter()
                .map(|s| {
                    s.segments
                        .iter()
                        .filter(|x| matches!(x, geometry::Segment::MoveTo(..)))
                        .count()
                })
                .sum::<usize>()
        };
        assert!(count(&built.shapes) < count(&plain_built.shapes));
    }

    #[test]
    fn logo_validation_rejects_bad_input() {
        let mut request = text_request("hi");
        request.logo = Some(LogoInput {
            png_base64: "not base64!!".into(),
            fraction: 0.15,
        });
        assert!(build(&request).unwrap_err().contains("could not be read"));

        let mut request = text_request("hi");
        request.logo = Some(LogoInput {
            png_base64: test_logo_base64(),
            fraction: 0.5,
        });
        assert!(build(&request).unwrap_err().contains("between 5% and 20%"));

        // No logo: level control works normally again.
        let request = text_request("hi");
        assert_eq!(build(&request).unwrap().level_used, Ecc::Quartile);
    }

    #[test]
    fn logo_near_max_warns() {
        let request = with_logo(text_request("hi"), 0.19);
        let built = build(&request).expect("builds");
        assert!(
            built
                .warnings
                .iter()
                .any(|w| w.contains("close to the maximum"))
        );
        let request = with_logo(text_request("hi"), 0.12);
        let built = build(&request).expect("builds");
        assert!(
            !built
                .warnings
                .iter()
                .any(|w| w.contains("close to the maximum"))
        );
    }

    #[test]
    fn margin_range_is_enforced() {
        assert_eq!(validate_margin(1), Ok(1));
        assert_eq!(validate_margin(10), Ok(10));
    }

    #[test]
    fn png_size_range_is_enforced_and_used_exactly() {
        assert_eq!(validate_png_size(256), Ok(256));
        assert_eq!(validate_png_size(4096), Ok(4096));
        assert!(validate_png_size(255).is_err());
        assert!(validate_png_size(4097).is_err());

        // The build reports the requested size as-is (no snapping).
        let built = build(&text_request("Hello")).expect("builds");
        assert_eq!(built.png_pixels, 1080);
        let mut request = text_request("Hello");
        request.png_size = 777;
        assert_eq!(build(&request).expect("builds").png_pixels, 777);
    }

    /// Grid positions of every drawn dark module (sharp style draws exactly
    /// one subpath per module; its MoveTo is the top-left corner in canvas
    /// coordinates, so the margin is subtracted back off).
    fn drawn_modules(shapes: &[Shape], margin: usize) -> HashSet<(usize, usize)> {
        shapes[0]
            .segments
            .iter()
            .filter_map(|segment| match *segment {
                Segment::MoveTo(x, y) => {
                    Some(((x - margin as f64) as usize, (y - margin as f64) as usize))
                }
                _ => None,
            })
            .collect()
    }

    /// Pure hole geometry: on an all-dark grid the removed modules are
    /// exactly the fully covered ones, so they must be mirror-symmetric
    /// (horizontally, vertically and point-wise) around the center, for
    /// odd and even sizes and for both styles (Rounded clears with a
    /// rounded rect, Square with the plain one).
    #[test]
    fn logo_clear_rect_is_symmetric_on_an_all_dark_grid() {
        for style in [QrStyle::Sharp, QrStyle::Round] {
            for size in [21usize, 22, 29] {
                let logo_size = 0.15 * (size as f64 + 8.0);
                let clear = logo_clear_rect(logo_size, size, style);
                // Mirror symmetry by construction: opposite edges sum to size.
                assert!((clear.x0 + clear.x1 - size as f64).abs() < 1e-9);
                assert!((clear.y0 + clear.y1 - size as f64).abs() < 1e-9);
                assert!((clear.x1 - clear.x0 - (logo_size + 2.0)).abs() < 1e-9);
                if style == QrStyle::Round {
                    assert!(clear.radius > 0.0 && clear.radius <= (clear.x1 - clear.x0) / 2.0);
                } else {
                    assert_eq!(clear.radius, 0.0);
                }

                let all_dark = Matrix::from_dark_grid(size, vec![true; size * size]);
                let shapes = geometry::build_shapes(&all_dark, QrStyle::Sharp, 0, Some(clear));
                let drawn = drawn_modules(&shapes, 0);
                let all: HashSet<(usize, usize)> = (0..size)
                    .flat_map(|y| (0..size).map(move |x| (x, y)))
                    .collect();
                let removed: HashSet<(usize, usize)> = all.difference(&drawn).copied().collect();
                assert!(!removed.is_empty(), "size {size}: nothing removed");
                for &(x, y) in &removed {
                    assert!(
                        removed.contains(&(size - 1 - x, y))
                            && removed.contains(&(x, size - 1 - y))
                            && removed.contains(&(size - 1 - x, size - 1 - y)),
                        "size {size}, style {style:?}: removed ({x},{y}) has no mirror"
                    );
                }
                let min_x = removed.iter().map(|&(x, _)| x).min().unwrap();
                let max_x = removed.iter().map(|&(x, _)| x).max().unwrap();
                let bounds_center = (min_x + max_x + 1) as f64 / 2.0;
                assert!(
                    (bounds_center - size as f64 / 2.0).abs() <= 0.5,
                    "size {size}: hole bounds center {bounds_center} vs {}",
                    size as f64 / 2.0
                );
            }
        }
    }

    /// Rounded style: the corner cells of the old square area are NOT
    /// removed (the rounded corner only partially covers them), while the
    /// cells in the middle of each side and the center ARE removed.
    #[test]
    fn rounded_clear_keeps_corner_cells_and_removes_side_cells() {
        let size = 29usize;
        let logo_size = 0.15 * 37.0; // production-like: side = 7.55 modules
        let clear = logo_clear_rect(logo_size, size, QrStyle::Round);
        assert!(clear.radius > 0.0);
        assert!(clear.radius <= (clear.x1 - clear.x0) / 2.0);

        let all_dark = Matrix::from_dark_grid(size, vec![true; size * size]);
        let shapes = geometry::build_shapes(&all_dark, QrStyle::Sharp, 0, Some(clear));
        let drawn = drawn_modules(&shapes, 0);
        let is_removed = |x: usize, y: usize| !drawn.contains(&(x, y));

        // First/last cells fully inside the plain bounding box.
        let min_cell = clear.x0.ceil() as usize;
        let max_cell = (clear.x1 - 1.0).floor() as usize;
        assert!(min_cell < max_cell);
        // The old square area's corner cells are kept.
        assert!(!is_removed(min_cell, min_cell), "TL corner cell kept");
        assert!(!is_removed(max_cell, min_cell), "TR corner cell kept");
        assert!(!is_removed(min_cell, max_cell), "BL corner cell kept");
        assert!(!is_removed(max_cell, max_cell), "BR corner cell kept");
        // The middle cell of each side is removed.
        let mid = (min_cell + max_cell) / 2;
        assert!(is_removed(mid, min_cell), "top side middle removed");
        assert!(is_removed(mid, max_cell), "bottom side middle removed");
        assert!(is_removed(min_cell, mid), "left side middle removed");
        assert!(is_removed(max_cell, mid), "right side middle removed");
        // The center cell is removed.
        assert!(is_removed(size / 2, size / 2), "center cell removed");
    }

    /// The plate exists only for Rounded style with an opaque background
    /// and a logo; it matches the cleared area, shifted into canvas
    /// coordinates, and its radius is proportional and clamped.
    #[test]
    fn plate_rules_by_style_and_background() {
        let logo_base64 = test_logo_base64();

        let mut rounded = text_request("Hello with logo");
        rounded.style = "round".into();
        rounded.error_correction = "H".into();
        rounded.logo = Some(LogoInput {
            png_base64: logo_base64,
            fraction: 0.15,
        });
        let built = build(&rounded).expect("builds");
        let plate = built
            .plate
            .as_ref()
            .expect("rounded + opaque bg has a plate");
        assert!(
            (plate.x + plate.size / 2.0 - built.total_modules as f64 / 2.0).abs() < 1e-9,
            "plate is centered on the canvas"
        );
        assert!((plate.x - plate.y).abs() < 1e-9, "plate is symmetric");
        assert!(plate.radius > 0.0 && plate.radius <= plate.size / 2.0);

        let mut square = rounded.clone();
        square.style = "sharp".into();
        assert!(
            build(&square).expect("builds").plate.is_none(),
            "square style has no plate"
        );

        let mut transparent = rounded.clone();
        transparent.transparent = true;
        assert!(
            build(&transparent).expect("builds").plate.is_none(),
            "transparent background has no plate"
        );

        let mut no_logo = text_request("Hello");
        no_logo.style = "round".into();
        assert!(
            build(&no_logo).expect("builds").plate.is_none(),
            "no logo means no plate"
        );
    }

    /// The real pipeline: with a logo, the removed dark modules (compared
    /// against the same payload without one) must form a hole centered
    /// under the logo rect, for margins across the whole VALIDATED range
    /// (1..=10; margin 0 is covered by the all-dark geometry test above
    /// because build() rejects it before the geometry runs).
    #[test]
    fn logo_hole_is_centered_under_the_logo_for_several_margins() {
        let logo_base64 = test_logo_base64();
        for margin in [1usize, 4, 10] {
            let mut plain = text_request("Hello with logo");
            plain.error_correction = "H".into();
            plain.margin = margin as i32;
            let plain_built = build(&plain).expect("plain builds");

            let mut with_logo = plain.clone();
            with_logo.logo = Some(LogoInput {
                png_base64: logo_base64.clone(),
                fraction: 0.15,
            });
            let built = build(&with_logo).expect("logo builds");

            let before = drawn_modules(&plain_built.shapes, margin);
            let after = drawn_modules(&built.shapes, margin);
            let removed: Vec<(usize, usize)> = before.difference(&after).copied().collect();
            assert!(!removed.is_empty(), "margin {margin}: nothing removed");

            let size = plain_built.size_modules;
            let total = built.total_modules;
            assert_eq!(total, size + 2 * margin);

            // The hole's bounds, converted to canvas coords (+ margin), are
            // centered on the code center.
            let min_x = removed.iter().map(|&(x, _)| x).min().unwrap();
            let max_x = removed.iter().map(|&(x, _)| x).max().unwrap();
            let min_y = removed.iter().map(|&(_, y)| y).min().unwrap();
            let max_y = removed.iter().map(|&(_, y)| y).max().unwrap();
            let center_x = (min_x + max_x + 1) as f64 / 2.0 + margin as f64;
            let center_y = (min_y + max_y + 1) as f64 / 2.0 + margin as f64;
            assert!(
                (center_x - total as f64 / 2.0).abs() <= 0.5
                    && (center_y - total as f64 / 2.0).abs() <= 0.5,
                "margin {margin}: hole center ({center_x}, {center_y}) vs {}",
                total as f64 / 2.0
            );

            // The logo rect (canvas coords) lies fully inside the hole.
            let logo = built.logo.as_ref().expect("logo present");
            assert!(
                logo.x >= min_x as f64 + margin as f64
                    && logo.y >= min_y as f64 + margin as f64
                    && logo.x + logo.size <= (max_x + 1) as f64 + margin as f64
                    && logo.y + logo.size <= (max_y + 1) as f64 + margin as f64,
                "margin {margin}: logo at ({}, {}) size {} not inside hole x[{min_x}, {max_x}] y[{min_y}, {max_y}]",
                logo.x,
                logo.y,
                logo.size
            );
        }
    }
}
