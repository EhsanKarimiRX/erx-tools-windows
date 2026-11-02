//! The pure build pipeline shared by every command: validate all inputs,
//! build the payload, generate the matrix, render the geometry and SVG.
//! No Tauri types here, so the whole pipeline is unit testable.

use super::colors::{self, Rgb};
use super::geometry::{self, ClearRect, LogoDraw, QrStyle, Shape};
use super::matrix::{self, Ecc, Matrix};
use super::payload::{self, WifiOptions, WifiSecurity};
use super::raster;
use super::svg;

/// Quiet-zone margin in modules: the allowed range (the default of 4 is
/// the frontend's concern; Rust only validates the range).
pub const MIN_MARGIN: usize = 1;
pub const MAX_MARGIN: usize = 10;

/// Requested PNG export sizes: the allowed range (default 1024 lives in
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
    /// Requested PNG side length in pixels (snapped down to whole modules).
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
    /// QR side length in modules (without the quiet zone).
    pub size_modules: usize,
    /// QR side length including both margins.
    pub total_modules: usize,
    /// Exact PNG side length after snapping to whole pixels per module.
    pub png_pixels: u32,
    pub png_pixels_per_module: u32,
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

    // The cleared module area is slightly larger than the logo (one module
    // of padding on every side). With a background it fills with the
    // background color automatically; transparent backgrounds simply stay
    // empty behind the logo.
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
    let clear = logo_draw.as_ref().map(|draw| {
        let side = draw.size + 2.0 * LOGO_CLEAR_PADDING_MODULES;
        let center = total_modules as f64 / 2.0;
        ClearRect {
            x0: center - side / 2.0,
            y0: center - side / 2.0,
            x1: center + side / 2.0,
            y1: center + side / 2.0,
        }
    });

    let shapes = geometry::build_shapes(&matrix, style, margin, clear);
    let svg_text = svg::render(
        &shapes,
        total_modules as f64,
        foreground,
        background,
        logo_draw.as_ref(),
    );
    let snapped = raster::snap_size(total_modules, png_size);

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
        size_modules: matrix.size(),
        total_modules,
        png_pixels: snapped.pixels,
        png_pixels_per_module: snapped.pixels_per_module,
        level_used: ecc,
        warnings,
    })
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
            png_size: 1024,
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
    fn png_size_range_is_enforced_and_snapped() {
        assert_eq!(validate_png_size(256), Ok(256));
        assert_eq!(validate_png_size(4096), Ok(4096));
        assert!(validate_png_size(255).is_err());
        assert!(validate_png_size(4097).is_err());

        // The build reports the exact snapped size (29 modules at 1024 ->
        // 35 px per module -> 1015).
        let built = build(&text_request("Hello")).expect("builds");
        assert_eq!(built.png_pixels as usize, built.total_modules * 35);
        assert_eq!(built.png_pixels_per_module, 35);
    }
}
