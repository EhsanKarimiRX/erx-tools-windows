//! The Windows WIC HEIC/HEIF decoder — the only HEIC file with unsafe/COM
//! code. Uses Microsoft's WIC codec (the installed HEIF Image Extensions +
//! HEVC Video Extensions); the app ships no decoder of its own.
//!
//! Pure helpers (`wide_path`, `check_dimensions`) are separated so they are
//! unit-testable without COM.

use std::cell::RefCell;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::Win32::Foundation::{GENERIC_READ, RPC_E_CHANGED_MODE, S_FALSE, S_OK};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICBitmapFrameDecode,
    IWICFormatConverter, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom,
    WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};

use super::error::{HeicFailure, Stage, classify_hresult};
use crate::tools::image_convert::core::MAX_PIXELS;

/// A decoded HEIC image: straight (non-premultiplied) 8-bit RGBA pixels.
#[derive(Debug, Clone)]
pub struct DecodedRgba {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// Frames the WIC decoder reports; we always use frame 0 (the primary
    /// image), like the animated-input convention elsewhere in the converter.
    /// Read by the live tests; production consumers may surface it later.
    #[allow(dead_code)]
    pub frame_count: u32,
}

/// Reads the container's primary image dimensions without touching pixels.
pub fn dimensions(path: &Path) -> Result<(u32, u32), HeicFailure> {
    com_ready()?;
    let factory = com_factory().map_err(|e| fail(Stage::CreateDecoder, e))?;
    let (_frame, _frame_count, width, height) = open_primary_frame(&factory, path)?;
    Ok((width, height))
}

/// Decodes the primary (first) image of a HEIC file to 8-bit RGBA.
pub fn decode_first_frame(path: &Path) -> Result<DecodedRgba, HeicFailure> {
    com_ready()?;
    let factory = com_factory().map_err(|e| fail(Stage::CreateDecoder, e))?;
    let (frame, frame_count, width, height) = open_primary_frame(&factory, path)?;

    // SAFETY: factory is a valid COM interface pointer obtained above.
    let converter: IWICFormatConverter =
        unsafe { factory.CreateFormatConverter() }.map_err(|e| fail(Stage::ConvertPixels, e))?;
    // SAFETY: frame is a valid COM interface; the pixel format GUID and the
    // enum values are the documented straight-RGBA conversion setup.
    unsafe {
        converter.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppRGBA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )
    }
    .map_err(|e| fail(Stage::ConvertPixels, e))?;

    let stride = width * 4; // fits u32: check_dimensions ruled out overflow
    let byte_count = u64::from(width) * u64::from(height) * 4;
    let mut buffer: Vec<u8> = Vec::new();
    buffer
        .try_reserve_exact(byte_count as usize)
        .map_err(|_| HeicFailure::OutOfMemory)?;
    buffer.resize(byte_count as usize, 0);
    // SAFETY: buffer is `height * stride` bytes as CopyPixels requires;
    // a null region pointer means "the whole image".
    unsafe { converter.CopyPixels(std::ptr::null(), stride, &mut buffer) }
        .map_err(|e| fail(Stage::ConvertPixels, e))?;

    Ok(DecodedRgba {
        width,
        height,
        rgba: buffer,
        frame_count,
    })
}

// ---------------------------------------------------------------------------
// COM plumbing
// ---------------------------------------------------------------------------

/// One thread's COM initialization outcome, created on first COM use and
/// uninitialized when the thread exits.
struct ComInit {
    /// True only when OUR `CoInitializeEx` call initialized COM (S_OK) and
    /// must be balanced with `CoUninitialize`. `S_FALSE` (COM already
    /// initialized on this thread by someone else) deliberately does not
    /// own the reference.
    owns_uninitialize: bool,
}

impl Drop for ComInit {
    fn drop(&mut self) {
        if self.owns_uninitialize {
            // SAFETY: balances our successful S_OK CoInitializeEx.
            unsafe { CoUninitialize() };
        }
    }
}

thread_local! {
    /// Per-thread COM state; `None` = "initialized with a different apartment
    /// model by someone else, COM still usable".
    static COM_INIT: RefCell<Option<ComInit>> = const { RefCell::new(None) };
}

/// Makes sure COM is usable on the calling thread. Batch workers are plain
/// threads, so each one initializes COM on first use.
fn com_ready() -> Result<(), HeicFailure> {
    COM_INIT.with(|slot| {
        if slot.borrow().is_none() {
            // SAFETY: documented CoInitializeEx call; reserved is None, MTA.
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            let guard = if hr == RPC_E_CHANGED_MODE {
                // The thread already runs another apartment model; COM still
                // works, but we must not uninitialize it.
                Some(ComInit {
                    owns_uninitialize: false,
                })
            } else if hr == S_OK || hr == S_FALSE {
                Some(ComInit {
                    owns_uninitialize: hr == S_OK,
                })
            } else {
                None
            };
            match guard {
                Some(guard) => *slot.borrow_mut() = Some(guard),
                None => return Err(HeicFailure::Other { code: hr.0 }),
            }
        }
        Ok(())
    })
}

/// Creates a fresh WIC factory (cheap and thread-safe; factories are not
/// shared across threads).
fn com_factory() -> Result<IWICImagingFactory, windows::core::Error> {
    // SAFETY: documented CoCreateInstance call for the WIC factory CLSID,
    // in-proc server, no outer unknown.
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
}

/// Maps one `windows` crate error through the shared classifier.
fn fail(stage: Stage, error: windows::core::Error) -> HeicFailure {
    classify_hresult(error.code().0, stage)
}

// ---------------------------------------------------------------------------
// Shared decode steps
// ---------------------------------------------------------------------------

/// Opens the file's primary image: factory → decoder → frame 0 → dimensions
/// and the size guards. No pixels are read.
fn open_primary_frame(
    factory: &IWICImagingFactory,
    path: &Path,
) -> Result<(IWICBitmapFrameDecode, u32, u32, u32), HeicFailure> {
    let wide = wide_path(path);
    // SAFETY: `wide` is a NUL-terminated UTF-16 buffer that outlives the call.
    let decoder = unsafe {
        factory.CreateDecoderFromFilename(
            windows::core::PCWSTR::from_raw(wide.as_ptr()),
            None,
            GENERIC_READ,
            WICDecodeMetadataCacheOnDemand,
        )
    }
    .map_err(|e| fail(Stage::CreateDecoder, e))?;

    // SAFETY: decoder is a valid COM interface pointer obtained above.
    let frame_count =
        unsafe { decoder.GetFrameCount() }.map_err(|e| fail(Stage::DecodeFrame, e))?;
    // Frame 0 only: the primary image, matching the converter's
    // "first frame/page only" convention for animated inputs.
    // SAFETY: decoder is a valid COM interface pointer; 0 is a valid index.
    let frame = unsafe { decoder.GetFrame(0) }.map_err(|e| fail(Stage::DecodeFrame, e))?;

    let mut width = 0u32;
    let mut height = 0u32;
    // SAFETY: plain out-parameters documented by GetSize.
    unsafe { frame.GetSize(&mut width, &mut height) }.map_err(|e| fail(Stage::DecodeFrame, e))?;
    check_dimensions(width, height)?;

    Ok((frame, frame_count, width, height))
}

/// NUL-terminated UTF-16 for `CreateDecoderFromFilename`.
fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// The converter's own hard guards, applied before any pixel memory is
/// allocated: no zero dimensions, no more than the shared 80 MP cap, and a
/// stride that fits `u32`.
fn check_dimensions(width: u32, height: u32) -> Result<(), HeicFailure> {
    if width == 0 || height == 0 {
        return Err(HeicFailure::TooLarge);
    }
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(HeicFailure::TooLarge);
    }
    if width.checked_mul(4).is_none() {
        return Err(HeicFailure::TooLarge);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::image_convert::heic::error::user_message;
    use std::fs::File;
    use std::io::Write;
    use std::os::windows::fs::OpenOptionsExt;
    use std::path::PathBuf;

    // ------------------------------------------------------------------
    // Pure helpers (no COM)
    // ------------------------------------------------------------------

    #[test]
    fn wide_paths_are_nul_terminated() {
        let wide = wide_path(Path::new("C:\\pics\\photo.heic"));
        assert_eq!(wide.last(), Some(&0));
        assert_eq!(wide.len(), "C:\\pics\\photo.heic".len() + 1);
    }

    #[test]
    fn dimension_guards_reject_bad_sizes() {
        assert_eq!(check_dimensions(0, 10), Err(HeicFailure::TooLarge));
        assert_eq!(check_dimensions(10, 0), Err(HeicFailure::TooLarge));
        // 12000 x 12000 = 144 MP > 80 MP cap.
        assert_eq!(check_dimensions(12_000, 12_000), Err(HeicFailure::TooLarge));
        // Exactly at the cap is allowed.
        let cap_side = (MAX_PIXELS as f64).sqrt() as u32;
        assert_eq!(check_dimensions(cap_side, cap_side), Ok(()));
        // Stride overflow guard: an extreme width must not wrap.
        assert_eq!(
            check_dimensions(u32::MAX / 2 + 2, 1),
            Err(HeicFailure::TooLarge)
        );
    }

    // ------------------------------------------------------------------
    // Live tests (#[ignore]; run with `cargo test -- --ignored`).
    // Scratch outputs go to the git-ignored target/heic-check/ folder.
    // ------------------------------------------------------------------

    /// target/heic-check/ — git-ignored scratch space (R8).
    fn scratch_dir() -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("heic-check");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The kamadak-exif crate test fixture `exif.heic`, found by scanning the
    /// cargo registry source folders (no hardcoded hash).
    fn find_kamadak_fixture() -> Option<PathBuf> {
        let registry_src = std::env::var_os("USERPROFILE")
            .map(|home| Path::new(&home).join(".cargo").join("registry").join("src"))?;
        for entry in std::fs::read_dir(&registry_src).ok()? {
            let entry = entry.ok()?;
            if !entry.file_type().ok()?.is_dir() {
                continue;
            }
            for version in std::fs::read_dir(entry.path()).ok()? {
                let version = version.ok()?;
                let name = version.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("kamadak-exif-") {
                    let candidate = version.path().join("tests").join("exif.heic");
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
        None
    }

    #[test]
    #[ignore]
    fn fixture_decodes_to_real_pixels() {
        let fixture = find_kamadak_fixture().expect("kamadak-exif exif.heic fixture not found");
        println!("fixture: {}", fixture.display());

        let (width, height) = dimensions(&fixture).expect("dimensions must succeed");
        println!("dimensions: {width}x{height}");
        assert!(width > 0 && height > 0);

        let decoded = decode_first_frame(&fixture).expect("decode must succeed");
        assert_eq!(decoded.width, width);
        assert_eq!(decoded.height, height);
        assert!(decoded.frame_count >= 1);
        assert_eq!(decoded.rgba.len(), (width as usize) * (height as usize) * 4);
        assert!(
            decoded.rgba.windows(2).any(|pair| pair[0] != pair[1]),
            "all pixels identical — decode produced garbage"
        );

        let png_path = scratch_dir().join("fixture.png");
        image::save_buffer(
            &png_path,
            &decoded.rgba,
            decoded.width,
            decoded.height,
            image::ExtendedColorType::Rgba8,
        )
        .expect("PNG export must succeed");
        println!("saved: {}", png_path.display());
    }

    /// (1) A truncated copy of the fixture must fail with a non-empty reason.
    #[test]
    #[ignore]
    fn truncated_fixture_fails() {
        let fixture = find_kamadak_fixture().expect("fixture not found");
        let bytes = std::fs::read(&fixture).unwrap();
        let cut = &bytes[..bytes.len() * 40 / 100];
        let path = scratch_dir().join("truncated.heic");
        File::create(&path).unwrap().write_all(cut).unwrap();

        let failure = decode_first_frame(&path).expect_err("truncated file must fail");
        println!("truncated  -> {failure:?}");
        println!("           -> {}", user_message(&failure));
        assert!(matches!(
            failure,
            HeicFailure::Corrupt { .. } | HeicFailure::CodecOrFile { .. }
        ));
    }

    /// (2) A valid-looking `ftypheic` header followed by garbage must fail.
    #[test]
    #[ignore]
    fn garbage_after_heic_header_fails() {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend(44u32.to_be_bytes());
        bytes.extend_from_slice(b"ftyp");
        bytes.extend_from_slice(b"heic");
        bytes.extend_from_slice(b"\0\0\0\0");
        bytes.extend([0u8; 24]);
        bytes.extend(std::iter::repeat_n(0xA5u8, 128));
        let path = scratch_dir().join("garbage.heic");
        File::create(&path).unwrap().write_all(&bytes).unwrap();

        let failure = decode_first_frame(&path).expect_err("garbage must fail");
        println!("garbage    -> {failure:?}");
        println!("           -> {}", user_message(&failure));
        assert!(matches!(
            failure,
            HeicFailure::Corrupt { .. } | HeicFailure::CodecOrFile { .. }
        ));
    }

    /// (3) A missing path must map to FileAccess.
    #[test]
    #[ignore]
    fn missing_file_maps_to_file_access() {
        let path = scratch_dir().join("does-not-exist.heic");
        let failure = decode_first_frame(&path).expect_err("missing file must fail");
        println!("missing    -> {failure:?}");
        println!("           -> {}", user_message(&failure));
        assert!(matches!(failure, HeicFailure::FileAccess { code: _ }));
    }

    /// (4) A file held open with exclusive sharing must map to FileAccess.
    #[test]
    #[ignore]
    fn locked_file_maps_to_file_access() {
        let fixture = find_kamadak_fixture().expect("fixture not found");
        let bytes = std::fs::read(&fixture).unwrap();
        let path = scratch_dir().join("locked.heic");
        File::create(&path).unwrap().write_all(&bytes).unwrap();

        // Hold the file open with NO sharing allowed while WIC tries to open it.
        let _lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();

        let failure = decode_first_frame(&path).expect_err("locked file must fail");
        println!("locked     -> {failure:?}");
        println!("           -> {}", user_message(&failure));
        assert!(matches!(failure, HeicFailure::FileAccess { code: _ }));

        drop(_lock);
        std::fs::remove_file(&path).unwrap();
    }
}
