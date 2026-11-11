//! Pure image helpers for Favicons: trim, square canvas, padding,
//! alpha-correct resize, flatten, maskable icon. Spec: docs/PROMPT_11_SPEC.md section 5.

use image::RgbaImage;

/// Crops the image to the bounding box of all pixels with alpha > 0.
/// A fully transparent image is an error (nothing would remain).
pub fn trim_transparent(img: &RgbaImage) -> Result<RgbaImage, String> {
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0;
    let mut max_y = 0;
    for (x, y, pixel) in img.enumerate_pixels() {
        if pixel.0[3] > 0 {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    if min_x == u32::MAX {
        return Err("The image is fully transparent.".into());
    }
    Ok(
        image::imageops::crop_imm(img, min_x, min_y, max_x - min_x + 1, max_y - min_y + 1)
            .to_image(),
    )
}

/// Centers `img` on a transparent square canvas whose side is the longer
/// side of `img`. The canvas is fully transparent, so pixels are copied
/// directly without blending.
pub fn make_square(img: &RgbaImage) -> RgbaImage {
    let side = img.width().max(img.height());
    let mut canvas = image::RgbaImage::new(side, side);
    paste_centered(&mut canvas, img);
    canvas
}

/// Pastes `img` centered onto `canvas`, overwriting the covered pixels
/// (no blending). `img` must not be larger than the canvas.
pub fn paste_centered(canvas: &mut RgbaImage, img: &RgbaImage) {
    debug_assert!(
        img.width() <= canvas.width() && img.height() <= canvas.height(),
        "pasted image must not be larger than the canvas"
    );
    let x = canvas.width().saturating_sub(img.width()) / 2;
    let y = canvas.height().saturating_sub(img.height()) / 2;
    for (px, py, pixel) in img.enumerate_pixels() {
        canvas.put_pixel(x + px, y + py, *pixel);
    }
}

/// Shrinks the square base by the given padding percent on every side and
/// centers it back on a transparent canvas of the original size.
/// Only used for the favicon files (ico, 16, 32) — never for the
/// apple/android icons. NaN counts as 0; the percent is clamped to 0..=45.
pub fn apply_padding(base: &RgbaImage, percent: f32) -> RgbaImage {
    let percent = if percent.is_nan() {
        0.0
    } else {
        percent.clamp(0.0, 45.0)
    };
    if percent == 0.0 {
        return base.clone();
    }
    let side = base.width();
    // content = max(1, round(side * (100 - 2*p) / 100))
    let content = ((side as f32 * (100.0 - 2.0 * percent) / 100.0).round() as u32).max(1);
    let resized = resize_square(base, content);
    let mut canvas = image::RgbaImage::new(side, side);
    paste_centered(&mut canvas, &resized);
    canvas
}

/// Resizes a square image with premultiplied-alpha handling so that
/// transparent edges do not pick up dark fringes. Returns a clone when the
/// image already has the requested size.
pub fn resize_square(img: &RgbaImage, size: u32) -> RgbaImage {
    if img.width() == size && img.height() == size {
        return img.clone();
    }
    // 1. premultiply alpha (adapted from the spec reference: image 0.25's
    //    Rgba<u8> is a plain tuple struct without Index, so `.0[c]`).
    let mut pre = img.clone();
    for p in pre.pixels_mut() {
        let a = u32::from(p.0[3]);
        for c in 0..3 {
            p.0[c] = ((u32::from(p.0[c]) * a + 127) / 255) as u8;
        }
    }
    // 2. resize
    let mut out = image::imageops::resize(&pre, size, size, image::imageops::FilterType::Lanczos3);
    // 3. un-premultiply (checked_div for a == 0 keeps the pixel at 0, which
    //    is exactly the same result as the spec's explicit zero branch).
    for p in out.pixels_mut() {
        let a = u32::from(p.0[3]);
        for c in 0..3 {
            p.0[c] = ((u32::from(p.0[c]) * 255) + a / 2)
                .checked_div(a)
                .map_or(0, |value| value.min(255)) as u8;
        }
    }
    out
}

/// Blends the image over an opaque `rgb` background: per channel
/// out = (src * a + bg * (255 - a) + 127) / 255, output alpha 255.
pub fn flatten_on_color(img: &RgbaImage, rgb: [u8; 3]) -> RgbaImage {
    let mut out = img.clone();
    for p in out.pixels_mut() {
        let a = u32::from(p.0[3]);
        for (c, bg) in p.0[0..3].iter_mut().zip(rgb.iter()) {
            *c = ((u32::from(*c) * a + u32::from(*bg) * (255 - a) + 127) / 255) as u8;
        }
        p.0[3] = 255;
    }
    out
}

/// Builds an Android maskable icon: the logo scaled to 60% of the canvas
/// (20% padding per side), flattened on an opaque background color.
pub fn make_maskable(base: &RgbaImage, size: u32, rgb: [u8; 3]) -> RgbaImage {
    let content = ((size as f32 * 0.6).round() as u32).max(1);
    let resized = resize_square(base, content);
    let mut canvas = image::RgbaImage::new(size, size);
    paste_centered(&mut canvas, &resized);
    flatten_on_color(&canvas, rgb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn solid(width: u32, height: u32, rgba: [u8; 4]) -> RgbaImage {
        let mut img = RgbaImage::new(width, height);
        for p in img.pixels_mut() {
            *p = Rgba(rgba);
        }
        img
    }

    fn fill_rect(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, rgba: [u8; 4]) {
        for py in y..y + h {
            for px in x..x + w {
                img.put_pixel(px, py, Rgba(rgba));
            }
        }
    }

    #[test]
    fn trim_finds_the_exact_box() {
        let mut img = solid(100, 100, [0, 0, 0, 0]);
        fill_rect(&mut img, 30, 40, 20, 10, [10, 20, 30, 255]);
        let trimmed = trim_transparent(&img).expect("trims");
        assert_eq!((trimmed.width(), trimmed.height()), (20, 10));
        assert!(trimmed.pixels().all(|p| p.0[3] == 255));
    }

    #[test]
    fn trim_of_a_fully_transparent_image_is_an_error() {
        let img = solid(50, 50, [0, 0, 0, 0]);
        assert_eq!(
            trim_transparent(&img).unwrap_err(),
            "The image is fully transparent."
        );
    }

    #[test]
    fn make_square_centers_wide_and_tall_images() {
        // 40x20 -> 40x40; the content starts at y = 10, x = 0.
        let square = make_square(&solid(40, 20, [255, 0, 0, 255]));
        assert_eq!((square.width(), square.height()), (40, 40));
        assert_eq!(square.get_pixel(0, 10).0[3], 255);
        assert_eq!(square.get_pixel(0, 9).0[3], 0);
        // 20x40 -> 40x40; the content starts at x = 10, y = 0.
        let square = make_square(&solid(20, 40, [255, 0, 0, 255]));
        assert_eq!((square.width(), square.height()), (40, 40));
        assert_eq!(square.get_pixel(10, 0).0[3], 255);
        assert_eq!(square.get_pixel(9, 0).0[3], 0);
    }

    #[test]
    fn apply_padding_zero_returns_identical_pixels() {
        let img = solid(64, 64, [1, 2, 3, 255]);
        assert_eq!(apply_padding(&img, 0.0).as_raw(), img.as_raw());
    }

    #[test]
    fn apply_padding_nan_behaves_like_zero() {
        let img = solid(64, 64, [9, 8, 7, 128]);
        assert_eq!(apply_padding(&img, f32::NAN).as_raw(), img.as_raw());
    }

    #[test]
    fn apply_padding_25_shrinks_the_content_to_half() {
        let img = solid(400, 400, [255, 0, 0, 255]);
        let padded = apply_padding(&img, 25.0);
        assert_eq!((padded.width(), padded.height()), (400, 400));
        // content = round(400 * 50 / 100) = 200, centered at (100, 100).
        assert_eq!(padded.get_pixel(200, 200).0[3], 255);
        assert_eq!(padded.get_pixel(101, 101).0[3], 255);
        assert_eq!(padded.get_pixel(95, 95).0[3], 0);
        assert_eq!(padded.get_pixel(10, 10).0[3], 0);
        assert_eq!(padded.get_pixel(100, 100).0[3], 255);
        assert_eq!(padded.get_pixel(299, 299).0[3], 255);
        assert_eq!(padded.get_pixel(300, 300).0[3], 0);
    }

    #[test]
    fn apply_padding_clamps_to_45() {
        let img = solid(400, 400, [255, 0, 0, 255]);
        let p99 = apply_padding(&img, 99.0);
        let p45 = apply_padding(&img, 45.0);
        assert_eq!(p99.as_raw(), p45.as_raw());
        // content = round(400 * 10 / 100) = 40, spanning 180..219.
        assert_eq!(p99.get_pixel(181, 200).0[3], 255);
        assert_eq!(p99.get_pixel(179, 200).0[3], 0);
    }

    #[test]
    fn resize_square_has_no_dark_fringe_on_half_transparent_white() {
        // Top half opaque white, bottom half fully transparent.
        let mut img = solid(64, 64, [0, 0, 0, 0]);
        fill_rect(&mut img, 0, 0, 64, 32, [255, 255, 255, 255]);
        let resized = resize_square(&img, 16);
        assert_eq!((resized.width(), resized.height()), (16, 16));
        for p in resized.pixels() {
            if p.0[3] > 0 {
                assert!(
                    p.0[0] >= 250 && p.0[1] >= 250 && p.0[2] >= 250,
                    "dark fringe at {p:?}"
                );
            }
        }
    }

    #[test]
    fn resize_square_output_size_is_exact() {
        let img = solid(64, 64, [255, 0, 0, 255]);
        for size in [1, 16, 100] {
            let resized = resize_square(&img, size);
            assert_eq!((resized.width(), resized.height()), (size, size));
        }
    }

    #[test]
    fn flatten_on_color_replaces_transparency_with_the_background() {
        let mut img = RgbaImage::new(2, 1);
        img.put_pixel(0, 0, Rgba([0, 0, 0, 0]));
        img.put_pixel(1, 0, Rgba([10, 20, 30, 255]));
        let flat = flatten_on_color(&img, [200, 100, 50]);
        assert_eq!(flat.get_pixel(0, 0), &Rgba([200, 100, 50, 255]));
        assert_eq!(flat.get_pixel(1, 0), &Rgba([10, 20, 30, 255]));
        assert!(flat.pixels().all(|p| p.0[3] == 255));
    }

    #[test]
    fn make_maskable_is_fully_opaque_on_the_background() {
        let base = solid(64, 64, [255, 0, 0, 255]);
        let maskable = make_maskable(&base, 512, [0, 255, 0]);
        assert_eq!((maskable.width(), maskable.height()), (512, 512));
        assert!(maskable.pixels().all(|p| p.0[3] == 255));
        assert_eq!(maskable.get_pixel(0, 0), &Rgba([0, 255, 0, 255]));
        assert_eq!(maskable.get_pixel(255, 255), &Rgba([255, 0, 0, 255]));
    }
}
