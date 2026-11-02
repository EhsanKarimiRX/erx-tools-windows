//! QR matrix generation on top of the `qrcodegen` crate (Nayuki, MIT,
//! pure Rust, no dependencies). Pure logic, unit tested.

use qrcodegen::{QrCode, QrCodeEcc};

/// Error correction level. Higher levels survive more damage (and make the
/// modules denser for the same content); `H` is required when a logo
/// covers part of the code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ecc {
    Low,
    Medium,
    Quartile,
    High,
}

impl Ecc {
    /// Maps the UI token; the default (`Q`) is applied by the caller.
    pub fn from_ui(value: &str) -> Result<Self, String> {
        match value {
            "L" => Ok(Ecc::Low),
            "M" => Ok(Ecc::Medium),
            "Q" => Ok(Ecc::Quartile),
            "H" => Ok(Ecc::High),
            other => Err(format!("Unknown error correction level: {other}")),
        }
    }

    pub fn to_qrcodegen(self) -> QrCodeEcc {
        match self {
            Ecc::Low => QrCodeEcc::Low,
            Ecc::Medium => QrCodeEcc::Medium,
            Ecc::Quartile => QrCodeEcc::Quartile,
            Ecc::High => QrCodeEcc::High,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Ecc::Low => "L",
            Ecc::Medium => "M",
            Ecc::Quartile => "Q",
            Ecc::High => "H",
        }
    }
}

/// The payload did not fit any QR version at this level. `max_chars` is the
/// length (in characters) of the longest prefix of the same content that
/// would fit — used in the user-facing message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooLong {
    pub max_chars: usize,
}

/// A generated QR code as a square grid of dark/light modules.
#[derive(Debug, Clone)]
pub struct Matrix {
    size: usize,
    dark: Vec<bool>,
}

impl Matrix {
    /// Encodes text (any Unicode; `qrcodegen` picks the best segment mode,
    /// falling back to UTF-8 byte mode automatically).
    pub fn from_text(text: &str, ecc: Ecc) -> Result<Matrix, TooLong> {
        let code = QrCode::encode_text(text, ecc.to_qrcodegen()).map_err(|_| TooLong {
            max_chars: max_fitting_chars(text, ecc),
        })?;
        Ok(Matrix::from_code(&code))
    }

    fn from_code(code: &QrCode) -> Matrix {
        let size = usize::try_from(code.size()).unwrap_or(0);
        let mut dark = Vec::with_capacity(size * size);
        for y in 0..size {
            for x in 0..size {
                dark.push(code.get_module(x as i32, y as i32));
            }
        }
        Matrix { size, dark }
    }

    /// Test-only constructor for exact synthetic grids.
    #[cfg(test)]
    pub fn from_dark_grid(size: usize, dark: Vec<bool>) -> Matrix {
        assert_eq!(dark.len(), size * size);
        Matrix { size, dark }
    }

    /// Side length in modules (21..=177, version 1..=40).
    pub fn size(&self) -> usize {
        self.size
    }

    /// True when the module at (x, y) is dark. Out-of-bounds counts as
    /// light — callers use this for corner-rounding at the grid border.
    pub fn is_dark(&self, x: isize, y: isize) -> bool {
        if x < 0 || y < 0 || x >= self.size as isize || y >= self.size as isize {
            return false;
        }
        self.dark[y as usize * self.size + x as usize]
    }
}

/// Length in characters of the longest prefix of `text` that still fits a
/// QR code at `ecc`. Binary search over character boundaries: at most
/// ~log2(char count) encode calls, each fast even at version 40.
pub fn max_fitting_chars(text: &str, ecc: Ecc) -> usize {
    // boundaries[k] = byte offset right after the k-th character.
    let boundaries: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect();
    let fits =
        |chars: usize| QrCode::encode_text(&text[..boundaries[chars]], ecc.to_qrcodegen()).is_ok();

    let total = boundaries.len() - 1;
    if fits(total) {
        return total;
    }
    let mut lo = 0usize; // empty string always fits version 1
    let mut hi = total; // known not to fit
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if fits(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// The friendly error message shown when the content is too long.
pub fn too_long_message(max_chars: usize) -> String {
    format!(
        "The content is too long for a QR code at this error correction level. \
At most {max_chars} characters fit \u{2014} shorten the text or choose a lower level (L fits the most)."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use qrcodegen::DataTooLong;

    #[test]
    fn matrix_basics() {
        let matrix = Matrix::from_text("Hello", Ecc::Quartile).expect("fits");
        assert!(matrix.size() >= 21);
        // Out of bounds is light, in-bounds corner modules exist.
        assert!(!matrix.is_dark(-1, 0));
        assert!(!matrix.is_dark(0, matrix.size() as isize));
        // Finder patterns: the very corners of a QR are always dark.
        assert!(matrix.is_dark(0, 0));
        assert!(matrix.is_dark(matrix.size() as isize - 1, 0));
        assert!(matrix.is_dark(0, matrix.size() as isize - 1));
    }

    #[test]
    fn persian_text_encodes() {
        let matrix = Matrix::from_text("\u{0633}\u{0644}\u{0627}\u{0645}", Ecc::High);
        assert!(matrix.is_ok());
    }

    #[test]
    fn too_long_error_reports_a_sane_limit() {
        // Version 40 byte mode fits 2953 bytes at L; 2953 ASCII chars at Q
        // need more than one version, so pick something comfortably over.
        let text = "a".repeat(4000);
        let error = Matrix::from_text(&text, Ecc::Quartile).unwrap_err();
        assert!(error.max_chars > 1000 && error.max_chars < 4000);
        // The reported limit really does fit.
        assert!(
            Matrix::from_text(&"a".repeat(error.max_chars), Ecc::Quartile).is_ok(),
            "reported limit must fit"
        );
        // One more character really does not.
        assert!(Matrix::from_text(&"a".repeat(error.max_chars + 1), Ecc::Quartile).is_err());
    }

    #[test]
    fn too_long_message_mentions_limit() {
        let message = too_long_message(1234);
        assert!(message.contains("1234"));
        assert!(message.contains("lower level"));
    }

    #[test]
    fn max_limit_is_multibyte_safe() {
        // Persian characters are 2 bytes each; the limit must be computed
        // on character boundaries, not raw bytes.
        let text = "\u{0633}".repeat(3000);
        let error = Matrix::from_text(&text, Ecc::Low).unwrap_err();
        assert!(error.max_chars > 0);
        assert!(Matrix::from_text(&"\u{0633}".repeat(error.max_chars), Ecc::Low).is_ok());
    }

    #[test]
    fn ecc_from_ui_and_labels() {
        assert_eq!(Ecc::from_ui("Q"), Ok(Ecc::Quartile));
        assert!(Ecc::from_ui("x").is_err());
        assert_eq!(Ecc::High.label(), "H");
    }

    #[test]
    fn datatoolong_is_only_about_capacity() {
        // Guards against the crate changing its error shape silently.
        fn describe(error: DataTooLong) -> &'static str {
            match error {
                DataTooLong::SegmentTooLong => "segment",
                DataTooLong::DataOverCapacity(..) => "capacity",
            }
        }
        assert!(matches!(
            describe(DataTooLong::SegmentTooLong),
            "segment" | "capacity"
        ));
    }
}
