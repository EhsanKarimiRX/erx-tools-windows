//! Favicon package builder for Favicons: output file list, web manifest,
//! HTML snippet, folder-name handling and writing. Pure logic, no Tauri.
//! Spec: docs/PROMPT_11_SPEC.md sections 3, 4, 6.

use image::ExtendedColorType;
use image::ImageEncoder;
use image::RgbaImage;
use serde_json::json;

use super::ico::build_ico;
use super::render::{
    apply_padding, flatten_on_color, make_maskable, make_square, resize_square, trim_transparent,
};
use super::source::LoadedSource;
use super::source::SourceKind;

/// All options as chosen in the UI (already validated on the Rust side).
#[derive(Debug, Clone)]
pub struct FaviconsOptions {
    pub site_name: String,
    /// Padding for favicon.ico, 16x16 and 32x32 only.
    pub padding_percent: f32,
    /// Padding for the Apple touch icon and the Android 192/512 icons
    /// (home screen shortcuts).
    pub app_padding_percent: f32,
    pub trim_transparent: bool,
    pub background_color: String,
    pub theme_color: String,
    pub include_maskable: bool,
}

impl Default for FaviconsOptions {
    fn default() -> Self {
        Self {
            site_name: String::new(),
            padding_percent: 0.0,
            app_padding_percent: 10.0,
            trim_transparent: true,
            background_color: "#ffffff".to_string(),
            theme_color: "#ffffff".to_string(),
            include_maskable: true,
        }
    }
}

fn invalid_color() -> String {
    "Invalid color. Use a value like #ff8800.".to_string()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Parses "#rgb" and "#rrggbb" (any case) into RGB bytes. Anything else is
/// an error. Byte-based so odd text can never panic on a char boundary.
pub fn parse_hex_color(input: &str) -> Result<[u8; 3], String> {
    let digits = input
        .trim()
        .strip_prefix('#')
        .ok_or_else(invalid_color)?
        .as_bytes();
    let mut out = [0u8; 3];
    match digits.len() {
        3 => {
            for (i, &byte) in digits.iter().enumerate() {
                let value = hex_value(byte).ok_or_else(invalid_color)?;
                out[i] = value * 16 + value;
            }
        }
        6 => {
            for (i, pair) in digits.as_chunks::<2>().0.iter().enumerate() {
                let high = hex_value(pair[0]).ok_or_else(invalid_color)?;
                let low = hex_value(pair[1]).ok_or_else(invalid_color)?;
                out[i] = high * 16 + low;
            }
        }
        _ => return Err(invalid_color()),
    }
    Ok(out)
}

/// Normalizes a color to lowercase "#rrggbb". Only used by tests (the
/// package builds normalize colors internally from parsed RGB).
#[cfg(test)]
pub fn normalize_hex_color(input: &str) -> Result<String, String> {
    let [r, g, b] = parse_hex_color(input)?;
    Ok(format!("#{r:02x}{g:02x}{b:02x}"))
}

/// One output file, fully built in memory.
#[derive(Debug)]
pub struct GeneratedFile {
    pub name: String,
    pub bytes: Vec<u8>,
    /// Some for PNG files, None for everything else.
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// The complete package for one source: files in output order, the HTML
/// snippet (also stored as a file) and all warnings.
#[derive(Debug)]
pub struct Package {
    pub files: Vec<GeneratedFile>,
    pub html_snippet: String,
    pub warnings: Vec<String>,
}

/// Builds the whole favicon package in memory (no file system access).
pub fn build_package(source: &LoadedSource, options: &FaviconsOptions) -> Result<Package, String> {
    // 1. Colors are validated first so a typo fails before any image work.
    let background = parse_hex_color(&options.background_color)?;
    let theme = parse_hex_color(&options.theme_color)?;
    let background_str = normalize_from_rgb(background);
    let theme_str = normalize_from_rgb(theme);

    // 2. Optional trim of fully transparent borders.
    let mut master = source.master.clone();
    if options.trim_transparent {
        master = trim_transparent(&master)?;
    }

    // 3. The non-square warning depends on trim, so it is computed here.
    let mut warnings = source.warnings.clone();
    let (width, height) = (master.width(), master.height());
    let smaller = width.min(height);
    let larger = width.max(height);
    if f64::from(larger) / f64::from(smaller) > 1.05 {
        warnings.push(
            "The image is not square. It will be centered on a transparent square canvas.".into(),
        );
    }
    let base = make_square(&master);

    // 4. The favicon padding is used ONLY for the favicon files. The home
    //    screen icons get their own padding; the maskable icon keeps the
    //    unpadded base with its fixed 60 % logo.
    let favicon_base = apply_padding(&base, options.padding_percent);
    let app_base = apply_padding(&base, options.app_padding_percent);

    // 5. Favicon sizes: PNG 16/32 and the multi-size .ico (16/32/48).
    let favicon_16 = resize_square(&favicon_base, 16);
    let favicon_32 = resize_square(&favicon_base, 32);
    let favicon_48 = resize_square(&favicon_base, 48);
    let ico = build_ico(&[favicon_16.clone(), favicon_32.clone(), favicon_48])?;

    // 6. The home screen icons use the app-padded base.
    let apple = flatten_on_color(&resize_square(&app_base, 180), background);
    let android_192 = resize_square(&app_base, 192);
    let android_512 = resize_square(&app_base, 512);
    let maskable = if options.include_maskable {
        Some(make_maskable(&base, 512, background))
    } else {
        None
    };

    // 7. Encode everything.
    let is_svg = source.kind == SourceKind::Svg && source.svg_bytes.is_some();
    let mut files = Vec::new();
    files.push(GeneratedFile {
        name: "favicon.ico".to_string(),
        bytes: ico,
        width: None,
        height: None,
    });
    files.push(png_file("favicon-16x16.png", &favicon_16)?);
    files.push(png_file("favicon-32x32.png", &favicon_32)?);
    if is_svg && let Some(svg_bytes) = &source.svg_bytes {
        files.push(GeneratedFile {
            name: "favicon.svg".to_string(),
            bytes: svg_bytes.clone(),
            width: None,
            height: None,
        });
    }
    files.push(png_file("apple-touch-icon.png", &apple)?);
    files.push(png_file("android-chrome-192x192.png", &android_192)?);
    files.push(png_file("android-chrome-512x512.png", &android_512)?);
    if let Some(maskable) = &maskable {
        files.push(png_file("android-chrome-maskable-512x512.png", maskable)?);
    }
    let html_snippet = build_html_snippet(is_svg, &theme_str);
    files.push(GeneratedFile {
        name: "site.webmanifest".to_string(),
        bytes: build_manifest(
            &options.site_name,
            &theme_str,
            &background_str,
            options.include_maskable,
        )
        .into_bytes(),
        width: None,
        height: None,
    });
    files.push(GeneratedFile {
        name: "favicon-snippet.html".to_string(),
        bytes: html_snippet.clone().into_bytes(),
        width: None,
        height: None,
    });

    Ok(Package {
        files,
        html_snippet,
        warnings,
    })
}

fn normalize_from_rgb(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

fn png_file(name: &str, img: &RgbaImage) -> Result<GeneratedFile, String> {
    Ok(GeneratedFile {
        name: name.to_string(),
        bytes: encode_png(img)?,
        width: Some(img.width()),
        height: Some(img.height()),
    })
}

/// PNG encode with best compression; falls back to the default encoder API
/// only if the quality variant is unavailable.
pub fn encode_png(img: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new_with_quality(
        &mut out,
        image::codecs::png::CompressionType::Best,
        image::codecs::png::FilterType::Adaptive,
    );
    encoder
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(|error| {
            eprintln!("PNG encode error: {error}");
            "The icon files could not be built.".to_string()
        })?;
    Ok(out)
}

/// The site.webmanifest content: pretty-printed JSON with a trailing
/// newline. `theme` and `background` are already normalized "#rrggbb".
pub fn build_manifest(
    site_name: &str,
    theme: &str,
    background: &str,
    include_maskable: bool,
) -> String {
    let name = site_name.trim();
    let name = if name.is_empty() { "Website" } else { name };
    let mut icons = vec![
        json!({ "src": "/android-chrome-192x192.png", "sizes": "192x192", "type": "image/png" }),
        json!({ "src": "/android-chrome-512x512.png", "sizes": "512x512", "type": "image/png" }),
    ];
    if include_maskable {
        icons.push(json!({
            "src": "/android-chrome-maskable-512x512.png",
            "sizes": "512x512",
            "type": "image/png",
            "purpose": "maskable"
        }));
    }
    let manifest = json!({
        "name": name,
        "short_name": name,
        "icons": icons,
        "theme_color": theme,
        "background_color": background,
        "display": "standalone"
    });
    let mut out = serde_json::to_string_pretty(&manifest).expect("manifest JSON always serializes");
    out.push('\n');
    out
}

/// The ready-to-paste HTML head snippet: one line per tag, trailing newline,
/// the favicon.svg line only for SVG sources.
pub fn build_html_snippet(has_svg: bool, theme: &str) -> String {
    let mut lines = vec![r#"<link rel="icon" href="/favicon.ico" sizes="32x32">"#.to_string()];
    if has_svg {
        lines.push(r#"<link rel="icon" href="/favicon.svg" type="image/svg+xml">"#.to_string());
    }
    lines.push(
        r#"<link rel="icon" type="image/png" sizes="32x32" href="/favicon-32x32.png">"#.to_string(),
    );
    lines.push(
        r#"<link rel="icon" type="image/png" sizes="16x16" href="/favicon-16x16.png">"#.to_string(),
    );
    lines.push(r#"<link rel="apple-touch-icon" href="/apple-touch-icon.png">"#.to_string());
    lines.push(r#"<link rel="manifest" href="/site.webmanifest">"#.to_string());
    lines.push(format!(r#"<meta name="theme-color" content="{theme}">"#));
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

const WINDOWS_RESERVED_NAMES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Turns the site name into a safe folder name (spec section 6 rules).
pub fn sanitize_folder_name(input: &str) -> String {
    let mapped: String = input
        .trim()
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                Some(c)
            } else if c == ' ' {
                Some('_')
            } else {
                None
            }
        })
        .collect();
    let mut name = mapped.trim_matches('_').to_string();
    if name.is_empty() {
        name = "website".to_string();
    }
    if name.chars().count() > 40 {
        name = name.chars().take(40).collect();
    }
    if WINDOWS_RESERVED_NAMES.contains(&name.to_lowercase().as_str()) {
        name.push_str("_site");
    }
    name
}

/// `<name>_favicon_package` under the parent, made unique with `_1`, `_2, ...
/// (the counter goes at the END).
pub fn unique_folder_path(parent: &std::path::Path, folder_name: &str) -> std::path::PathBuf {
    let first = parent.join(format!("{folder_name}_favicon_package"));
    if !first.exists() {
        return first;
    }
    let mut counter = 1;
    loop {
        let candidate = parent.join(format!("{folder_name}_favicon_package_{counter}"));
        if !candidate.exists() {
            return candidate;
        }
        counter += 1;
    }
}

/// Creates the unique package folder and writes every file. If ANY step
/// fails, the files and the folder this call created are removed and a
/// friendly error is returned. The parent folder must already exist.
pub fn write_package(
    parent: &std::path::Path,
    site_name: &str,
    package: &Package,
) -> Result<std::path::PathBuf, String> {
    let folder = unique_folder_path(parent, &sanitize_folder_name(site_name));
    if let Err(error) = std::fs::create_dir(&folder) {
        eprintln!("package folder error: {error}");
        return Err("Windows refused to write the icon files. Check the folder.".into());
    }
    for file in &package.files {
        if let Err(error) = std::fs::write(folder.join(&file.name), &file.bytes) {
            eprintln!("package write error: {error}");
            let _ = std::fs::remove_dir_all(&folder);
            return Err("Windows refused to write the icon files. Check the folder.".into());
        }
    }
    Ok(folder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn solid_master(width: u32, height: u32, rgba: [u8; 4]) -> RgbaImage {
        let mut img = RgbaImage::new(width, height);
        for p in img.pixels_mut() {
            *p = Rgba(rgba);
        }
        img
    }

    fn raster_source(width: u32, height: u32) -> LoadedSource {
        LoadedSource {
            kind: SourceKind::Raster,
            format_label: "PNG".to_string(),
            original_width: width,
            original_height: height,
            master: solid_master(width, height, [255, 0, 0, 255]),
            svg_bytes: None,
            warnings: Vec::new(),
        }
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "erx_site_icons_test_{}_{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("creates temp dir");
        dir
    }

    #[test]
    fn parse_and_normalize_hex_colors() {
        assert_eq!(parse_hex_color("#fff"), Ok([0xff, 0xff, 0xff]));
        assert_eq!(parse_hex_color("#FFFFFF"), Ok([0xff, 0xff, 0xff]));
        assert_eq!(parse_hex_color("  #0a0b0c "), Ok([0x0a, 0x0b, 0x0c]));
        assert!(parse_hex_color("#12345").is_err());
        assert!(parse_hex_color("red").is_err());
        assert!(parse_hex_color("").is_err());
        assert_eq!(normalize_hex_color("#ABC").as_deref(), Ok("#aabbcc"));
    }

    #[test]
    fn sanitize_folder_name_follows_the_rules() {
        assert_eq!(sanitize_folder_name("My Site"), "My_Site");
        assert_eq!(sanitize_folder_name("  a/b\\c:d  "), "abcd");
        assert_eq!(sanitize_folder_name(""), "website");
        assert_eq!(sanitize_folder_name("!!!"), "website");
        assert_eq!(sanitize_folder_name("سایت من"), "سایت_من");
        assert_eq!(sanitize_folder_name("CON"), "CON_site");
        assert_eq!(sanitize_folder_name("con"), "con_site");
        let long = sanitize_folder_name(&"a".repeat(100));
        assert_eq!(long.chars().count(), 40);
    }

    #[test]
    fn unique_folder_path_counts_up() {
        let parent = temp_dir("unique");
        let first = unique_folder_path(&parent, "site");
        assert!(first.ends_with("site_favicon_package"));
        std::fs::create_dir(&first).expect("creates");
        let second = unique_folder_path(&parent, "site");
        assert!(second.ends_with("site_favicon_package_1"));
        let _ = std::fs::remove_dir_all(&parent);
    }

    /// Finds one package file's bytes by name.
    fn file_bytes<'a>(package: &'a Package, name: &str) -> &'a Vec<u8> {
        &package
            .files
            .iter()
            .find(|f| f.name == name)
            .expect(name)
            .bytes
    }

    /// Padding on the favicon files works BEFORE any other change: with 25 %
    /// the 32 px icon has a transparent corner and an opaque center, without
    /// padding the whole icon is the logo.
    #[test]
    fn favicon_padding_is_applied_to_favicon_files() {
        let options_a = FaviconsOptions {
            padding_percent: 25.0,
            trim_transparent: false,
            ..Default::default()
        };
        let options_b = FaviconsOptions {
            padding_percent: 0.0,
            trim_transparent: false,
            ..Default::default()
        };
        let source = raster_source(400, 400);
        let package_a = build_package(&source, &options_a).expect("builds A");
        let package_b = build_package(&source, &options_b).expect("builds B");

        let png_a = package_a
            .files
            .iter()
            .find(|f| f.name == "favicon-32x32.png")
            .expect("favicon 32 in A");
        let img_a = image::load_from_memory(&png_a.bytes)
            .expect("decodes")
            .to_rgba8();
        assert_eq!(
            img_a.get_pixel(0, 0).0[3],
            0,
            "padding leaves the corner transparent"
        );
        assert_eq!(img_a.get_pixel(16, 16).0[3], 255, "the center stays opaque");

        let png_b = package_b
            .files
            .iter()
            .find(|f| f.name == "favicon-32x32.png")
            .expect("favicon 32 in B");
        let img_b = image::load_from_memory(&png_b.bytes)
            .expect("decodes")
            .to_rgba8();
        assert_eq!(
            img_b.get_pixel(0, 0).0[3],
            255,
            "without padding the corner is the logo"
        );

        let ico_a = package_a
            .files
            .iter()
            .find(|f| f.name == "favicon.ico")
            .expect("ico A");
        let ico_b = package_b
            .files
            .iter()
            .find(|f| f.name == "favicon.ico")
            .expect("ico B");
        assert_ne!(
            ico_a.bytes, ico_b.bytes,
            "padding must change the .ico bytes"
        );
        let png16_a = package_a
            .files
            .iter()
            .find(|f| f.name == "favicon-16x16.png")
            .expect("16 A");
        let png16_b = package_b
            .files
            .iter()
            .find(|f| f.name == "favicon-16x16.png")
            .expect("16 B");
        assert_ne!(
            png16_a.bytes, png16_b.bytes,
            "padding must change the 16x16 bytes"
        );
    }

    /// The second option only touches the home screen icons: 25 % padding on
    /// a 400x400 logo gives white edges on the (opaque) Apple icon and
    /// transparent bars on the Android icons.
    #[test]
    fn app_padding_is_applied_to_apple_and_android() {
        let options = FaviconsOptions {
            app_padding_percent: 25.0,
            padding_percent: 0.0,
            trim_transparent: false,
            ..Default::default()
        };
        let package = build_package(&raster_source(400, 400), &options).expect("builds");

        let apple = image::load_from_memory(file_bytes(&package, "apple-touch-icon.png"))
            .expect("decodes")
            .to_rgba8();
        assert_eq!((apple.width(), apple.height()), (180, 180));
        assert_eq!(apple.get_pixel(0, 0), &Rgba([255, 255, 255, 255]));
        assert_eq!(apple.get_pixel(90, 90), &Rgba([255, 0, 0, 255]));

        let android_192 =
            image::load_from_memory(file_bytes(&package, "android-chrome-192x192.png"))
                .expect("decodes")
                .to_rgba8();
        assert_eq!(android_192.get_pixel(0, 0).0[3], 0);
        assert_eq!(android_192.get_pixel(96, 96).0[3], 255);

        let android_512 =
            image::load_from_memory(file_bytes(&package, "android-chrome-512x512.png"))
                .expect("decodes")
                .to_rgba8();
        assert_eq!(android_512.get_pixel(0, 0).0[3], 0);
        assert_eq!(android_512.get_pixel(256, 256).0[3], 255);
    }

    #[test]
    fn app_padding_zero_keeps_logo_at_the_edges() {
        let options = FaviconsOptions {
            app_padding_percent: 0.0,
            padding_percent: 0.0,
            trim_transparent: false,
            ..Default::default()
        };
        let package = build_package(&raster_source(400, 400), &options).expect("builds");
        let apple = image::load_from_memory(file_bytes(&package, "apple-touch-icon.png"))
            .expect("decodes")
            .to_rgba8();
        assert_eq!(apple.get_pixel(0, 0), &Rgba([255, 0, 0, 255]));
        let android_512 =
            image::load_from_memory(file_bytes(&package, "android-chrome-512x512.png"))
                .expect("decodes")
                .to_rgba8();
        assert_eq!(android_512.get_pixel(0, 0).0[3], 255);
    }

    #[test]
    fn app_padding_does_not_change_favicon_or_maskable_files() {
        let build = |app_padding: f32| FaviconsOptions {
            app_padding_percent: app_padding,
            padding_percent: 0.0,
            trim_transparent: false,
            ..Default::default()
        };
        let zero = build_package(&raster_source(400, 400), &build(0.0)).expect("builds");
        let forty = build_package(&raster_source(400, 400), &build(40.0)).expect("builds");
        for name in [
            "favicon.ico",
            "favicon-16x16.png",
            "favicon-32x32.png",
            "android-chrome-maskable-512x512.png",
        ] {
            assert_eq!(file_bytes(&zero, name), file_bytes(&forty, name), "{name}");
        }
        assert_ne!(
            file_bytes(&zero, "apple-touch-icon.png"),
            file_bytes(&forty, "apple-touch-icon.png"),
            "apple must change with app padding"
        );
    }

    #[test]
    fn favicon_padding_does_not_change_app_icons() {
        let build = |favicon_padding: f32| FaviconsOptions {
            padding_percent: favicon_padding,
            app_padding_percent: 10.0,
            trim_transparent: false,
            ..Default::default()
        };
        let zero = build_package(&raster_source(400, 400), &build(0.0)).expect("builds");
        let forty = build_package(&raster_source(400, 400), &build(40.0)).expect("builds");
        for name in [
            "apple-touch-icon.png",
            "android-chrome-192x192.png",
            "android-chrome-512x512.png",
        ] {
            assert_eq!(file_bytes(&zero, name), file_bytes(&forty, name), "{name}");
        }
        assert_ne!(
            file_bytes(&zero, "favicon-32x32.png"),
            file_bytes(&forty, "favicon-32x32.png"),
            "the favicon 32 must change with favicon padding"
        );
    }

    #[test]
    fn app_padding_is_clamped() {
        let build = |app_padding: f32| FaviconsOptions {
            app_padding_percent: app_padding,
            padding_percent: 0.0,
            trim_transparent: false,
            ..Default::default()
        };
        let clamp_45 = build_package(&raster_source(400, 400), &build(45.0)).expect("builds");
        let clamp_99 = build_package(&raster_source(400, 400), &build(99.0)).expect("builds");
        assert_eq!(
            file_bytes(&clamp_45, "apple-touch-icon.png"),
            file_bytes(&clamp_99, "apple-touch-icon.png"),
            "99 clamps to 45"
        );
        let zero = build_package(&raster_source(400, 400), &build(0.0)).expect("builds");
        let nan = build_package(&raster_source(400, 400), &build(f32::NAN)).expect("builds");
        assert_eq!(
            file_bytes(&zero, "apple-touch-icon.png"),
            file_bytes(&nan, "apple-touch-icon.png"),
            "NaN behaves like 0"
        );
    }

    #[test]
    fn default_options_values() {
        let options = FaviconsOptions::default();
        assert_eq!(options.padding_percent, 0.0);
        assert_eq!(options.app_padding_percent, 10.0);
    }

    #[test]
    fn build_package_defaults_for_a_wide_raster() {
        let package =
            build_package(&raster_source(600, 400), &FaviconsOptions::default()).expect("builds");
        let names: Vec<&str> = package.files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "favicon.ico",
                "favicon-16x16.png",
                "favicon-32x32.png",
                "apple-touch-icon.png",
                "android-chrome-192x192.png",
                "android-chrome-512x512.png",
                "android-chrome-maskable-512x512.png",
                "site.webmanifest",
                "favicon-snippet.html",
            ]
        );

        let expected_sizes = [
            ("favicon-16x16.png", (16, 16)),
            ("favicon-32x32.png", (32, 32)),
            ("apple-touch-icon.png", (180, 180)),
            ("android-chrome-192x192.png", (192, 192)),
            ("android-chrome-512x512.png", (512, 512)),
            ("android-chrome-maskable-512x512.png", (512, 512)),
        ];
        for (name, (w, h)) in expected_sizes {
            let file = package.files.iter().find(|f| f.name == name).expect(name);
            let img = image::load_from_memory(&file.bytes).expect("decodes");
            assert_eq!((img.width(), img.height()), (w, h), "{name}");
        }

        for name in [
            "apple-touch-icon.png",
            "android-chrome-maskable-512x512.png",
        ] {
            let file = package.files.iter().find(|f| f.name == name).expect(name);
            let img = image::load_from_memory(&file.bytes)
                .expect("decodes")
                .to_rgba8();
            assert!(img.pixels().all(|p| p.0[3] == 255), "{name} not opaque");
        }

        // The wide source leaves transparent bars on the android icons.
        let file = package
            .files
            .iter()
            .find(|f| f.name == "android-chrome-512x512.png")
            .expect("android 512");
        let img = image::load_from_memory(&file.bytes)
            .expect("decodes")
            .to_rgba8();
        assert_eq!(img.get_pixel(0, 0), &Rgba([0, 0, 0, 0]));
    }

    #[test]
    fn the_non_square_warning_tracks_the_ratio() {
        let wide =
            build_package(&raster_source(600, 400), &FaviconsOptions::default()).expect("builds");
        assert!(
            wide.warnings
                .iter()
                .any(|w| w.contains("The image is not square")),
            "warnings: {:?}",
            wide.warnings
        );

        let square =
            build_package(&raster_source(500, 500), &FaviconsOptions::default()).expect("builds");
        assert!(
            !square
                .warnings
                .iter()
                .any(|w| w.contains("The image is not square"))
        );
    }

    #[test]
    fn trim_changes_whether_the_corner_is_opaque() {
        // 100x100 transparent with an opaque 10x10 square in the middle.
        let mut source = raster_source(100, 100);
        source.master = solid_master(100, 100, [0, 0, 0, 0]);
        for py in 45..55 {
            for px in 45..55 {
                source.master.put_pixel(px, py, Rgba([255, 0, 0, 255]));
            }
        }

        let mut options = FaviconsOptions::default();
        options.trim_transparent = true;
        let trimmed = build_package(&source, &options).expect("builds");
        let file = trimmed
            .files
            .iter()
            .find(|f| f.name == "favicon-32x32.png")
            .expect("favicon 32");
        let img = image::load_from_memory(&file.bytes)
            .expect("decodes")
            .to_rgba8();
        assert_eq!(img.get_pixel(16, 16).0[3], 255);
        assert_eq!(img.get_pixel(0, 0).0[3], 255);

        options.trim_transparent = false;
        let untrimmed = build_package(&source, &options).expect("builds");
        let file = untrimmed
            .files
            .iter()
            .find(|f| f.name == "favicon-32x32.png")
            .expect("favicon 32");
        let img = image::load_from_memory(&file.bytes)
            .expect("decodes")
            .to_rgba8();
        assert_eq!(img.get_pixel(0, 0).0[3], 0);
        assert_eq!(img.get_pixel(16, 16).0[3], 255);
    }

    #[test]
    fn the_favicon_ico_has_three_frames_16_32_48() {
        let package =
            build_package(&raster_source(500, 500), &FaviconsOptions::default()).expect("builds");
        let file = package
            .files
            .iter()
            .find(|f| f.name == "favicon.ico")
            .expect("ico present");
        assert_eq!(&file.bytes[0..2], &[0, 0], "reserved");
        assert_eq!(&file.bytes[2..4], &[1, 0], "type icon");
        assert_eq!(&file.bytes[4..6], &[3, 0], "count");
        assert_eq!(
            [file.bytes[6], file.bytes[22], file.bytes[38]],
            [16, 32, 48]
        );
    }

    #[test]
    fn maskable_can_be_disabled() {
        let mut options = FaviconsOptions::default();
        options.include_maskable = false;
        let package = build_package(&raster_source(500, 500), &options).expect("builds");
        assert!(!package.files.iter().any(|f| f.name.contains("maskable")));
        let manifest = package
            .files
            .iter()
            .find(|f| f.name == "site.webmanifest")
            .expect("manifest");
        let value: serde_json::Value = serde_json::from_slice(&manifest.bytes).expect("parses");
        let icons = value["icons"].as_array().expect("icons");
        assert!(icons.iter().all(|icon| icon["purpose"] != "maskable"));
    }

    #[test]
    fn svg_sources_keep_the_original_bytes_and_snippet_line() {
        let mut source = raster_source(500, 500);
        source.kind = SourceKind::Svg;
        source.format_label = "SVG".to_string();
        source.svg_bytes = Some(b"<svg/>".to_vec());

        let package = build_package(&source, &FaviconsOptions::default()).expect("builds");
        let names: Vec<&str> = package.files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names[3], "favicon.svg");
        let svg_file = package
            .files
            .iter()
            .find(|f| f.name == "favicon.svg")
            .expect("svg file");
        assert_eq!(svg_file.bytes, b"<svg/>".to_vec());
        assert!(package.html_snippet.contains("/favicon.svg"));

        let raster_package =
            build_package(&raster_source(500, 500), &FaviconsOptions::default()).expect("builds");
        assert!(!raster_package.html_snippet.contains("favicon.svg"));
        assert!(!raster_package.files.iter().any(|f| f.name == "favicon.svg"));
    }

    #[test]
    fn manifest_round_trip_and_quoting() {
        let manifest = build_manifest("My \"Best\" Site", "#112233", "#445566", true);
        let value: serde_json::Value = serde_json::from_str(&manifest).expect("valid JSON");
        assert_eq!(value["name"], "My \"Best\" Site");
        assert_eq!(value["short_name"], "My \"Best\" Site");
        assert_eq!(value["theme_color"], "#112233");
        assert_eq!(value["background_color"], "#445566");
        assert_eq!(value["display"], "standalone");
        let icons = value["icons"].as_array().expect("icons");
        assert_eq!(icons.len(), 3);
        assert_eq!(icons[0]["src"], "/android-chrome-192x192.png");
        assert_eq!(icons[0]["sizes"], "192x192");
        assert_eq!(icons[1]["src"], "/android-chrome-512x512.png");
        assert_eq!(icons[2]["purpose"], "maskable");

        let empty = build_manifest("   ", "#112233", "#445566", false);
        let value: serde_json::Value = serde_json::from_str(&empty).expect("valid JSON");
        assert_eq!(value["name"], "Website");
        assert_eq!(value["icons"].as_array().expect("icons").len(), 2);
    }

    #[test]
    fn html_snippet_contents() {
        let snippet = build_html_snippet(true, "#ff8800");
        assert!(snippet.contains("#ff8800"));
        assert!(snippet.ends_with('\n'));
        assert!(snippet.contains("rel=\"apple-touch-icon\""));
        assert!(snippet.contains("rel=\"manifest\""));
        assert!(snippet.contains("/favicon.svg"));
        assert_eq!(snippet.lines().count(), 7);

        let raster_snippet = build_html_snippet(false, "#ff8800");
        assert!(!raster_snippet.contains("/favicon.svg"));
        assert_eq!(raster_snippet.lines().count(), 6);
    }

    #[test]
    fn an_invalid_color_fails_before_image_work() {
        let mut options = FaviconsOptions::default();
        options.background_color = "red".to_string();
        let error = build_package(&raster_source(500, 500), &options).unwrap_err();
        assert!(error.contains("Invalid color"), "{error}");
    }

    #[test]
    fn write_package_creates_folder_and_files() {
        let parent = temp_dir("write");
        let package =
            build_package(&raster_source(500, 500), &FaviconsOptions::default()).expect("builds");

        let folder = write_package(&parent, "My Site", &package).expect("writes");
        assert!(folder.ends_with("My_Site_favicon_package"));
        assert!(folder.is_dir());
        for file in &package.files {
            let path = folder.join(&file.name);
            assert!(path.is_file(), "{} missing", file.name);
            let bytes = std::fs::read(&path).expect("reads");
            assert_eq!(bytes, file.bytes, "{} bytes differ", file.name);
        }

        // The second call must not touch the first folder.
        let folder2 = write_package(&parent, "My Site", &package).expect("writes");
        assert!(folder2.ends_with("My_Site_favicon_package_1"));
        let _ = std::fs::remove_dir_all(&parent);
    }

    #[test]
    fn write_package_needs_an_existing_parent() {
        let parent = std::env::temp_dir().join(format!(
            "erx_site_icons_test_{}_missing_parent",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&parent);
        let package =
            build_package(&raster_source(100, 100), &FaviconsOptions::default()).expect("builds");
        let result = write_package(&parent, "site", &package);
        assert!(result.is_err());
        assert!(!parent.exists(), "nothing may be created");
    }
}
