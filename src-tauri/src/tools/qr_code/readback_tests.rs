//! Read-back tests: generate a QR code, render it to a PNG, decode the PNG
//! with an independent QR decoder (`rqrr`, dev-dependency only — it never
//! ships in the app) and assert the decoded text equals the input.
//!
//! 3A note: the PNG here is rendered by a test-only rasterizer that fills
//! each dark matrix module as a square (the sharp style). When the real
//! PNG renderer lands in 3B, these tests move over to it so the rounded
//! geometry and colors are covered end-to-end as well.

use super::core::{self, BuildRequest, ContentInput};
use super::matrix::Matrix;

/// Renders a matrix to a PNG in memory: white background, black modules,
/// `scale` pixels per module.
fn render_matrix_png(matrix: &Matrix, margin: usize, scale: usize) -> Vec<u8> {
    use image::Luma;

    let total = (matrix.size() + 2 * margin) as u32;
    let side = total * scale as u32;
    let mut img = image::GrayImage::from_pixel(side, side, Luma([255u8]));
    for y in 0..matrix.size() {
        for x in 0..matrix.size() {
            if !matrix.is_dark(x as isize, y as isize) {
                continue;
            }
            for dy in 0..scale {
                for dx in 0..scale {
                    img.put_pixel(
                        ((margin + x) * scale + dx) as u32,
                        ((margin + y) * scale + dy) as u32,
                        Luma([0u8]),
                    );
                }
            }
        }
    }
    let mut bytes = Vec::new();
    image::DynamicImage::ImageLuma8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("PNG encode of a synthetic image always works");
    bytes
}

/// Decodes the first QR code found in a PNG and returns its text.
fn decode_png_png_text(bytes: &[u8]) -> String {
    let img = image::load_from_memory(bytes)
        .expect("our own PNG must decode")
        .to_luma8();
    let (width, height) = (img.width() as usize, img.height() as usize);
    let mut prepared = rqrr::PreparedImage::prepare_from_greyscale(width, height, |x, y| {
        img.get_pixel(x as u32, y as u32).0[0]
    });
    let grids = prepared.detect_grids();
    assert!(!grids.is_empty(), "no QR code found in the rendered image");
    let (_meta, text) = grids[0]
        .decode()
        .expect("a freshly generated QR code must decode");
    text
}

/// Full round trip: pipeline request → matrix → PNG → decoded text.
fn roundtrip_text(request: &BuildRequest) -> String {
    let built = core::build(request).expect("request must build");
    // Rebuild the matrix from the payload so the test rasterizer can draw
    // it; the pipeline itself already validated that it fits.
    let payload = request.content.payload().expect("payload was valid");
    let ecc = built.level_used;
    let matrix = Matrix::from_text(&payload, ecc).expect("matrix fits (already validated)");
    let png = render_matrix_png(&matrix, 4, 12);
    decode_png_png_text(&png)
}

fn text_request(content: ContentInput, ecc: &str) -> BuildRequest {
    BuildRequest {
        content,
        style: "sharp".into(),
        foreground: "#000000".into(),
        background: "#ffffff".into(),
        transparent: false,
        error_correction: ecc.into(),
        margin: 4,
    }
}

#[test]
fn url_survives_at_every_error_correction_level() {
    let url = ContentInput::Website("https://example.com/erx-tools".into());
    for level in ["L", "M", "Q", "H"] {
        let payload = url.payload().unwrap();
        let decoded = roundtrip_text(&text_request(url.clone(), level));
        assert_eq!(decoded, payload, "level {level}");
    }
}

#[test]
fn persian_text_survives_as_utf8() {
    let content = ContentInput::Text("سلام، این یک آزمون است!".into());
    let payload = content.payload().unwrap();
    let decoded = roundtrip_text(&text_request(content, "Q"));
    assert_eq!(decoded, payload);
}

#[test]
fn multiline_text_survives() {
    let content = ContentInput::Text("first line\nsecond line\n\ttabbed".into());
    let decoded = roundtrip_text(&text_request(content, "M"));
    assert_eq!(decoded, "first line\nsecond line\n\ttabbed");
}

#[test]
fn wifi_payload_with_special_characters_survives() {
    // The payload is built (and escaped) by the same Rust code the app
    // uses; the decoded text must be byte-identical.
    let content = ContentInput::Wifi {
        ssid: "net;work".into(),
        password: "p\\a,s:s\"w".into(),
        security: "wpa".into(),
        hidden: true,
    };
    let payload = content.payload().unwrap();
    assert!(payload.contains(';'), "payload shape sanity");
    let decoded = roundtrip_text(&text_request(content, "Q"));
    assert_eq!(decoded, payload);
}

#[test]
fn highest_density_content_still_decodes() {
    // Close to the Q limit: long content at a high level stresses the
    // decoder with a dense code.
    let text = "x".repeat(1500);
    let content = ContentInput::Text(text.clone());
    let decoded = roundtrip_text(&text_request(content, "Q"));
    assert_eq!(decoded, text);
}
