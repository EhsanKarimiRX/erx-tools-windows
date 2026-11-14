//! Safe names for restored files.
//!
//! SECURITY: the original file name comes out of the encrypted data and was
//! chosen by whoever made the file, so it can be hostile (`..\..\x.exe`,
//! `CON`, an alternate data stream, a right-to-left override that disguises
//! an extension). A name is only used if it is a plain Windows file name.

/// Longest file name Windows accepts (UTF-16 units).
const MAX_NAME_UNITS: usize = 255;

/// Windows device names; they are reserved with or without an extension.
const RESERVED_DEVICE_NAMES: &[&str] = &[
    "con",
    "prn",
    "aux",
    "nul",
    "com1",
    "com2",
    "com3",
    "com4",
    "com5",
    "com6",
    "com7",
    "com8",
    "com9",
    "lpt1",
    "lpt2",
    "lpt3",
    "lpt4",
    "lpt5",
    "lpt6",
    "lpt7",
    "lpt8",
    "lpt9",
    "com\u{b9}",
    "com\u{b2}",
    "com\u{b3}",
    "lpt\u{b9}",
    "lpt\u{b2}",
    "lpt\u{b3}",
];

fn is_forbidden_char(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '<' | '>'
                | ':'
                | '"'
                | '/'
                | '\\'
                | '|'
                | '?'
                | '*'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{061c}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

/// The name itself when it is safe to create a file with it, else `None`.
pub fn safe_file_name(name: &str) -> Option<String> {
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    if name.encode_utf16().count() > MAX_NAME_UNITS {
        return None;
    }
    if name.starts_with(' ') || name.ends_with(' ') || name.ends_with('.') {
        return None;
    }
    if name.chars().any(is_forbidden_char) {
        return None;
    }
    // Our own temporary files must never be mistaken for restored files.
    if name.to_lowercase().ends_with(".erxtmp") {
        return None;
    }
    let base = name.split('.').next().unwrap_or(name);
    let device = base.trim_end_matches(' ').to_lowercase();
    if RESERVED_DEVICE_NAMES.contains(&device.as_str()) {
        return None;
    }
    Some(name.to_owned())
}

/// `photo.jpg` and 2 give `photo (2).jpg`; a name without an extension
/// gets the number at the end.
pub fn numbered_name(name: &str, number: u32) -> String {
    if let Some(dot) = name.rfind('.')
        && dot > 0
        && let (Some(stem), Some(extension)) = (name.get(..dot), name.get(dot..))
    {
        return format!("{stem} ({number}){extension}");
    }
    format!("{name} ({number})")
}
