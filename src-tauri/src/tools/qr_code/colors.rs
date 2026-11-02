//! Colors: strict parsing, WCAG relative luminance and contrast, and the
//! warnings shown next to the preview. Pure logic, unit tested.
//!
//! Only `#RRGGBB` is accepted — the same strict rule the SVG renderer
//! relies on: a validated color is the only string that ever ends up in
//! generated markup, so no user text can break out of the SVG.

/// An RGB color parsed from strict `#RRGGBB` input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    /// Parses `#RRGGBB` strictly: exactly seven characters, `#` first, all
    /// remaining characters hex digits. Anything else is a friendly error.
    pub fn parse(input: &str) -> Result<Self, String> {
        let Some(digits) = input.trim().strip_prefix('#') else {
            return Err("Use a color in the #RRGGBB form, for example #1A1A1A.".into());
        };
        if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("Use a color in the #RRGGBB form, for example #1A1A1A.".into());
        }
        let value = u32::from_str_radix(digits, 16).unwrap_or(0);
        Ok(Rgb {
            r: (value >> 16) as u8,
            g: (value >> 8) as u8,
            b: value as u8,
        })
    }

    /// Lowercase `#rrggbb` form used in generated SVG.
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// WCAG 2.x relative luminance (0.0 = black, 1.0 = white).
    pub fn relative_luminance(self) -> f64 {
        fn channel(value: u8) -> f64 {
            let c = f64::from(value) / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.212_6 * channel(self.r) + 0.715_2 * channel(self.g) + 0.072_2 * channel(self.b)
    }
}

/// WCAG contrast ratio between two colors, from 1.0 (identical) to 21.0.
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let la = a.relative_luminance();
    let lb = b.relative_luminance();
    let (light, dark) = if la >= lb { (la, lb) } else { (lb, la) };
    (light + 0.05) / (dark + 0.05)
}

/// Contrast ratio below which the preview shows a "hard to scan" warning.
/// Chosen at 3.0:1 — well below ideal print contrast (~7:1 for real black
/// on white) but still safely scannable on screens; lower values regularly
/// fail in practice.
pub const MIN_COMFORTABLE_CONTRAST: f64 = 3.0;

/// Warnings for the chosen colors, shown non-blocking next to the preview.
/// `background` is `None` when the background is transparent.
pub fn color_warnings(foreground: Rgb, background: Option<Rgb>) -> Vec<String> {
    let mut warnings = Vec::new();
    match background {
        Some(bg) => {
            let ratio = contrast_ratio(foreground, bg);
            if ratio < MIN_COMFORTABLE_CONTRAST {
                warnings.push(format!(
                    "The contrast between the two colors is low ({ratio:.1}:1). The code may be hard to scan."
                ));
            }
            if foreground.relative_luminance() > bg.relative_luminance() {
                warnings.push(
                    "The code is lighter than its background (inverted). Some scanner apps cannot read inverted codes \u{2014} test before use.".into(),
                );
            }
        }
        None => {
            warnings.push(
                "The background is transparent. Scanning depends on the surface behind the code \u{2014} test it on the intended surface.".into(),
            );
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_strict_hex() {
        assert_eq!(Rgb::parse("#000000").unwrap(), Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(
            Rgb::parse("#FFFFFF").unwrap(),
            Rgb {
                r: 255,
                g: 255,
                b: 255
            }
        );
        assert_eq!(
            Rgb::parse("#1a2B3c").unwrap(),
            Rgb {
                r: 0x1a,
                g: 0x2b,
                b: 0x3c
            }
        );
        assert_eq!(
            Rgb::parse(" #0067c0 ").unwrap(),
            Rgb {
                r: 0,
                g: 0x67,
                b: 0xc0
            }
        );
    }

    #[test]
    fn parse_rejects_everything_else() {
        for bad in [
            "", "#", "#fff", "#fffff", "#fffffff", "ffffff", "#ffgggg", "#ff;fff", "red",
        ] {
            assert!(Rgb::parse(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn hex_output_is_lowercase() {
        assert_eq!(Rgb::parse("#1A2B3C").unwrap().hex(), "#1a2b3c");
    }

    #[test]
    fn luminance_ordering() {
        let black = Rgb::parse("#000000").unwrap();
        let white = Rgb::parse("#ffffff").unwrap();
        let mid = Rgb::parse("#808080").unwrap();
        assert!(black.relative_luminance() < mid.relative_luminance());
        assert!(mid.relative_luminance() < white.relative_luminance());
        assert!((white.relative_luminance() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn contrast_ratio_known_values() {
        let black = Rgb::parse("#000000").unwrap();
        let white = Rgb::parse("#ffffff").unwrap();
        assert!((contrast_ratio(black, white) - 21.0).abs() < 0.1);
        assert!((contrast_ratio(white, white) - 1.0).abs() < 1e-9);
        // Symmetry.
        assert!((contrast_ratio(black, white) - contrast_ratio(white, black)).abs() < 1e-9);
    }

    #[test]
    fn warnings_low_contrast_and_inverted() {
        let white = Rgb::parse("#ffffff").unwrap();
        let black = Rgb::parse("#000000").unwrap();
        let light_gray = Rgb::parse("#cccccc").unwrap();

        // Inverted: a light code on a darker background.
        let inverted = color_warnings(white, Some(light_gray));
        assert!(inverted.iter().any(|w| w.contains("inverted")));

        // Low contrast: two grays close together.
        let low = color_warnings(light_gray, Some(Rgb::parse("#bbbbbb").unwrap()));
        assert!(
            low.iter()
                .any(|w| w.contains("contrast") && w.contains("low"))
        );

        // Normal black on white: no warnings.
        assert!(color_warnings(black, Some(white)).is_empty());

        // Transparent background: exactly one transparent warning.
        let transparent = color_warnings(black, None);
        assert_eq!(transparent.len(), 1);
        assert!(transparent[0].contains("transparent"));
    }

    #[test]
    fn warning_threshold_matches_constant() {
        // Ratio just above the threshold must not warn.
        let black = Rgb::parse("#000000").unwrap();
        // #5a5a5a sits at ~3.0:1 against black.
        let dim = Rgb::parse("#5a5a5a").unwrap();
        assert!(contrast_ratio(black, dim) > MIN_COMFORTABLE_CONTRAST);
        assert!(color_warnings(black, Some(dim)).is_empty());
        // A color clearly below the threshold must warn.
        let darker = Rgb::parse("#303030").unwrap();
        assert!(contrast_ratio(black, darker) < MIN_COMFORTABLE_CONTRAST);
        assert!(
            color_warnings(black, Some(darker))
                .iter()
                .any(|w| w.contains("contrast") && w.contains("low"))
        );
    }
}
