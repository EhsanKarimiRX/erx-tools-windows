//! Read-back tests: generate a QR code, render it with the REAL PNG
//! rasterizer (the exact code path Save PNG uses), decode the PNG with an
//! independent QR decoder (`rqrr`, dev-dependency only — it never ships in
//! the app) and assert the decoded text equals the input.
//!
//! Covers both styles, every error correction level, the three content
//! types, custom colors, transparent backgrounds and inverted codes.
//! (Inverted codes are read here by inverting pixels first — real-world
//! scanner support for inverted codes varies, which is why the app shows
//! a warning for them.)

use super::core::{self, BuildRequest, ContentInput};

/// Full pipeline: request → build → real PNG render → rqrr decode.
fn roundtrip(request: &BuildRequest) -> String {
    let built = core::build(request).expect("request must build");
    let png = super::raster::render_png(
        &built.shapes,
        built.total_modules,
        built.foreground,
        built.background,
        768, // comfortable density for the decoder
        built.plate.as_ref(),
        None,
    )
    .expect("real renderer must work");
    decode_png_text(&png)
}

/// Decodes the first QR code found in a PNG.
fn decode_png_text(bytes: &[u8]) -> String {
    let img = image::load_from_memory(bytes)
        .expect("our own PNG must decode")
        .to_luma8();
    let (width, height) = (img.width() as usize, img.height() as usize);
    let mut prepared = rqrr::PreparedImage::prepare_from_greyscale(width, height, |x, y| {
        img.get_pixel(x as u32, y as u32).0[0]
    });
    let grids = prepared.detect_grids();
    assert!(!grids.is_empty(), "no QR code found in the rendered image");
    if grids.len() > 1 {
        println!("PROBE: {} grids found", grids.len());
    }
    let (_meta, text) = grids[0]
        .decode()
        .expect("a freshly generated QR code must decode");
    text
}

/// Decodes after compositing an RGBA PNG onto white (for transparent
/// backgrounds) and optionally inverting (for inverted codes).
fn decode_composited(bytes: &[u8], invert: bool) -> String {
    let rgba = image::load_from_memory(bytes)
        .expect("our own PNG must decode")
        .to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    let mut prepared =
        rqrr::PreparedImage::prepare_from_greyscale(width as usize, height as usize, |x, y| {
            let [r, g, b, a] = rgba.get_pixel(x as u32, y as u32).0;
            let [r, g, b] = if a == 255 {
                [r, g, b]
            } else {
                // Alpha blend over white.
                let blend = |c: u8| {
                    u8::try_from((u16::from(c) * u16::from(a) + 255 * (255 - u16::from(a))) / 255)
                        .unwrap_or(255)
                };
                [blend(r), blend(g), blend(b)]
            };
            // Luma, optionally inverted for light-on-dark codes.
            let luma = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
            let luma = if invert { 255.0 - luma } else { luma };
            luma.round() as u8
        });
    let grids = prepared.detect_grids();
    assert!(!grids.is_empty(), "no QR code found (composited)");
    let (_meta, text) = grids[0]
        .decode()
        .expect("a freshly generated QR code must decode");
    text
}

fn request(content: ContentInput) -> BuildRequest {
    BuildRequest {
        content,
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
fn url_survives_at_every_level_and_style() {
    let url = ContentInput::Website("https://example.com/erx-tools".into());
    let payload = url.payload().unwrap();
    for level in ["L", "M", "Q", "H"] {
        for style in ["sharp", "round"] {
            let mut req = request(url.clone());
            req.error_correction = level.into();
            req.style = style.into();
            assert_eq!(roundtrip(&req), payload, "level {level}, style {style}");
        }
    }
}

#[test]
fn persian_text_survives_in_both_styles() {
    let content = ContentInput::Text("سلام، این یک آزمون است!".into());
    let payload = content.payload().unwrap();
    for style in ["sharp", "round"] {
        let mut req = request(content.clone());
        req.style = style.into();
        assert_eq!(roundtrip(&req), payload, "style {style}");
    }
}

#[test]
fn wifi_payload_with_special_characters_survives() {
    let content = ContentInput::Wifi {
        ssid: "net;work".into(),
        password: "p\\a,s:s\"w".into(),
        security: "wpa".into(),
        hidden: true,
    };
    let payload = content.payload().unwrap();
    assert!(payload.contains(';'), "payload shape sanity");
    let decoded = roundtrip(&request(content));
    assert_eq!(decoded, payload);
}

#[test]
fn custom_dark_on_light_colors_survive() {
    // Not black/white: a dark blue code on a light cream background.
    let mut req = request(ContentInput::Text("colored".into()));
    req.foreground = "#1a2b5f".into();
    req.background = "#fdf6e3".into();
    assert_eq!(roundtrip(&req), "colored");
}

#[test]
fn transparent_background_survives_composited_on_white() {
    let mut req = request(ContentInput::Text("clear".into()));
    req.transparent = true;
    let built = core::build(&req).expect("builds");
    let png = super::raster::render_png(
        &built.shapes,
        built.total_modules,
        built.foreground,
        built.background,
        768,
        built.plate.as_ref(),
        None,
    )
    .expect("renders");
    assert_eq!(decode_composited(&png, false), "clear");
}

#[test]
fn inverted_code_survives_after_pixel_inversion() {
    // White code on black: real scanners vary on inverted codes (the app
    // warns about it); the test inverts pixels first, as tolerant scanners
    // effectively do.
    let mut req = request(ContentInput::Text("dark mode".into()));
    req.foreground = "#ffffff".into();
    req.background = "#000000".into();
    let built = core::build(&req).expect("builds");
    let png = super::raster::render_png(
        &built.shapes,
        built.total_modules,
        built.foreground,
        built.background,
        768,
        built.plate.as_ref(),
        None,
    )
    .expect("renders");
    assert_eq!(decode_composited(&png, true), "dark mode");
}

#[test]
fn logo_code_still_decodes_and_forces_h() {
    // A centered logo at a middle size must not break scanning.
    let img = image::RgbaImage::from_pixel(512, 512, image::Rgba([180, 30, 30, 255]));
    let mut png_bytes = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    let logo_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, png_bytes);

    let mut req = request(ContentInput::Website("https://example.com/logo".into()));
    req.error_correction = "Q".into(); // must be forced up to H
    req.logo = Some(core::LogoInput {
        png_base64: logo_b64,
        fraction: 0.15,
    });
    let built = core::build(&req).expect("builds");
    assert_eq!(built.level_used, super::matrix::Ecc::High);
    let png = super::raster::render_png(
        &built.shapes,
        built.total_modules,
        built.foreground,
        built.background,
        768,
        built.plate.as_ref(),
        built.logo.as_ref(),
    )
    .expect("renders");
    assert_eq!(decode_png_text(&png), "https://example.com/logo");
}

#[test]
fn logo_with_margin_4_and_exact_export_size_decodes() {
    // Regression for the centered-hole fix (margin 4 was enough to knock
    // the cleared area off the logo) and the exact export size: the PNG
    // must be exactly the requested side and still decode.
    let img = image::RgbaImage::from_pixel(512, 512, image::Rgba([30, 120, 200, 255]));
    let mut png_bytes = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    let logo_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, png_bytes);

    let mut req = request(ContentInput::Website("https://example.com/margin4".into()));
    req.error_correction = "Q".into(); // must be forced up to H
    req.margin = 4;
    req.logo = Some(core::LogoInput {
        png_base64: logo_b64,
        fraction: 0.10,
    });
    let built = core::build(&req).expect("builds");
    assert_eq!(built.level_used, super::matrix::Ecc::High);
    let png = super::raster::render_png(
        &built.shapes,
        built.total_modules,
        built.foreground,
        built.background,
        built.png_pixels, // the exact requested export size
        built.plate.as_ref(),
        built.logo.as_ref(),
    )
    .expect("renders");
    let decoded = image::load_from_memory(&png).expect("our PNG decodes");
    assert_eq!(
        (decoded.width(), decoded.height()),
        (built.png_pixels, built.png_pixels)
    );
    assert_eq!(decode_png_text(&png), "https://example.com/margin4");
}

#[test]
fn rounded_style_with_logo_and_plate_at_1080_decodes() {
    // Rounded style + logo + margin 4, exported at the default 1080: the
    // rounded plate must not break scanning, and the file must be exactly
    // 1080 × 1080.
    let img = image::RgbaImage::from_pixel(512, 512, image::Rgba([180, 30, 30, 255]));
    let mut png_bytes = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    let logo_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, png_bytes);

    let mut req = request(ContentInput::Website("https://example.com/rounded".into()));
    req.style = "round".into();
    req.error_correction = "Q".into(); // must be forced up to H
    req.margin = 4;
    req.logo = Some(core::LogoInput {
        png_base64: logo_b64,
        fraction: 0.15,
    });
    let built = core::build(&req).expect("builds");
    assert_eq!(built.level_used, super::matrix::Ecc::High);
    assert!(
        built.plate.is_some(),
        "rounded style with an opaque background must draw the plate"
    );
    let png = super::raster::render_png(
        &built.shapes,
        built.total_modules,
        built.foreground,
        built.background,
        built.png_pixels,
        built.plate.as_ref(),
        built.logo.as_ref(),
    )
    .expect("renders");
    let decoded = image::load_from_memory(&png).expect("our PNG decodes");
    assert_eq!((decoded.width(), decoded.height()), (1080, 1080));
    assert_eq!(decode_png_text(&png), "https://example.com/rounded");
}

#[test]
fn highest_density_content_still_decodes() {
    // Close to the Q limit: long content at a high level stresses the
    // decoder with a dense code.
    let text = "x".repeat(1500);
    let decoded = roundtrip(&request(ContentInput::Text(text.clone())));
    assert_eq!(decoded, text);
}

#[test]
fn round_and_sharp_render_same_payload_from_same_geometry() {
    // Both styles must decode identically — same matrix, different shapes.
    let content = ContentInput::Website("https://example.com/styles".into());
    let mut sharp = request(content.clone());
    sharp.style = "sharp".into();
    let mut round = request(content);
    round.style = "round".into();
    assert_eq!(roundtrip(&sharp), roundtrip(&round));
}
