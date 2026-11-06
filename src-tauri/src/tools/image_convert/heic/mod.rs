//! HEIC/HEIF input support for the Image Converter.
//!
//! - `sniff`: pure ISO-BMFF `ftyp` content sniffing (no I/O).
//! - `error`: pure HRESULT classification and user-facing messages.
//! - `wic`: the Windows WIC decoder — the only file with unsafe/COM code.
//!
//! The app ships no decoder: WIC uses Microsoft's HEIF/HEVC extensions
//! installed on the user's PC. Missing codecs are an expected situation and
//! are reported honestly through `error::user_message`.

pub mod error;
pub mod sniff;
pub mod wic;
