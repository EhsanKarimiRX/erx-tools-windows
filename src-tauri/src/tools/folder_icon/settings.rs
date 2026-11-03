//! Settings for the Folder Icon tool — the single source of truth.
//!
//! Everything the user can choose lives here, and everything the user sends
//! is validated HERE first: raw UI strings and unbounded integers never
//! travel past this module. The frontend mirrors the wire shape in
//! `src/tools/folder-icon/types.ts`; the TypeScript copy holds the same
//! constants and must never be trusted — this file decides what is valid.

use super::colors::Rgb;

/// Pattern drawn on the front panel of the folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternKind {
    /// Solid front panel, no pattern.
    None,
    /// Vertical bars.
    Vertical,
    /// Horizontal bars.
    Horizontal,
    /// Alternating squares.
    Checker,
}

impl PatternKind {
    /// Maps the UI token; anything unknown is a friendly error.
    pub fn from_ui(value: &str) -> Result<Self, String> {
        match value {
            "none" => Ok(PatternKind::None),
            "vertical" => Ok(PatternKind::Vertical),
            "horizontal" => Ok(PatternKind::Horizontal),
            "checker" => Ok(PatternKind::Checker),
            other => Err(format!("Unknown pattern: {other}")),
        }
    }

    /// Stable token used in tests and error messages.
    pub fn token(self) -> &'static str {
        match self {
            PatternKind::None => "none",
            PatternKind::Vertical => "vertical",
            PatternKind::Horizontal => "horizontal",
            PatternKind::Checker => "checker",
        }
    }
}

/// Pattern size steps, thin (1) to thick (5).
pub const PATTERN_SIZE_MIN: i32 = 1;
pub const PATTERN_SIZE_MAX: i32 = 5;
/// Pattern opacity bounds, in percent.
pub const PATTERN_OPACITY_MIN: i32 = 10;
pub const PATTERN_OPACITY_MAX: i32 = 100;

/// A fully validated set of icon settings. Every field is a plain number or
/// enum — nothing raw from the UI survives into this struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IconSettings {
    /// The front panel color, exactly as the user picked it.
    pub base_color: Rgb,
    pub pattern: PatternKind,
    /// Color of the pattern bars/squares (used only when `pattern` != None).
    pub pattern_color: Rgb,
    /// Pattern size step, clamped-checked into `PATTERN_SIZE_MIN..=MAX`.
    pub pattern_size: i32,
    /// Pattern opacity in percent, into `PATTERN_OPACITY_MIN..=MAX`.
    pub pattern_opacity: i32,
}

impl IconSettings {
    /// Validates raw UI input and builds settings. All arguments arrive as
    /// strings/loose integers from the wire; every failure is a short,
    /// friendly English sentence the UI can show as-is.
    pub fn from_parts(
        base_color: &str,
        pattern: &str,
        pattern_color: &str,
        pattern_size: i32,
        pattern_opacity: i32,
    ) -> Result<Self, String> {
        let base_color = Rgb::parse(base_color)?;
        let pattern = PatternKind::from_ui(pattern)?;
        let pattern_color = Rgb::parse(pattern_color)?;
        if !(PATTERN_SIZE_MIN..=PATTERN_SIZE_MAX).contains(&pattern_size) {
            return Err(format!(
                "Pattern size must be between {PATTERN_SIZE_MIN} (thin) and {PATTERN_SIZE_MAX} (thick)."
            ));
        }
        if !(PATTERN_OPACITY_MIN..=PATTERN_OPACITY_MAX).contains(&pattern_opacity) {
            return Err(format!(
                "Pattern opacity must be between {PATTERN_OPACITY_MIN} and {PATTERN_OPACITY_MAX} percent."
            ));
        }
        Ok(IconSettings {
            base_color,
            pattern,
            pattern_color,
            pattern_size,
            pattern_opacity,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: fn(&str, &str, &str, i32, i32) -> Result<IconSettings, String> =
        IconSettings::from_parts;

    #[test]
    fn accepts_a_valid_setting() {
        let settings = VALID("#0067C0", "vertical", "#FFFFFF", 3, 60).expect("valid");
        assert_eq!(
            settings,
            IconSettings {
                base_color: Rgb {
                    r: 0,
                    g: 0x67,
                    b: 0xc0
                },
                pattern: PatternKind::Vertical,
                pattern_color: Rgb {
                    r: 255,
                    g: 255,
                    b: 255
                },
                pattern_size: 3,
                pattern_opacity: 60,
            }
        );
    }

    #[test]
    fn accepts_all_pattern_kinds() {
        for (token, expected) in [
            ("none", PatternKind::None),
            ("vertical", PatternKind::Vertical),
            ("horizontal", PatternKind::Horizontal),
            ("checker", PatternKind::Checker),
        ] {
            let settings = VALID("#808080", token, "#000000", 1, 10).expect(token);
            assert_eq!(settings.pattern, expected);
            assert_eq!(settings.pattern.token(), token);
        }
    }

    #[test]
    fn rejects_bad_colors_with_the_color_message() {
        for base in ["", "blue", "#12345", "#1234567", "#12g456"] {
            let error = VALID(base, "none", "#000000", 3, 50).unwrap_err();
            assert_eq!(error, super::super::colors::COLOR_MESSAGE);
        }
        // The pattern color is validated even when no pattern is used, so
        // the field can never hold something unusable.
        let error = VALID("#808080", "none", "pink", 3, 50).unwrap_err();
        assert_eq!(error, super::super::colors::COLOR_MESSAGE);
    }

    #[test]
    fn rejects_unknown_pattern_tokens() {
        assert!(
            VALID("#808080", "stripes", "#000000", 3, 50)
                .unwrap_err()
                .contains("Unknown pattern")
        );
        assert!(
            VALID("#808080", "", "#000000", 3, 50)
                .unwrap_err()
                .contains("Unknown pattern")
        );
    }

    #[test]
    fn rejects_out_of_bounds_size_and_opacity() {
        for size in [0, -1, 6, 100] {
            let error = VALID("#808080", "checker", "#000000", size, 50).unwrap_err();
            assert!(error.contains("Pattern size"), "size {size}: {error}");
        }
        for opacity in [0, 9, 101, -10] {
            let error = VALID("#808080", "checker", "#000000", 3, opacity).unwrap_err();
            assert!(error.contains("opacity"), "opacity {opacity}: {error}");
        }
    }

    #[test]
    fn boundary_values_are_accepted() {
        assert!(
            VALID(
                "#808080",
                "checker",
                "#000000",
                PATTERN_SIZE_MIN,
                PATTERN_OPACITY_MIN
            )
            .is_ok()
        );
        assert!(
            VALID(
                "#808080",
                "checker",
                "#000000",
                PATTERN_SIZE_MAX,
                PATTERN_OPACITY_MAX
            )
            .is_ok()
        );
    }
}
