//! The pure build pipeline shared by every command: validate all inputs,
//! build the payload, generate the matrix, render the geometry and SVG.
//! No Tauri types here, so the whole pipeline is unit testable.

use super::colors::{self, Rgb};
use super::geometry::{self, ClearRect, QrStyle};
use super::matrix::{self, Ecc, Matrix};
use super::payload::{self, WifiOptions, WifiSecurity};
use super::svg;

/// Quiet-zone margin in modules: the allowed range (the default of 4 is
/// the frontend's concern; Rust only validates the range).
pub const MIN_MARGIN: usize = 1;
pub const MAX_MARGIN: usize = 10;

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
}

/// The result of one successful build.
#[derive(Debug, Clone)]
pub struct BuiltQr {
    /// Complete SVG document (also used verbatim for the .svg export).
    pub svg: String,
    /// QR side length in modules (without the quiet zone).
    pub size_modules: usize,
    /// QR side length including both margins.
    pub total_modules: usize,
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
    build_with_clear(request, None)
}

/// Like `build`, with an optional cleared module area for a centered logo.
pub fn build_with_clear(
    request: &BuildRequest,
    clear: Option<ClearRect>,
) -> Result<BuiltQr, String> {
    let payload_text = request.content.payload()?;
    let style = QrStyle::from_ui(&request.style)?;
    let foreground = Rgb::parse(&request.foreground)?;
    let background = if request.transparent {
        None
    } else {
        Some(Rgb::parse(&request.background)?)
    };
    let ecc = Ecc::from_ui(&request.error_correction)?;
    let margin = validate_margin(request.margin)?;
    if payload_text.is_empty() {
        return Err("Enter some content first.".into());
    }

    let matrix = Matrix::from_text(&payload_text, ecc)
        .map_err(|too_long| matrix::too_long_message(too_long.max_chars))?;
    let shapes = geometry::build_shapes(&matrix, style, margin, clear);
    let total_modules = matrix.size() + 2 * margin;
    let svg_text = svg::render(&shapes, total_modules as f64, foreground, background);

    Ok(BuiltQr {
        char_count: payload_text.chars().count(),
        max_chars: matrix::max_fitting_chars(&payload_text, ecc),
        svg: svg_text,
        size_modules: matrix.size(),
        total_modules,
        level_used: ecc,
        warnings: colors::color_warnings(foreground, background),
    })
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

    #[test]
    fn margin_range_is_enforced() {
        assert_eq!(validate_margin(1), Ok(1));
        assert_eq!(validate_margin(10), Ok(10));
    }
}
