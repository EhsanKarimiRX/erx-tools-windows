//! Colors for the Folder Icon tool: strict parsing of `#RRGGBB` input,
//! WCAG contrast, and perceptual derivation of the panel colors in OKLab.
//!
//! Security property (same rule as the QR tool): a validated `Rgb` is the
//! only user-derived string that can ever reach generated markup — the SVG
//! writer emits `self.hex()` and nothing else.
//!
//! Color model: the user picks ONE base color (the front panel). The back
//! panel is derived in OKLab (a perceptual color space) so the two panels
//! keep a visible lightness difference for ANY base color, including pure
//! white, pure black and fully saturated colors. OKLab is chosen over HSL
//! because its L channel matches perceived lightness: an HSL "50% lightness"
//! yellow looks far brighter than a "50%" blue, while OKLab L stays
//! comparable across hues. The formulas are Björn Ottosson's published
//! reference math (public domain); correctness is pinned by round-trip and
//! known-value tests below.

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
            return Err(COLOR_MESSAGE.into());
        };
        if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(COLOR_MESSAGE.into());
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

    /// sRGB byte to linear light, standard sRGB EOTF.
    fn linear_channel(value: u8) -> f64 {
        let c = f64::from(value) / 255.0;
        if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    /// Linear light to an sRGB byte (rounded, clamped).
    fn byte_from_linear(value: f64) -> u8 {
        let c = if value <= 0.003_130_8 {
            value * 12.92
        } else {
            1.055 * value.powf(1.0 / 2.4) - 0.055
        };
        (c.clamp(0.0, 1.0) * 255.0).round() as u8
    }

    /// Converts to OKLab (L in 0..1 for grays, a/b roughly -0.4..0.4).
    pub fn to_oklab(self) -> Oklab {
        let r = Self::linear_channel(self.r);
        let g = Self::linear_channel(self.g);
        let b = Self::linear_channel(self.b);

        let l = 0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b;
        let m = 0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b;
        let s = 0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b;
        let l_ = l.cbrt();
        let m_ = m.cbrt();
        let s_ = s.cbrt();

        Oklab {
            l: 0.210_454_255_3 * l_ + 0.793_617_785_0 * m_ - 0.004_072_046_8 * s_,
            a: 1.977_998_495_1 * l_ - 2.428_592_205_0 * m_ + 0.450_593_709_9 * s_,
            b: 0.025_904_037_1 * l_ + 0.782_771_766_2 * m_ - 0.808_675_766_0 * s_,
        }
    }

    /// Builds a color from OKLab; out-of-gamut results are gamut-mapped by
    /// shrinking chroma toward neutral (see `Oklab::to_rgb_gamut_mapped`).
    pub fn from_oklab(color: Oklab) -> Self {
        color.to_rgb_gamut_mapped()
    }
}

/// The single user-facing message for every malformed color.
pub const COLOR_MESSAGE: &str = "Use a color in the #RRGGBB form, for example #1A9E4F.";

/// A color in the OKLab perceptual space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Oklab {
    /// Perceived lightness, 0.0 (black) to 1.0 (white).
    pub l: f64,
    /// Green–red axis.
    pub a: f64,
    /// Blue–yellow axis.
    pub b: f64,
}

impl Oklab {
    /// Converts straight to sRGB bytes with per-channel clamping. Only used
    /// where a slight hue shift from clamping is acceptable; prefer
    /// `to_rgb_gamut_mapped` for artwork colors.
    fn to_rgb_raw(self) -> Rgb {
        let l_ = self.l + 0.396_337_777_4 * self.a + 0.215_803_757_3 * self.b;
        let m_ = self.l - 0.105_561_345_8 * self.a - 0.063_854_172_8 * self.b;
        let s_ = self.l - 0.089_484_177_5 * self.a - 1.291_485_548_0 * self.b;
        let l = l_ * l_ * l_;
        let m = m_ * m_ * m_;
        let s = s_ * s_ * s_;

        let r = 4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s;
        let g = -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s;
        let b = -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s;

        Rgb {
            r: Rgb::byte_from_linear(r),
            g: Rgb::byte_from_linear(g),
            b: Rgb::byte_from_linear(b),
        }
    }

    /// True when the color lies inside the sRGB gamut (with a small margin
    /// so borderline colors don't flicker between representations).
    fn in_gamut(self) -> bool {
        let l_ = self.l + 0.396_337_777_4 * self.a + 0.215_803_757_3 * self.b;
        let m_ = self.l - 0.105_561_345_8 * self.a - 0.063_854_172_8 * self.b;
        let s_ = self.l - 0.089_484_177_5 * self.a - 1.291_485_548_0 * self.b;
        let l = l_ * l_ * l_;
        let m = m_ * m_ * m_;
        let s = s_ * s_ * s_;
        let r = 4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s;
        let g = -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s;
        let b = -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s;
        r > -1e-4 && r < 1.000_1 && g > -1e-4 && g < 1.000_1 && b > -1e-4 && b < 1.000_1
    }

    /// Converts to sRGB, shrinking chroma toward neutral until the color is
    /// representable (standard chroma-clip gamut mapping, ~binary search).
    /// Lightness and hue are preserved; only saturation is reduced, so the
    /// derived panel keeps the base color's character.
    fn to_rgb_gamut_mapped(self) -> Rgb {
        if self.in_gamut() {
            return self.to_rgb_raw();
        }
        let mut low = 0.0_f64; // neutral gray: always in gamut
        let mut high = self.chroma();
        // Guard against a degenerate loop for L outside 0..1.
        let l = self.l.clamp(0.0, 1.0);
        for _ in 0..24 {
            let mid = (low + high) / 2.0;
            let scaled = self.scale_chroma(mid, l);
            if scaled.in_gamut() {
                low = mid;
            } else {
                high = mid;
            }
        }
        self.scale_chroma(low, l).to_rgb_raw()
    }

    /// Chroma magnitude sqrt(a² + b²).
    fn chroma(self) -> f64 {
        (self.a * self.a + self.b * self.b).sqrt()
    }

    /// The same color with chroma scaled to `target` (hue preserved).
    fn scale_chroma(self, target: f64, l: f64) -> Self {
        let c = self.chroma();
        if c <= 1e-9 {
            return Oklab { l, a: 0.0, b: 0.0 };
        }
        let k = target / c;
        Oklab {
            l,
            a: self.a * k,
            b: self.b * k,
        }
    }
}

/// WCAG 2.x relative luminance (0.0 = black, 1.0 = white).
pub fn relative_luminance(color: Rgb) -> f64 {
    fn channel(value: u8) -> f64 {
        let c = f64::from(value) / 255.0;
        if c <= 0.039_28 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    0.212_6 * channel(color.r) + 0.715_2 * channel(color.g) + 0.072_2 * channel(color.b)
}

/// WCAG contrast ratio between two colors, from 1.0 (identical) to 21.0.
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (light, dark) = if la >= lb { (la, lb) } else { (lb, la) };
    (light + 0.05) / (dark + 0.05)
}

/// Contrast ratio below which the pattern gets the "hard to see" hint. A
/// decorative overlay is still visible somewhat below the 3.0 the QR tool
/// needs for scannability; 1.5:1 is where a pattern truly melts into the
/// panel, so the hint fires there.
pub const MIN_PATTERN_CONTRAST: f64 = 1.5;

// ----- panel color derivation -----

/// Target OKLab lightness difference between the front and back panels. At
/// least this much separation is enforced for every possible base color.
pub const MIN_PANEL_LIGHTNESS_DIFF: f64 = 0.10;

/// Chroma multiplier for the back panel (shadows are slightly muted).
const BACK_CHROMA_SCALE: f64 = 0.9;

/// Lightness factor for light colors: the back panel is proportionally
/// darker. Crossover with the fixed `-MIN_PANEL_LIGHTNESS_DIFF` rule sits
/// at L = MIN_PANEL_LIGHTNESS_DIFF / (1 - 0.76) ≈ 0.4167.
const BACK_LIGHTNESS_FACTOR: f64 = 0.76;

/// Derives the back panel color from the base (front panel) color:
///
/// - lightness: `0.76 × L` for light colors (proportional depth), `L − 0.10`
///   for mid colors, and `L + 0.10` for very dark colors (L < 0.10) where
///   darkening further is impossible — the back panel becomes slightly
///   lighter instead. Every branch keeps |ΔL| ≥ 0.10.
/// - chroma: the hue is kept, chroma slightly muted, then gamut-mapped.
pub fn derive_back_color(base: Rgb) -> Rgb {
    let lab = base.to_oklab();
    // The darker of "proportional depth" and "fixed -0.10" so the minimum
    // difference holds in both regimes (min = the darker candidate; the
    // fixed rule is the binding one below the L≈0.4167 crossover).
    let back_l = if lab.l >= MIN_PANEL_LIGHTNESS_DIFF {
        (BACK_LIGHTNESS_FACTOR * lab.l).min(lab.l - MIN_PANEL_LIGHTNESS_DIFF)
    } else {
        lab.l + MIN_PANEL_LIGHTNESS_DIFF
    };
    let back = Oklab {
        l: back_l.clamp(0.0, 1.0),
        a: lab.a * BACK_CHROMA_SCALE,
        b: lab.b * BACK_CHROMA_SCALE,
    };
    Rgb::from_oklab(back)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(color: Rgb) -> String {
        color.hex()
    }

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
            Rgb::parse(" #1a2B3c ").unwrap(),
            Rgb {
                r: 0x1a,
                g: 0x2b,
                b: 0x3c
            }
        );
    }

    #[test]
    fn parse_rejects_everything_else() {
        for bad in [
            "", "#", "#fff", "#fffff", "#fffffff", "ffffff", "#ffgggg", "#ff;fff", "red", "#12 456",
        ] {
            assert!(Rgb::parse(bad).is_err(), "should reject {bad:?}");
            assert_eq!(Rgb::parse(bad).unwrap_err(), COLOR_MESSAGE);
        }
    }

    #[test]
    fn hex_output_is_lowercase() {
        assert_eq!(Rgb::parse("#1A2B3C").unwrap().hex(), "#1a2b3c");
    }

    #[test]
    fn oklab_known_values() {
        // Widely cited OKLab values for the sRGB primaries.
        let white = Rgb {
            r: 255,
            g: 255,
            b: 255,
        }
        .to_oklab();
        assert!((white.l - 1.0).abs() < 1e-6);
        assert!(white.a.abs() < 1e-6 && white.b.abs() < 1e-6);

        let black = Rgb { r: 0, g: 0, b: 0 }.to_oklab();
        assert!(black.l.abs() < 1e-6);

        let red = Rgb { r: 255, g: 0, b: 0 }.to_oklab();
        assert!((red.l - 0.628).abs() < 2e-3, "red L = {}", red.l);
        assert!((red.a - 0.2249).abs() < 2e-3, "red a = {}", red.a);
        assert!((red.b - 0.1258).abs() < 2e-3, "red b = {}", red.b);

        // Mid gray stays neutral and lands near L = 0.6.
        let gray = Rgb {
            r: 128,
            g: 128,
            b: 128,
        }
        .to_oklab();
        assert!(gray.a.abs() < 1e-4 && gray.b.abs() < 1e-4);
        assert!((gray.l - 0.6).abs() < 0.02, "gray L = {}", gray.l);
    }

    #[test]
    fn oklab_round_trip_over_grays_and_hues() {
        // Every gray level must survive OKLab → sRGB within one byte.
        for v in [0_u8, 1, 7, 31, 64, 128, 200, 240, 255] {
            let color = Rgb { r: v, g: v, b: v };
            let back = Rgb::from_oklab(color.to_oklab());
            assert!((i32::from(back.r) - i32::from(v)).abs() <= 1, "gray {v}");
            assert_eq!((back.g, back.b), (back.r, back.r));
        }
        // Saturated in-gamut colors must round-trip within a small delta.
        for (r, g, b) in [(0, 103, 192), (232, 17, 35), (16, 124, 16), (247, 198, 0)] {
            let color = Rgb { r, g, b };
            let back = Rgb::from_oklab(color.to_oklab());
            assert!(
                (i32::from(back.r) - i32::from(r)).abs() <= 2
                    && (i32::from(back.g) - i32::from(g)).abs() <= 2
                    && (i32::from(back.b) - i32::from(b)).abs() <= 2,
                "round trip {r},{g},{b} -> {back:?}"
            );
        }
    }

    #[test]
    fn back_panel_keeps_minimum_lightness_difference_everywhere() {
        // All 12 presets plus the extremes the user can reach.
        let mut bases: Vec<Rgb> = vec![
            Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
            Rgb { r: 0, g: 0, b: 0 },
            Rgb { r: 255, g: 0, b: 0 },
            Rgb { r: 0, g: 255, b: 0 },
            Rgb { r: 0, g: 0, b: 255 },
            Rgb {
                r: 255,
                g: 255,
                b: 0,
            },
            Rgb {
                r: 0,
                g: 255,
                b: 255,
            },
            Rgb {
                r: 255,
                g: 0,
                b: 255,
            },
            Rgb {
                r: 10,
                g: 10,
                b: 10,
            },
            Rgb { r: 2, g: 2, b: 2 },
        ];
        for hex_value in [
            "#0078D4", "#4CC2FF", "#107C10", "#F7C600", "#F7630C", "#E81123", "#EA005E", "#8E44AD",
            "#8E562E", "#9B9B9B", "#3B3B3B", "#F3F3F3",
        ] {
            bases.push(Rgb::parse(hex_value).unwrap());
        }
        for base in bases {
            let back = derive_back_color(base);
            let diff = (base.to_oklab().l - back.to_oklab().l).abs();
            assert!(
                diff >= MIN_PANEL_LIGHTNESS_DIFF - 0.005,
                "base {} -> back {}: ΔL = {diff}",
                hex(base),
                hex(back)
            );
        }
    }

    #[test]
    fn back_panel_is_darker_for_light_colors_and_lighter_for_near_black() {
        // Pure white gets a clearly darker back panel.
        let white_back = derive_back_color(Rgb {
            r: 255,
            g: 255,
            b: 255,
        });
        assert!(white_back.to_oklab().l < 0.85);

        // Pure black cannot go darker: the back panel becomes lighter.
        let black_back = derive_back_color(Rgb { r: 0, g: 0, b: 0 });
        assert!(black_back.to_oklab().l >= MIN_PANEL_LIGHTNESS_DIFF - 0.005);
        assert_ne!(black_back.hex(), "#000000");
    }

    #[test]
    fn back_panel_preserves_hue_direction() {
        // A blue base must yield a bluish back panel (b axis dominant).
        let base = Rgb::parse("#0078D4").unwrap();
        let back = derive_back_color(base).to_oklab();
        assert!(back.b < -0.02, "back b = {} (should stay blue-ish)", back.b);
    }

    #[test]
    fn gamut_mapping_terminates_and_stays_reasonable() {
        // Extremely saturated out-of-gamut targets must not panic and must
        // produce a gray-ish or valid color.
        let wild = Rgb::from_oklab(Oklab {
            l: 0.5,
            a: 0.4,
            b: -0.4,
        });
        let _ = hex(wild); // just needs to be constructible
        // Out-of-range lightness is clamped, not panicked.
        let _ = Rgb::from_oklab(Oklab {
            l: 1.4,
            a: 0.0,
            b: 0.0,
        });
    }

    #[test]
    fn contrast_ratio_known_values() {
        let black = Rgb { r: 0, g: 0, b: 0 };
        let white = Rgb {
            r: 255,
            g: 255,
            b: 255,
        };
        assert!((contrast_ratio(black, white) - 21.0).abs() < 0.1);
        assert!((contrast_ratio(white, white) - 1.0).abs() < 1e-9);
        assert!((contrast_ratio(black, white) - contrast_ratio(white, black)).abs() < 1e-9);
    }

    #[test]
    fn luminance_ordering() {
        let black = Rgb { r: 0, g: 0, b: 0 };
        let mid = Rgb {
            r: 128,
            g: 128,
            b: 128,
        };
        let white = Rgb {
            r: 255,
            g: 255,
            b: 255,
        };
        assert!(relative_luminance(black) < relative_luminance(mid));
        assert!(relative_luminance(mid) < relative_luminance(white));
    }
}
