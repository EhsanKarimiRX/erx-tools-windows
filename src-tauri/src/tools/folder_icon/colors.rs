//! Colors for the Folder Icon tool: strict parsing of `#RRGGBB` input.
//!
//! Security property (same rule as the QR tool): a validated `Rgb` is the
//! only user-derived string that can ever reach generated markup — the SVG
//! writer emits `self.hex()` and nothing else. Perceptual color derivation
//! (OKLab) for the back panel and highlight lives here too (checkpoint 4A-2).

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
}

/// The single user-facing message for every malformed color.
pub const COLOR_MESSAGE: &str = "Use a color in the #RRGGBB form, for example #1A9E4F.";

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
}
