//! Pure HEIC failure classification and user-facing messages (no Windows
//! calls, so the mapping and wording are unit-testable).
//!
//! We only call WIC for files our sniffer already proved to be HEIC, so a
//! failure almost always means "a required Windows codec is missing" or
//! "the file is damaged". Both are expected situations and must be reported
//! honestly, in plain English.

use windows::Win32::Foundation::{
    E_OUTOFMEMORY, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_LOCK_VIOLATION,
    ERROR_PATH_NOT_FOUND, ERROR_SHARING_VIOLATION, WIN32_ERROR, WINCODEC_ERR_BADHEADER,
    WINCODEC_ERR_BADIMAGE, WINCODEC_ERR_BADSTREAMDATA, WINCODEC_ERR_STREAMREAD,
    WINCODEC_ERR_UNEXPECTEDSIZE,
};

use crate::tools::image_convert::core::MAX_PIXELS;

/// Where in the WIC pipeline a call failed. Currently only carried for
/// context; classification is stage-independent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Creating the WIC decoder (COM factory, decoder, file open).
    CreateDecoder,
    /// Reading the container or a frame (frame count, frame, size).
    DecodeFrame,
    /// Pixel conversion or copy.
    ConvertPixels,
}

/// A classified HEIC failure. `code` is the raw HRESULT when there is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeicFailure {
    /// Most likely missing Windows codecs; could also be an unknown format.
    CodecOrFile { code: i32 },
    /// The file exists but its contents are damaged or incomplete.
    Corrupt { code: i32 },
    /// The file could not be opened or read at all.
    FileAccess { code: i32 },
    /// Allocation failed.
    OutOfMemory,
    /// Produced by our own size guards, never by an HRESULT.
    TooLarge,
    /// Anything else.
    Other { code: i32 },
}

/// Wraps a Win32 error number into the HRESULT shape WIC reports
/// (FACILITY_WIN32), mirroring the documented `HRESULT_FROM_WIN32` macro.
/// The `windows` 0.62 crate exports no such helper (verified in step 6a-2),
/// so the wrap is implemented here; the error numbers themselves are the
/// crate's `WIN32_ERROR` constants.
fn hresult_from_win32(error: WIN32_ERROR) -> i32 {
    let raw = error.0;
    if raw & 0x8000_0000 != 0 {
        raw as i32
    } else {
        (raw | 0x8007_0000) as i32
    }
}

/// True when `code` is the HRESULT-wrapped form of one of `errors`.
fn is_win32_error(code: i32, errors: &[WIN32_ERROR]) -> bool {
    errors
        .iter()
        .any(|error| hresult_from_win32(*error) == code)
}

/// Classifies one HRESULT. Pure: no Windows calls, stage-independent.
pub fn classify_hresult(code: i32, _stage: Stage) -> HeicFailure {
    if code == E_OUTOFMEMORY.0 {
        return HeicFailure::OutOfMemory;
    }
    if is_win32_error(
        code,
        &[
            ERROR_FILE_NOT_FOUND,
            ERROR_PATH_NOT_FOUND,
            ERROR_ACCESS_DENIED,
            ERROR_SHARING_VIOLATION,
            ERROR_LOCK_VIOLATION,
        ],
    ) {
        return HeicFailure::FileAccess { code };
    }
    if code == WINCODEC_ERR_BADIMAGE.0
        || code == WINCODEC_ERR_BADHEADER.0
        || code == WINCODEC_ERR_BADSTREAMDATA.0
        || code == WINCODEC_ERR_STREAMREAD.0
        || code == WINCODEC_ERR_UNEXPECTEDSIZE.0
    {
        return HeicFailure::Corrupt { code };
    }
    // Includes WINCODEC_ERR_COMPONENTNOTFOUND (missing codec) and
    // WINCODEC_ERR_UNKNOWNIMAGEFORMAT.
    HeicFailure::CodecOrFile { code }
}

/// Formats an HRESULT the way users see it in Windows, e.g. `0x80070002`.
/// The cast matters: Rust would otherwise print a minus sign for negative
/// `i32` values instead of the two's-complement HRESULT.
fn format_code(code: i32) -> String {
    format!("0x{:08X}", code as u32)
}

/// Short user-facing English message for a failure, shown as-is.
pub fn user_message(failure: &HeicFailure) -> String {
    match failure {
        HeicFailure::CodecOrFile { code } => format!(
            "Windows could not open this HEIC photo. iPhone photos need Microsoft's \
             HEIF Image Extensions and HEVC Video Extensions (get them from the \
             Microsoft Store). If they are already installed, the file may be \
             damaged. (Windows error {})",
            format_code(*code)
        ),
        HeicFailure::Corrupt { code } => format!(
            "This HEIC file looks damaged or incomplete. (Windows error {})",
            format_code(*code)
        ),
        HeicFailure::FileAccess { code } => format!(
            "Couldn't read this file. It may be in use by another program or you \
             may not have permission. (Windows error {})",
            format_code(*code)
        ),
        HeicFailure::OutOfMemory => "Not enough memory to open this photo.".to_string(),
        // Same wording the converter uses for oversized images elsewhere.
        HeicFailure::TooLarge => {
            format!(
                "image has more than the {} MP limit",
                MAX_PIXELS / 1_000_000
            )
        }
        HeicFailure::Other { code } => format!(
            "Windows could not decode this HEIC photo. (Windows error {})",
            format_code(*code)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{
        E_FAIL, WINCODEC_ERR_COMPONENTNOTFOUND, WINCODEC_ERR_UNKNOWNIMAGEFORMAT,
    };

    #[test]
    fn out_of_memory_is_its_own_class() {
        assert_eq!(
            classify_hresult(E_OUTOFMEMORY.0, Stage::DecodeFrame),
            HeicFailure::OutOfMemory
        );
    }

    #[test]
    fn wrapped_win32_file_errors_map_to_file_access() {
        for error in [
            ERROR_FILE_NOT_FOUND,
            ERROR_PATH_NOT_FOUND,
            ERROR_ACCESS_DENIED,
            ERROR_SHARING_VIOLATION,
            ERROR_LOCK_VIOLATION,
        ] {
            let code = hresult_from_win32(error);
            assert_eq!(
                classify_hresult(code, Stage::CreateDecoder),
                HeicFailure::FileAccess { code },
                "0x{code:08X}"
            );
        }
    }

    #[test]
    fn wic_corrupt_codes_map_to_corrupt() {
        for code in [
            WINCODEC_ERR_BADIMAGE.0,
            WINCODEC_ERR_BADHEADER.0,
            WINCODEC_ERR_BADSTREAMDATA.0,
            WINCODEC_ERR_STREAMREAD.0,
            WINCODEC_ERR_UNEXPECTEDSIZE.0,
        ] {
            assert_eq!(
                classify_hresult(code, Stage::ConvertPixels),
                HeicFailure::Corrupt { code },
                "0x{code:08X}"
            );
        }
    }

    #[test]
    fn missing_codec_lands_in_codec_or_file() {
        assert_eq!(
            classify_hresult(WINCODEC_ERR_COMPONENTNOTFOUND.0, Stage::CreateDecoder),
            HeicFailure::CodecOrFile {
                code: WINCODEC_ERR_COMPONENTNOTFOUND.0
            }
        );
        assert_eq!(
            classify_hresult(WINCODEC_ERR_UNKNOWNIMAGEFORMAT.0, Stage::DecodeFrame),
            HeicFailure::CodecOrFile {
                code: WINCODEC_ERR_UNKNOWNIMAGEFORMAT.0
            }
        );
    }

    #[test]
    fn unknown_codes_default_to_codec_or_file_at_every_stage() {
        for stage in [
            Stage::CreateDecoder,
            Stage::DecodeFrame,
            Stage::ConvertPixels,
        ] {
            assert_eq!(
                classify_hresult(E_FAIL.0, stage),
                HeicFailure::CodecOrFile { code: E_FAIL.0 }
            );
        }
    }

    #[test]
    fn negative_codes_format_as_eight_uppercase_hex_digits() {
        // E_FAIL = 0x80004005, stored as a negative i32.
        assert_eq!(format_code(E_FAIL.0), "0x80004005");
        assert_eq!(
            format_code(hresult_from_win32(ERROR_FILE_NOT_FOUND)),
            "0x80070002"
        );
    }

    #[test]
    fn every_variant_produces_a_non_empty_message() {
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
            let message = user_message(failure);
            assert!(!message.trim().is_empty(), "{failure:?}");
            assert!(
                message.is_ascii(),
                "UI text must be English only: {message}"
            );
        }
    }

    #[test]
    fn corrupt_message_carries_the_hex_code() {
        let message = user_message(&HeicFailure::Corrupt {
            code: WINCODEC_ERR_BADHEADER.0,
        });
        assert!(message.contains("0x88982F61"), "{message}");
        assert!(message.contains("damaged"), "{message}");
    }

    #[test]
    fn codec_missing_message_names_both_extensions_and_the_store() {
        let message = user_message(&HeicFailure::CodecOrFile {
            code: WINCODEC_ERR_COMPONENTNOTFOUND.0,
        });
        assert!(message.contains("HEIF Image Extensions"), "{message}");
        assert!(message.contains("HEVC Video Extensions"), "{message}");
        assert!(message.contains("Microsoft Store"), "{message}");
        assert!(message.contains("0x88982F50"), "{message}");
    }

    #[test]
    fn too_large_reuses_the_converter_wording() {
        assert_eq!(
            user_message(&HeicFailure::TooLarge),
            format!(
                "image has more than the {} MP limit",
                MAX_PIXELS / 1_000_000
            )
        );
        assert_eq!(
            user_message(&HeicFailure::TooLarge),
            "image has more than the 80 MP limit"
        );
    }
}
