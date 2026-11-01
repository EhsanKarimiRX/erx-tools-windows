//! Core inspection and option-validation logic for the Image Converter.
//!
//! Pure logic with no Tauri types so it can be unit tested directly.
//! Inspection detects the real format from file CONTENT (never the
//! extension), reads dimensions, and flags animated / multi-page files.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Hard limit on one input file, so a huge dropped file can never reach the
/// decoder (256 MB).
pub const MAX_INPUT_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// Hard limit on decoded pixel count — decompression-bomb guard (80 MP ≈
/// 320 MB of RGBA pixels per image). Enforced here on header dimensions and
/// again on the decoder when converting.
pub const MAX_PIXELS: u64 = 80_000_000;

// ---------------------------------------------------------------------------
// Input formats
// ---------------------------------------------------------------------------

/// Formats this tool can read, detected from file content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    Jpeg,
    Png,
    Webp,
    Bmp,
    Gif,
    Tiff,
}

impl InputFormat {
    /// Short display name used in the UI.
    pub fn label(self) -> &'static str {
        match self {
            InputFormat::Jpeg => "JPEG",
            InputFormat::Png => "PNG",
            InputFormat::Webp => "WebP",
            InputFormat::Bmp => "BMP",
            InputFormat::Gif => "GIF",
            InputFormat::Tiff => "TIFF",
        }
    }

    /// Maps a content-sniffed `image` crate format to ours, if supported.
    fn from_image_format(format: image::ImageFormat) -> Option<Self> {
        match format {
            image::ImageFormat::Jpeg => Some(InputFormat::Jpeg),
            image::ImageFormat::Png => Some(InputFormat::Png),
            image::ImageFormat::WebP => Some(InputFormat::Webp),
            image::ImageFormat::Bmp => Some(InputFormat::Bmp),
            image::ImageFormat::Gif => Some(InputFormat::Gif),
            image::ImageFormat::Tiff => Some(InputFormat::Tiff),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Inspection
// ---------------------------------------------------------------------------

/// What `inspect_path` found for one candidate file.
#[derive(Debug)]
pub enum Inspection {
    /// A supported, readable image file.
    Image(ImageInfo),
    /// A readable file that is not a supported image. `detected` names the
    /// real format when we recognize it (e.g. "AVIF"), for a clearer note.
    NotSupported { detected: Option<String> },
    /// Not usable at all (missing, folder, unreadable, damaged, over limit).
    Invalid { reason: String },
}

/// Facts about one supported image file.
#[derive(Debug)]
pub struct ImageInfo {
    pub format: InputFormat,
    pub size_bytes: u64,
    /// True for animated GIF/WebP and multi-page TIFF; conversion uses the
    /// first frame/page only and the UI marks the file.
    pub animated: bool,
}

/// Inspects one path: metadata checks, content sniffing, header dimensions
/// and the animated/multi-page flag. Never panics, never touches pixels.
pub fn inspect_path(path: &Path) -> Inspection {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => {
            return Inspection::Invalid {
                reason: "file not found or not accessible".into(),
            };
        }
    };
    if metadata.is_dir() {
        return Inspection::Invalid {
            reason: "folders are not supported".into(),
        };
    }
    if !metadata.is_file() {
        return Inspection::Invalid {
            reason: "not a regular file".into(),
        };
    }
    let size_bytes = metadata.len();
    if size_bytes == 0 {
        return Inspection::Invalid {
            reason: "file is empty".into(),
        };
    }
    if size_bytes > MAX_INPUT_FILE_BYTES {
        return Inspection::Invalid {
            reason: format!(
                "file is larger than the {} MB limit",
                MAX_INPUT_FILE_BYTES / (1024 * 1024)
            ),
        };
    }

    let file = match File::open(path) {
        Ok(file) => file,
        Err(_) => {
            return Inspection::Invalid {
                reason: "cannot open the file".into(),
            };
        }
    };
    let reader = image::ImageReader::new(BufReader::new(file));
    // Sniff the real format from content; the extension is never trusted.
    let reader = match reader.with_guessed_format() {
        Ok(reader) => reader,
        Err(_) => {
            return Inspection::Invalid {
                reason: "cannot read the file".into(),
            };
        }
    };
    let Some(image_format) = reader.format() else {
        return Inspection::NotSupported { detected: None };
    };
    let Some(format) = InputFormat::from_image_format(image_format) else {
        return Inspection::NotSupported {
            detected: Some(friendly_format_name(image_format).to_string()),
        };
    };

    // Parse the header for dimensions. A file with a valid magic but a
    // broken header is reported as damaged (the UI rejects it up front
    // instead of failing mid-batch later).
    let (width, height) = match reader.into_dimensions() {
        Ok(dimensions) => dimensions,
        Err(_) => {
            return Inspection::Invalid {
                reason: format!("{} file is damaged or incomplete", format.label()),
            };
        }
    };
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Inspection::Invalid {
            reason: format!(
                "image has more than the {} MP limit",
                MAX_PIXELS / 1_000_000
            ),
        };
    }

    let animated = match format {
        InputFormat::Gif => gif_is_animated(path).unwrap_or(false),
        InputFormat::Webp => webp_is_animated(path).unwrap_or(false),
        InputFormat::Tiff => tiff_has_multiple_pages(path).unwrap_or(false),
        _ => false,
    };

    Inspection::Image(ImageInfo {
        format,
        size_bytes,
        animated,
    })
}

/// Friendly name for a format `image` knows but this tool does not accept.
fn friendly_format_name(format: image::ImageFormat) -> &'static str {
    match format {
        image::ImageFormat::Avif => "AVIF",
        image::ImageFormat::Dds => "DDS",
        image::ImageFormat::Farbfeld => "Farbfeld",
        image::ImageFormat::Hdr => "HDR",
        image::ImageFormat::Ico => "ICO",
        image::ImageFormat::OpenExr => "OpenEXR",
        image::ImageFormat::Pnm => "PBM/PGM/PPM",
        image::ImageFormat::Qoi => "QOI",
        image::ImageFormat::Tga => "TGA",
        _ => "an unsupported image format",
    }
}

// ---------------------------------------------------------------------------
// Animated / multi-page detection (header-level byte parsing, no pixels)
// ---------------------------------------------------------------------------

/// Reads until `buf` is full. Returns false on end of file (a truncated
/// file simply stops the caller's parse early).
fn read_full<R: Read>(reader: &mut R, buf: &mut [u8]) -> std::io::Result<bool> {
    let mut filled = 0;
    while filled < buf.len() {
        let read = reader.read(&mut buf[filled..])?;
        if read == 0 {
            return Ok(false);
        }
        filled += read;
    }
    Ok(true)
}

/// Reads one byte; None on end of file.
fn read_byte<R: Read>(reader: &mut R) -> std::io::Result<Option<u8>> {
    let mut byte = [0u8; 1];
    if read_full(reader, &mut byte)? {
        Ok(Some(byte[0]))
    } else {
        Ok(None)
    }
}

/// Skips `n` bytes, tolerating end of file (truncated file).
fn skip_bytes<R: Read>(reader: &mut R, n: u64) -> std::io::Result<()> {
    let mut remaining = n;
    let mut buf = [0u8; 4096];
    while remaining > 0 {
        let take = remaining.min(buf.len() as u64) as usize;
        let read = reader.read(&mut buf[..take])?;
        if read == 0 {
            return Ok(());
        }
        remaining -= read as u64;
    }
    Ok(())
}

/// Skips a chain of GIF data sub-blocks (each: len byte + len bytes of data,
/// terminated by a zero length).
fn skip_sub_blocks<R: Read>(reader: &mut R) -> std::io::Result<()> {
    while let Some(len) = read_byte(reader)? {
        if len == 0 {
            return Ok(());
        }
        skip_bytes(reader, len as u64)?;
    }
    Ok(())
}

/// True if the GIF contains two or more frames (parsed from the block
/// structure only; no pixel data is decoded).
fn gif_is_animated(path: &Path) -> std::io::Result<bool> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut header = [0u8; 6];
    if !read_full(&mut reader, &mut header)? || (&header != b"GIF87a" && &header != b"GIF89a") {
        return Ok(false);
    }
    let mut lsd = [0u8; 7]; // logical screen descriptor
    if !read_full(&mut reader, &mut lsd)? {
        return Ok(false);
    }

    let mut descriptors = 0;
    loop {
        let Some(block) = read_byte(&mut reader)? else {
            return Ok(false); // truncated: treat as not animated
        };
        match block {
            0x21 => {
                // Extension: label byte, then data sub-blocks.
                read_byte(&mut reader)?; // label
                skip_sub_blocks(&mut reader)?;
            }
            0x2C => {
                // Image descriptor (one frame).
                descriptors += 1;
                if descriptors >= 2 {
                    return Ok(true);
                }
                let mut desc = [0u8; 9];
                if !read_full(&mut reader, &mut desc)? {
                    return Ok(false);
                }
                let packed = desc[8];
                if packed & 0x80 != 0 {
                    let table_len = 3u64 << ((packed & 0x07) + 1); // local color table
                    skip_bytes(&mut reader, table_len)?;
                }
                read_byte(&mut reader)?; // LZW minimum code size
                skip_sub_blocks(&mut reader)?;
            }
            0x3B => return Ok(false), // trailer: well-formed single-frame GIF
            _ => return Ok(false),    // malformed: give up, assume still
        }
    }
}

/// True if the WebP container carries an ANIM chunk (animated WebP).
fn webp_is_animated(path: &Path) -> std::io::Result<bool> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut header = [0u8; 12];
    if !read_full(&mut reader, &mut header)?
        || (&header[0..4] != b"RIFF" && &header[0..4] != b"RIFX")
        || &header[8..12] != b"WEBP"
    {
        return Ok(false);
    }
    let little_endian = &header[0..4] == b"RIFF";
    loop {
        let mut chunk = [0u8; 8];
        if !read_full(&mut reader, &mut chunk)? {
            return Ok(false);
        }
        let raw = if little_endian {
            u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]])
        } else {
            u32::from_be_bytes([chunk[4], chunk[5], chunk[6], chunk[7]])
        };
        if &chunk[0..4] == b"ANIM" {
            return Ok(true);
        }
        // Chunks are padded to even sizes.
        skip_bytes(&mut reader, u64::from(raw) + u64::from(raw & 1))?;
    }
}

/// True if the TIFF has more than one image directory (multi-page TIFF).
fn tiff_has_multiple_pages(path: &Path) -> std::io::Result<bool> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut header = [0u8; 8];
    if !read_full(&mut reader, &mut header)? {
        return Ok(false);
    }
    let little_endian = match &header[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Ok(false),
    };

    let read_u16 = |reader: &mut BufReader<File>| -> std::io::Result<u16> {
        let mut buf = [0u8; 2];
        if !read_full(reader, &mut buf)? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "eof",
            ));
        }
        Ok(if little_endian {
            u16::from_le_bytes(buf)
        } else {
            u16::from_be_bytes(buf)
        })
    };
    let read_u32 = |reader: &mut BufReader<File>| -> std::io::Result<u32> {
        let mut buf = [0u8; 4];
        if !read_full(reader, &mut buf)? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "eof",
            ));
        }
        Ok(if little_endian {
            u32::from_le_bytes(buf)
        } else {
            u32::from_be_bytes(buf)
        })
    };

    let ifd0_offset = match read_u32(&mut reader) {
        Ok(offset) => offset,
        Err(_) => return Ok(false),
    };
    if reader
        .seek(SeekFrom::Start(u64::from(ifd0_offset)))
        .is_err()
    {
        return Ok(false);
    }
    let entries = match read_u16(&mut reader) {
        Ok(entries) => entries,
        Err(_) => return Ok(false),
    };
    if reader
        .seek(SeekFrom::Current(i64::from(entries) * 12))
        .is_err()
    {
        return Ok(false);
    }
    Ok(read_u32(&mut reader).unwrap_or(0) != 0)
}

// ---------------------------------------------------------------------------
// Output options (validated once, reused by the conversion engine in 2B/2C)
// ---------------------------------------------------------------------------

/// Formats this tool can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Jpeg,
    Png,
    Webp,
    Bmp,
    Tiff,
}

impl OutputFormat {
    pub fn label(self) -> &'static str {
        match self {
            OutputFormat::Jpeg => "JPEG",
            OutputFormat::Png => "PNG",
            OutputFormat::Webp => "WebP",
            OutputFormat::Bmp => "BMP",
            OutputFormat::Tiff => "TIFF",
        }
    }

    /// File extension used for outputs.
    // First used by the conversion engine in checkpoint 2B.
    #[allow(dead_code)]
    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Jpeg => "jpg",
            OutputFormat::Png => "png",
            OutputFormat::Webp => "webp",
            OutputFormat::Bmp => "bmp",
            OutputFormat::Tiff => "tiff",
        }
    }

    /// True when the format cannot store alpha, so transparent images must
    /// be flattened onto the background color first. (32-bit BMP exists but
    /// is poorly supported elsewhere; v1 always flattens for BMP.)
    // First used by the conversion engine in checkpoint 2B.
    #[allow(dead_code)]
    pub fn needs_flattening(self) -> bool {
        matches!(self, OutputFormat::Jpeg | OutputFormat::Bmp)
    }
}

/// A parsed `#rrggbb` color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RgbColor(pub u8, pub u8, pub u8);

/// Parses `#rrggbb` (any case). Returns None for anything else.
pub fn parse_hex_rgb(text: &str) -> Option<RgbColor> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(RgbColor(
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    ))
}

/// Fully validated conversion options — the single source of truth shared by
/// the command layer and (from 2B) the conversion engine.
#[derive(Debug)]
pub struct ConvertOptions {
    pub format: OutputFormat,
    /// JPEG quality, 1-100 (WebP output is lossless in v1, so no quality).
    pub jpeg_quality: u8,
    /// Background used when flattening transparency.
    // First used by the conversion engine in checkpoint 2B.
    #[allow(dead_code)]
    pub background: RgbColor,
    /// `None` = same folder as each source file.
    pub output_folder: Option<PathBuf>,
}

impl ConvertOptions {
    /// Validates raw UI values; the error string is a friendly English
    /// message meant to be shown as-is.
    pub fn from_ui(
        format: &str,
        jpeg_quality: i32,
        background: &str,
        output_folder: Option<&str>,
    ) -> Result<Self, String> {
        let format = match format {
            "jpeg" => OutputFormat::Jpeg,
            "png" => OutputFormat::Png,
            "webp" => OutputFormat::Webp,
            "bmp" => OutputFormat::Bmp,
            "tiff" => OutputFormat::Tiff,
            other => return Err(format!("unsupported output format: {other}")),
        };
        if !(1..=100).contains(&jpeg_quality) {
            return Err("Quality must be between 1 and 100.".into());
        }
        let background = parse_hex_rgb(background)
            .ok_or_else(|| "Background color must look like #rrggbb.".to_string())?;
        let output_folder = match output_folder {
            Some(text) if !text.trim().is_empty() => {
                let path = PathBuf::from(text);
                if !path.is_absolute() {
                    return Err("The output folder must be an absolute path.".into());
                }
                if !path.is_dir() {
                    return Err("The chosen output folder does not exist.".into());
                }
                Some(path)
            }
            _ => None,
        };
        Ok(ConvertOptions {
            format,
            jpeg_quality: jpeg_quality as u8,
            background,
            output_folder,
        })
    }

    /// One-line English summary for messages.
    pub fn describe(&self) -> String {
        match self.format {
            OutputFormat::Jpeg => format!(
                "JPEG at quality {}{}",
                self.jpeg_quality,
                self.output_folder
                    .as_ref()
                    .map_or_else(String::new, |p| format!(" into {}", p.display()))
            ),
            other => format!(
                "{} (lossless){}",
                other.label(),
                self.output_folder
                    .as_ref()
                    .map_or_else(String::new, |p| format!(" into {}", p.display()))
            ),
        }
    }
}
