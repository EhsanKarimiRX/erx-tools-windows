//! The conversion engine: decode → orient → flatten → encode.
//!
//! Works on any `BufRead + Seek` input so unit tests run fully in memory.
//! No Tauri types here; `pipeline.rs` wraps it for real files.

use std::io::{BufRead, Cursor, Seek};
use std::path::Path;

use image::codecs::bmp::BmpEncoder;
use image::codecs::gif::GifDecoder;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::tiff::TiffEncoder;
use image::codecs::webp::{WebPDecoder, WebPEncoder};
use image::imageops::overlay;
use image::metadata::Orientation;
use image::{
    AnimationDecoder, DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder, ImageFormat,
    ImageReader, Limits,
};

use super::core::{ConvertOptions, InputFormat, MAX_PIXELS, OutputFormat, friendly_format_name};
use super::heic::error::{HeicFailure, user_message};
use super::heic::wic::DecodedRgba;

/// Converts one image from `input` to the configured output format.
///
/// `animated` must come from the header-level inspection: animated GIF/WebP
/// inputs are converted from their first frame only.
pub fn convert_stream<R: BufRead + Seek>(
    input: R,
    animated: bool,
    options: &ConvertOptions,
) -> Result<Vec<u8>, String> {
    let reader = ImageReader::new(input)
        .with_guessed_format()
        .map_err(|_| "cannot read the file".to_string())?;
    let format = reader
        .format()
        .ok_or_else(|| "not a recognized image".to_string())?;
    if InputFormat::from_image_format(format).is_none() {
        return Err(format!(
            "{} is not supported in v1",
            friendly_format_name(format)
        ));
    }

    let image = match (format, animated) {
        // Animated inputs: take the first frame via the animation API (the
        // generic decode path cannot express "first frame of an animation").
        (ImageFormat::Gif, true) => {
            let decoder = GifDecoder::new(reader.into_inner())
                .map_err(|_| "could not decode the GIF file".to_string())?;
            first_frame(decoder.dimensions(), decoder.into_frames())?
        }
        (ImageFormat::WebP, true) => {
            let decoder = WebPDecoder::new(reader.into_inner())
                .map_err(|_| "could not decode the WebP file".to_string())?;
            first_frame(decoder.dimensions(), decoder.into_frames())?
        }
        _ => decode_still(reader)?,
    };

    encode(prepare(image, options), options)
}

// ---------------------------------------------------------------------------
// HEIC input (Windows WIC decoder behind a testable seam)
// ---------------------------------------------------------------------------

/// Pluggable HEIC decode seam: production uses Windows WIC, tests inject a
/// fake so the whole conversion path is testable without codecs.
pub trait HeicDecoder: Send + Sync {
    fn decode(&self, path: &Path) -> Result<DecodedRgba, HeicFailure>;
}

/// The production decoder: Windows' own WIC codec (Microsoft's HEIF/HEVC
/// extensions on the user's PC — the app ships no decoder).
pub struct WicDecoder;

impl HeicDecoder for WicDecoder {
    fn decode(&self, path: &Path) -> Result<DecodedRgba, HeicFailure> {
        super::heic::wic::decode_first_frame(path)
    }
}

static WIC_DECODER: WicDecoder = WicDecoder;

/// The decoder used when none is injected (production path).
pub fn default_heic_decoder() -> &'static dyn HeicDecoder {
    &WIC_DECODER
}

/// Builds the prepared image from decoded HEIC pixels: re-checks the size
/// guards like every other input, collapses all-opaque buffers to RGB (so
/// opaque iPhone photos do not grow needless alpha in PNG/WebP/TIFF outputs)
/// and keeps alpha otherwise.
///
/// Orientation: Windows' HEIC decoder applies the photo's irot/imir rotation
/// itself. Verified 2026-11-07 with three upright 3024×4032 portrait iPhone
/// photos on Windows 11 build 26300 x64 (HEIF Image Extension 1.2.62.0 +
/// HEVC Video Extension 2.5.48.0): WIC reports the rotated portrait
/// dimensions and upright content, so applying EXIF orientation here would
/// rotate the picture a second time.
fn heic_image(decoded: DecodedRgba) -> Result<DynamicImage, String> {
    check_dimensions(decoded.width, decoded.height)?;
    let buffer = image::RgbaImage::from_raw(decoded.width, decoded.height, decoded.rgba)
        .ok_or_else(|| "Windows returned HEIC pixels in an unexpected size".to_string())?;
    if buffer.pixels().all(|pixel| pixel.0[3] == 255) {
        Ok(DynamicImage::ImageRgb8(
            DynamicImage::ImageRgba8(buffer).to_rgb8(),
        ))
    } else {
        Ok(DynamicImage::ImageRgba8(buffer))
    }
}

/// Converts one HEIC file with an injected decoder (the test seam).
pub fn convert_heic_with(
    decoder: &dyn HeicDecoder,
    source: &Path,
    options: &ConvertOptions,
) -> Result<Vec<u8>, String> {
    let decoded = decoder
        .decode(source)
        .map_err(|failure| user_message(&failure))?;
    let image = heic_image(decoded)?;
    encode(prepare(image, options), options)
}

/// Converts one HEIC file through the real Windows WIC decoder.
pub fn convert_heic(source: &Path, options: &ConvertOptions) -> Result<Vec<u8>, String> {
    convert_heic_with(default_heic_decoder(), source, options)
}

/// Hard gate before any large allocation: the decompression-bomb guard.
fn check_dimensions(width: u32, height: u32) -> Result<(), String> {
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(format!(
            "image has more than the {} MP limit",
            MAX_PIXELS / 1_000_000
        ));
    }
    Ok(())
}

/// Takes the first frame of an animation as a fully-composited RGBA image.
fn first_frame(
    (width, height): (u32, u32),
    mut frames: image::Frames,
) -> Result<DynamicImage, String> {
    check_dimensions(width, height)?;
    let frame = frames
        .next()
        .ok_or_else(|| "the file contains no image frames".to_string())?
        .map_err(|_| "could not decode the first frame of the animated file".to_string())?;
    Ok(DynamicImage::ImageRgba8(frame.buffer().clone()))
}

/// Generic decode with the size guard, allocation cap and EXIF orientation
/// applied (phone photos arrive upright).
fn decode_still<R: BufRead + Seek>(reader: ImageReader<R>) -> Result<DynamicImage, String> {
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| "could not decode the image file".to_string())?;
    let (width, height) = decoder.dimensions();
    check_dimensions(width, height)?;
    // Belt-and-braces allocation cap on top of the explicit pixel-count
    // check. Best effort: a decoder that refuses limits keeps its built-in
    // default cap (512 MiB).
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_PIXELS * 4 + 128 * 1024 * 1024);
    let _ = decoder.set_limits(limits);
    // No EXIF or unreadable EXIF simply means "no rotation needed".
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder)
        .map_err(|_| "could not decode the image file".to_string())?;
    image.apply_orientation(orientation);
    Ok(image)
}

/// Flattens transparency where the output format cannot store it, otherwise
/// matches the buffer's channel layout to what the encoder expects.
fn prepare(image: DynamicImage, options: &ConvertOptions) -> DynamicImage {
    if options.format.needs_flattening() {
        let background = image::Rgba([
            options.background.0,
            options.background.1,
            options.background.2,
            255,
        ]);
        let mut canvas = image::RgbaImage::from_pixel(image.width(), image.height(), background);
        // Alpha-blend the image onto the opaque background canvas.
        overlay(&mut canvas, &image.to_rgba8(), 0, 0);
        // Alpha is 255 everywhere now, so dropping the channel is lossless.
        return DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(canvas).to_rgb8());
    }
    if has_alpha_channel(image.color()) {
        DynamicImage::ImageRgba8(image.to_rgba8())
    } else {
        DynamicImage::ImageRgb8(image.to_rgb8())
    }
}

/// Formats whose decoded representation can carry an alpha channel.
fn has_alpha_channel(color: image::ColorType) -> bool {
    matches!(
        color,
        image::ColorType::La8
            | image::ColorType::Rgba8
            | image::ColorType::La16
            | image::ColorType::Rgba16
    )
}

/// Encodes the prepared image. Bytes are built in memory first, so a failed
/// conversion never leaves a partial file behind.
fn encode(image: DynamicImage, options: &ConvertOptions) -> Result<Vec<u8>, String> {
    let (width, height) = (image.width(), image.height());
    let (pixels, color): (&[u8], ExtendedColorType) = match &image {
        DynamicImage::ImageRgb8(buffer) => (buffer.as_raw(), ExtendedColorType::Rgb8),
        DynamicImage::ImageRgba8(buffer) => (buffer.as_raw(), ExtendedColorType::Rgba8),
        _ => unreachable!("prepare only returns 8-bit RGB/RGBA images"),
    };
    let mut out = Cursor::new(Vec::new());
    let result = match options.format {
        OutputFormat::Jpeg => JpegEncoder::new_with_quality(&mut out, options.jpeg_quality)
            .write_image(pixels, width, height, color),
        OutputFormat::Png => PngEncoder::new(&mut out).write_image(pixels, width, height, color),
        OutputFormat::Webp => {
            WebPEncoder::new_lossless(&mut out).write_image(pixels, width, height, color)
        }
        OutputFormat::Bmp => BmpEncoder::new(&mut out).write_image(pixels, width, height, color),
        OutputFormat::Tiff => TiffEncoder::new(&mut out).write_image(pixels, width, height, color),
    };
    result.map_err(|_| format!("could not encode the {} output", options.format.label()))?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::image_convert::core::{InputFormat, RgbColor};
    use image::codecs::gif::GifEncoder;
    use image::{Frame, GenericImage, GenericImageView, ImageBuffer, Rgba};
    use windows::Win32::Foundation::{
        E_FAIL, WINCODEC_ERR_BADHEADER, WINCODEC_ERR_COMPONENTNOTFOUND,
    };

    const ALL_INPUTS: [InputFormat; 6] = [
        InputFormat::Jpeg,
        InputFormat::Png,
        InputFormat::Webp,
        InputFormat::Bmp,
        InputFormat::Gif,
        InputFormat::Tiff,
    ];
    const ALL_OUTPUTS: [OutputFormat; 5] = [
        OutputFormat::Jpeg,
        OutputFormat::Png,
        OutputFormat::Webp,
        OutputFormat::Bmp,
        OutputFormat::Tiff,
    ];

    fn png_options() -> ConvertOptions {
        ConvertOptions {
            format: OutputFormat::Png,
            jpeg_quality: 85,
            background: RgbColor(255, 255, 255),
            output_folder: None,
        }
    }

    /// Deterministic opaque test image.
    fn gradient(width: u32, height: u32) -> DynamicImage {
        DynamicImage::ImageRgba8(ImageBuffer::from_fn(width, height, |x, y| {
            Rgba([
                ((x * 17) % 256) as u8,
                ((y * 23) % 256) as u8,
                ((x * y) % 256) as u8,
                255,
            ])
        }))
    }

    /// Encodes an image in one of the supported INPUT formats (test helper).
    fn encode_input(image: &DynamicImage, format: InputFormat) -> Vec<u8> {
        let (width, height) = (image.width(), image.height());
        let mut out = Cursor::new(Vec::new());
        let result = match format {
            InputFormat::Jpeg => JpegEncoder::new(&mut out).write_image(
                &image.to_rgb8(),
                width,
                height,
                ExtendedColorType::Rgb8,
            ),
            InputFormat::Png => PngEncoder::new(&mut out).write_image(
                &image.to_rgba8(),
                width,
                height,
                ExtendedColorType::Rgba8,
            ),
            InputFormat::Webp => WebPEncoder::new_lossless(&mut out).write_image(
                &image.to_rgba8(),
                width,
                height,
                ExtendedColorType::Rgba8,
            ),
            InputFormat::Bmp => BmpEncoder::new(&mut out).write_image(
                &image.to_rgb8(),
                width,
                height,
                ExtendedColorType::Rgb8,
            ),
            InputFormat::Gif => {
                GifEncoder::new(&mut out).encode_frame(Frame::new(image.to_rgba8()))
            }
            InputFormat::Tiff => TiffEncoder::new(&mut out).write_image(
                &image.to_rgba8(),
                width,
                height,
                ExtendedColorType::Rgba8,
            ),
            // HEIC has no encoder here: its pixels arrive through the
            // decoder seam (see the fake-decoder tests below).
            InputFormat::Heic => panic!("HEIC inputs come from the decoder seam, not an encoder"),
        };
        result.expect("test image must encode");
        out.into_inner()
    }

    /// Every supported input × output combination must produce a decodable
    /// file with unchanged dimensions, and lossless→lossless chains must be
    /// pixel-exact.
    #[test]
    fn all_input_output_combinations_roundtrip() {
        let source = gradient(16, 12);
        for input in ALL_INPUTS {
            let source_bytes = encode_input(&source, input);
            let reference = image::load_from_memory(&source_bytes).unwrap();
            for output in ALL_OUTPUTS {
                let options = ConvertOptions {
                    format: output,
                    jpeg_quality: 85,
                    background: RgbColor(255, 255, 255),
                    output_folder: None,
                };
                let converted = convert_stream(Cursor::new(source_bytes.clone()), false, &options)
                    .unwrap_or_else(|error| panic!("{input:?} -> {output:?}: {error}"));
                let result = image::load_from_memory(&converted)
                    .unwrap_or_else(|error| panic!("{input:?} -> {output:?}: {error}"));
                assert_eq!(
                    (result.width(), result.height()),
                    (16, 12),
                    "{input:?} -> {output:?}"
                );
                if input != InputFormat::Jpeg && output != OutputFormat::Jpeg {
                    assert_eq!(
                        result.to_rgb8().as_raw(),
                        reference.to_rgb8().as_raw(),
                        "{input:?} -> {output:?} must be pixel-exact"
                    );
                }
            }
        }
    }

    /// Transparent pixels land exactly on the chosen background (BMP is
    /// lossless, so the assertion can be exact).
    #[test]
    fn transparency_flattens_onto_background() {
        let mut image = gradient(4, 2);
        image.put_pixel(0, 0, Rgba([0, 0, 0, 0]));
        image.put_pixel(1, 0, Rgba([255, 0, 0, 255]));
        let source = encode_input(&image, InputFormat::Png);

        for output in [OutputFormat::Bmp, OutputFormat::Jpeg] {
            let options = ConvertOptions {
                format: output,
                jpeg_quality: 95,
                background: RgbColor(10, 20, 30),
                output_folder: None,
            };
            let result = image::load_from_memory(
                &convert_stream(Cursor::new(source.clone()), false, &options).unwrap(),
            )
            .unwrap();
            let pixel = |x: u32| result.get_pixel(x, 0).0;
            let expected: [u8; 3] = [10, 20, 30];
            let close = |actual: [u8; 4], wanted: [u8; 3]| {
                actual[..3]
                    .iter()
                    .zip(wanted)
                    .all(|(a, b)| (i16::from(*a) - i16::from(b)).abs() <= 8)
            };
            assert!(
                close(pixel(0), expected),
                "{output:?}: background pixel wrong: {:?}",
                pixel(0)
            );
            assert!(
                close(pixel(1), [255, 0, 0]),
                "{output:?}: opaque pixel must survive: {:?}",
                pixel(1)
            );
        }
    }

    /// Alpha-capable outputs keep transparency intact.
    #[test]
    fn alpha_is_preserved_for_capable_outputs() {
        let mut image = gradient(4, 2);
        image.put_pixel(0, 0, Rgba([1, 2, 3, 0]));
        let source = encode_input(&image, InputFormat::Png);
        let result = image::load_from_memory(
            &convert_stream(Cursor::new(source), false, &png_options()).unwrap(),
        )
        .unwrap();
        assert_eq!(result.color(), image::ColorType::Rgba8);
        assert_eq!(result.get_pixel(0, 0).0[3], 0);
    }

    /// A hand-crafted grayscale TIFF carrying EXIF orientation 6 (rotate
    /// 90° CW) must come out upright: 4×2 in, 2×4 out.
    #[test]
    fn exif_orientation_is_applied() {
        let mut bytes = Vec::new();
        bytes.extend(b"II");
        bytes.extend(42u16.to_le_bytes());
        bytes.extend(8u32.to_le_bytes()); // IFD offset (right after the header)
        bytes.extend(10u16.to_le_bytes()); // entry count
        let tag = |bytes: &mut Vec<u8>, id: u16, kind: u16, value: u32| {
            bytes.extend(id.to_le_bytes());
            bytes.extend(kind.to_le_bytes());
            bytes.extend(1u32.to_le_bytes());
            bytes.extend(value.to_le_bytes()); // SHORT sits in the low bytes (LE)
        };
        tag(&mut bytes, 0x0100, 3, 4); // width = 4
        tag(&mut bytes, 0x0101, 3, 2); // height = 2
        tag(&mut bytes, 0x0102, 3, 8); // bits per sample
        tag(&mut bytes, 0x0103, 3, 1); // compression: none
        tag(&mut bytes, 0x0106, 3, 1); // photometric: black is zero
        tag(&mut bytes, 0x0111, 4, 134); // strip offset
        tag(&mut bytes, 0x0112, 3, 6); // orientation: rotate 90 CW
        tag(&mut bytes, 0x0115, 3, 1); // samples per pixel
        tag(&mut bytes, 0x0116, 3, 2); // rows per strip (required by the reader)
        tag(&mut bytes, 0x0117, 4, 8); // strip byte count
        bytes.extend(0u32.to_le_bytes()); // no next IFD
        assert_eq!(bytes.len(), 134);
        // Stored rows: [0, 50, 100, 150] / [200, 250, 30, 80]. Rotated 90° CW
        // the rows become [200, 0], [250, 50], [30, 100], [80, 150].
        bytes.extend([0u8, 50, 100, 150, 200, 250, 30, 80]);
        // The TIFF reader reads strips with some slack; trailing padding is
        // ignored.
        bytes.extend([0u8; 32]);

        let result = image::load_from_memory(
            &convert_stream(Cursor::new(bytes.clone()), false, &png_options()).unwrap_or_else(
                |error| {
                    panic!(
                        "conversion failed: {error} (direct decode: {:?})",
                        image::load_from_memory(&bytes).map(|i| i.dimensions())
                    )
                },
            ),
        )
        .unwrap();
        assert_eq!(result.dimensions(), (2, 4));
        assert_eq!(result.get_pixel(0, 0).0[0], 200);
        assert_eq!(result.get_pixel(1, 0).0[0], 0);
        assert_eq!(result.get_pixel(0, 3).0[0], 80);
        assert_eq!(result.get_pixel(1, 3).0[0], 150);
    }

    /// Animated GIFs convert their first frame only.
    #[test]
    fn animated_gif_uses_first_frame() {
        let mut first = gradient(16, 12).to_rgba8();
        for pixel in first.pixels_mut() {
            pixel.0 = [255, 0, 0, 255];
        }
        let mut second = first.clone();
        for pixel in second.pixels_mut() {
            pixel.0 = [0, 0, 255, 255];
        }
        let source = {
            let mut out = Cursor::new(Vec::new());
            {
                let mut encoder = GifEncoder::new(&mut out);
                encoder.encode_frame(Frame::new(first)).unwrap();
                encoder.encode_frame(Frame::new(second)).unwrap();
            }
            out.into_inner()
        };

        let result = image::load_from_memory(
            &convert_stream(Cursor::new(source), true, &png_options()).unwrap(),
        )
        .unwrap();
        assert_eq!(result.get_pixel(0, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn non_image_input_is_rejected() {
        let error = convert_stream(
            Cursor::new(b"this is not an image at all".to_vec()),
            false,
            &png_options(),
        )
        .unwrap_err();
        assert!(error.contains("not a recognized image"));
    }

    #[test]
    fn damaged_image_is_rejected() {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        bytes.extend([0u8; 64]); // invalid after the magic
        let error = convert_stream(Cursor::new(bytes), false, &png_options()).unwrap_err();
        assert!(error.contains("could not decode"));
    }

    /// A BMP header claiming 12000×12000 (144 MP) must hit the pixel guard
    /// before anything is decoded.
    #[test]
    fn oversized_dimensions_are_rejected() {
        let mut bytes = Vec::new();
        bytes.extend(b"BM");
        bytes.extend(0u32.to_le_bytes()); // file size
        bytes.extend(0u32.to_le_bytes()); // reserved
        bytes.extend(54u32.to_le_bytes()); // pixel data offset
        bytes.extend(40u32.to_le_bytes()); // DIB header size
        bytes.extend(12000u32.to_le_bytes());
        bytes.extend(12000u32.to_le_bytes());
        bytes.extend(1u16.to_le_bytes()); // planes
        bytes.extend(24u16.to_le_bytes()); // bits per pixel
        bytes.extend(0u32.to_le_bytes()); // compression
        bytes.extend([0u8; 20]); // rest of the DIB header
        bytes.extend([0u8; 16]); // some "pixel" bytes

        let error = convert_stream(Cursor::new(bytes), false, &png_options()).unwrap_err();
        assert!(error.contains("80 MP limit"));
    }

    #[test]
    fn unsupported_formats_name_the_format() {
        // A farbfeld file: real image format, not supported by this tool.
        let mut bytes = Vec::new();
        bytes.extend(b"farbfeld");
        bytes.extend(1u32.to_be_bytes());
        bytes.extend(1u32.to_be_bytes());
        bytes.extend([0u8; 8]);
        let error = convert_stream(Cursor::new(bytes), false, &png_options()).unwrap_err();
        assert!(error.contains("Farbfeld"));
        assert!(error.contains("not supported"));
    }

    // ------------------------------------------------------------------
    // HEIC input through the decoder seam (6b)
    // ------------------------------------------------------------------

    /// Fake decoder returning a preset result; the path is ignored.
    struct FakeDecoder(Result<DecodedRgba, HeicFailure>);

    impl HeicDecoder for FakeDecoder {
        fn decode(&self, _path: &Path) -> Result<DecodedRgba, HeicFailure> {
            self.0.clone()
        }
    }

    /// A deterministic width×height HEIC frame: opaque unless `alpha` is set.
    fn fake_heic(width: u32, height: u32, alpha: Option<u8>) -> DecodedRgba {
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&[
                    ((x * 17) % 256) as u8,
                    ((y * 23) % 256) as u8,
                    ((x * y) % 256) as u8,
                    alpha.unwrap_or(255),
                ]);
            }
        }
        DecodedRgba {
            width,
            height,
            rgba,
            frame_count: 1,
        }
    }

    #[test]
    fn heic_opaque_converts_to_every_output() {
        let source = Path::new("C:\\fake\\photo.heic");
        for output in ALL_OUTPUTS {
            let options = ConvertOptions {
                format: output,
                jpeg_quality: 85,
                background: RgbColor(255, 255, 255),
                output_folder: None,
            };
            let converted =
                convert_heic_with(&FakeDecoder(Ok(fake_heic(4, 2, None))), source, &options)
                    .unwrap_or_else(|error| panic!("{output:?}: {error}"));
            let result = image::load_from_memory(&converted).unwrap();
            assert_eq!(result.dimensions(), (4, 2), "{output:?}");
            // All-opaque HEIC collapses to RGB, so no output carries alpha.
            assert_eq!(result.color(), image::ColorType::Rgb8, "{output:?}");
        }
    }

    #[test]
    fn heic_alpha_is_kept_or_flattened_per_output() {
        let source = Path::new("C:\\fake\\photo.heic");
        // First pixel fully transparent, the rest opaque.
        let mut decoded = fake_heic(4, 2, None);
        decoded.rgba[0..4].copy_from_slice(&[0, 0, 0, 0]);

        for output in [OutputFormat::Png, OutputFormat::Webp, OutputFormat::Tiff] {
            let options = ConvertOptions {
                format: output,
                jpeg_quality: 85,
                background: RgbColor(255, 255, 255),
                output_folder: None,
            };
            let converted = convert_heic_with(&FakeDecoder(Ok(decoded.clone())), source, &options)
                .unwrap_or_else(|error| panic!("{output:?}: {error}"));
            let result = image::load_from_memory(&converted).unwrap();
            assert_eq!(result.color(), image::ColorType::Rgba8, "{output:?}");
            assert_eq!(result.get_pixel(0, 0).0[3], 0, "{output:?}");
        }

        for output in [OutputFormat::Jpeg, OutputFormat::Bmp] {
            let options = ConvertOptions {
                format: output,
                jpeg_quality: 95,
                background: RgbColor(10, 20, 30),
                output_folder: None,
            };
            let converted = convert_heic_with(&FakeDecoder(Ok(decoded.clone())), source, &options)
                .unwrap_or_else(|error| panic!("{output:?}: {error}"));
            let result = image::load_from_memory(&converted).unwrap();
            let pixel = result.get_pixel(0, 0).0;
            let close = pixel[..3]
                .iter()
                .zip([10u8, 20, 30])
                .all(|(a, b)| i16::from(*a) - i16::from(b) <= 8);
            assert!(close, "{output:?}: background pixel wrong: {pixel:?}");
        }
    }

    #[test]
    fn heic_all_opaque_collapses_to_rgb_in_memory() {
        assert!(matches!(
            heic_image(fake_heic(4, 2, None)).unwrap(),
            DynamicImage::ImageRgb8(_)
        ));
        assert!(matches!(
            heic_image(fake_heic(4, 2, Some(128))).unwrap(),
            DynamicImage::ImageRgba8(_)
        ));
    }

    #[test]
    fn heic_rechecks_dimension_guards() {
        let error = convert_heic_with(
            &FakeDecoder(Ok(fake_heic(12_000, 12_000, None))),
            Path::new("C:\\fake\\photo.heic"),
            &png_options(),
        )
        .unwrap_err();
        assert_eq!(error, "image has more than the 80 MP limit");
    }

    #[test]
    fn heic_decoder_failures_become_the_exact_messages() {
        let source = Path::new("C:\\fake\\photo.heic");
        let failures = [
            HeicFailure::CodecOrFile {
                code: WINCODEC_ERR_COMPONENTNOTFOUND.0,
            },
            HeicFailure::Corrupt {
                code: WINCODEC_ERR_BADHEADER.0,
            },
            HeicFailure::FileAccess {
                code: 0x80070002u32 as i32,
            },
            HeicFailure::OutOfMemory,
            HeicFailure::TooLarge,
            HeicFailure::Other { code: E_FAIL.0 },
        ];
        for failure in &failures {
            let error =
                convert_heic_with(&FakeDecoder(Err(*failure)), source, &png_options()).unwrap_err();
            assert_eq!(error, user_message(failure), "{failure:?}");
            assert!(!error.trim().is_empty());
        }
        // Distinctive wording per class, as specified in 6a-5.
        assert!(user_message(&failures[0]).contains("HEIF Image Extensions"));
        assert!(user_message(&failures[0]).contains("0x88982F50"));
        assert!(user_message(&failures[1]).contains("damaged or incomplete"));
        assert!(user_message(&failures[2]).contains("in use by another program"));
        assert!(user_message(&failures[3]).contains("Not enough memory"));
        assert!(user_message(&failures[4]).contains("80 MP limit"));
        assert!(user_message(&failures[5]).contains("could not decode"));
    }
}
