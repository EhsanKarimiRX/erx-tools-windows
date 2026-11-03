//! Pure pipeline for the Folder Icon tool: validated settings → drawing →
//! SVG, plus the non-blocking hints shown next to the preview. Unit tested;
//! no Tauri types here (same split as the QR tool's `core`).

use super::colors::{MIN_PATTERN_CONTRAST, contrast_ratio, derive_back_color};
use super::geometry::{Drawing, build_drawing};
use super::settings::PatternKind;

/// A built folder icon: everything the UI and (in 4B) the ICO writer need.
#[derive(Debug, Clone)]
pub struct BuiltIcon {
    /// The SVG document shown in the preview.
    pub svg: String,
    /// The shared geometry the 4B rasterizer will consume for the .ico.
    /// Unused until then (same pattern as error.rs's allow(dead_code)).
    #[allow(dead_code)]
    pub drawing: Drawing,
    /// The derived back panel color (reportable in tests/UI).
    #[allow(dead_code)]
    pub back_color: super::colors::Rgb,
    /// Non-blocking hints ("Pattern may be hard to see.").
    pub warnings: Vec<String>,
}

/// Builds the icon from already-validated settings.
pub fn build(settings: &super::settings::IconSettings) -> BuiltIcon {
    let back_color = derive_back_color(settings.base_color);
    let drawing = build_drawing(
        settings.base_color,
        back_color,
        settings.pattern,
        settings.pattern_color,
        settings.pattern_size,
        f64::from(settings.pattern_opacity) / 100.0,
    );
    let svg = super::svg::render(&drawing);
    BuiltIcon {
        svg,
        drawing,
        back_color,
        warnings: warnings(settings),
    }
}

/// Non-blocking hints. Only one today: very low pattern contrast.
fn warnings(settings: &super::settings::IconSettings) -> Vec<String> {
    let mut warnings = Vec::new();
    if settings.pattern != PatternKind::None {
        let ratio = contrast_ratio(settings.base_color, settings.pattern_color);
        if ratio < MIN_PATTERN_CONTRAST {
            warnings.push("Pattern may be hard to see.".into());
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::super::colors::COLOR_MESSAGE;
    use super::super::settings::IconSettings;
    use super::*;

    fn settings(base: &str, pattern: &str, pattern_color: &str) -> IconSettings {
        IconSettings::from_parts(base, pattern, pattern_color, 3, 60).expect("valid settings")
    }

    #[test]
    fn build_produces_svg_and_no_warnings_for_a_normal_choice() {
        let built = build(&settings("#0078D4", "none", "#FFFFFF"));
        assert!(built.svg.starts_with("<svg"));
        assert!(built.warnings.is_empty());
        // The back panel differs from the base color.
        assert_ne!(built.back_color.hex(), "#0078d4");
    }

    #[test]
    fn pattern_hints_fire_only_for_low_contrast() {
        // Nearly the same color: the hint appears.
        let close = build(&settings("#0067C0", "vertical", "#0066C0"));
        assert!(close.warnings.iter().any(|w| w.contains("hard to see")));

        // Strong contrast: no hint.
        let strong = build(&settings("#0067C0", "vertical", "#FFFFFF"));
        assert!(strong.warnings.is_empty());

        // Low contrast but no pattern: still no hint.
        let no_pattern = build(&settings("#0067C0", "none", "#0066C0"));
        assert!(no_pattern.warnings.is_empty());
    }

    #[test]
    fn invalid_settings_never_reach_the_pipeline() {
        let error = IconSettings::from_parts("blue", "vertical", "#FFFFFF", 3, 60).unwrap_err();
        assert_eq!(error, COLOR_MESSAGE);
        let error = IconSettings::from_parts("#0067C0", "diagonal", "#FFFFFF", 3, 60).unwrap_err();
        assert!(error.contains("Unknown pattern"));
        let error = IconSettings::from_parts("#0067C0", "vertical", "#FFFFFF", 9, 60).unwrap_err();
        assert!(error.contains("Pattern size"));
    }

    #[test]
    fn every_pattern_kind_builds() {
        for kind in ["none", "vertical", "horizontal", "checker"] {
            let built = build(&settings("#8E44AD", kind, "#F7C600"));
            assert!(built.svg.starts_with("<svg"));
            assert!(built.svg.ends_with("</svg>"));
        }
    }

    #[test]
    fn extreme_base_colors_keep_panels_distinct_in_the_svg() {
        for base in ["#FFFFFF", "#000000", "#FF0000", "#10B000"] {
            let built = build(&settings(base, "none", "#FFFFFF"));
            // Both panel colors appear as distinct fills.
            let front = Rgb::parse(base).unwrap().hex();
            assert!(built.svg.contains(&format!("fill=\"{front}\"")));
            assert_ne!(built.back_color.hex(), front, "base {base}");
        }
    }

    use super::super::colors::Rgb;
}
