//! desktop.ini reading and editing, preserving everything we don't touch:
//! other sections and keys, the file's encoding (UTF-16 LE with BOM, UTF-8
//! with/without BOM, ANSI) and its line endings. Pure logic, unit tested;
//! the apply/restore layer does the file attribute dance around it.
//!
//! Safety rules (spec 4.4):
//! - A file that cannot be decoded safely is REFUSED, never overwritten.
//! - Editing touches exactly one kind of line: `IconResource=…` inside the
//!   `[.ShellClassInfo]` section. All other lines round-trip byte-exactly.
//! - Duplicate `[.ShellClassInfo]` sections are ambiguous → refused.

/// How the file is encoded on disk. ANSI is handled as latin-1, which is a
/// byte-exact round trip for every possible byte value (the edited lines
/// are pure ASCII, so this stays safe for Windows-1252 content we keep).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf16Le,
    Utf8,
    Ansi,
}

/// Line ending style of one line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnd {
    CrLf,
    Lf,
    /// The last line may have no terminator at all.
    None,
}

/// A decode/parse failure. Never overwritten, only reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IniError(pub String);

impl std::fmt::Display for IniError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A parsed desktop.ini: the exact line list plus the encoding.
#[derive(Debug, Clone)]
pub struct DesktopIni {
    lines: Vec<(String, LineEnd)>,
    encoding: Encoding,
    /// Whether a plain-UTF-8 file carried a BOM (re-emitted on write).
    utf8_bom: bool,
    /// The dominant terminator of the original file (used for new lines).
    dominant_end: LineEnd,
}

impl DesktopIni {
    /// Parses desktop.ini bytes. Fails for undecodable content (e.g.
    /// broken UTF-16) — the caller must NOT overwrite the file then.
    pub fn parse(bytes: &[u8]) -> Result<Self, IniError> {
        if bytes.is_empty() {
            return Ok(Self {
                lines: Vec::new(),
                encoding: Encoding::Utf8,
                utf8_bom: false,
                dominant_end: LineEnd::CrLf,
            });
        }

        let (text, encoding);
        let mut utf8_bom = false;
        (text, encoding) = if bytes.starts_with(&[0xFF, 0xFE]) {
            // UTF-16 LE with BOM. Odd byte counts are broken files.
            if !bytes.len().is_multiple_of(2) {
                return Err(IniError(
                    "desktop.ini is damaged (broken UTF-16 length).".into(),
                ));
            }
            let units: Vec<u16> = bytes[2..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect();
            let text = String::from_utf16(&units)
                .map_err(|_| IniError("desktop.ini is damaged (invalid UTF-16 content).".into()))?;
            (text, Encoding::Utf16Le)
        } else if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            let text = String::from_utf8(bytes[3..].to_vec())
                .map_err(|_| IniError("desktop.ini is damaged (invalid UTF-8).".into()))?;
            utf8_bom = true;
            (text, Encoding::Utf8)
        } else {
            match std::str::from_utf8(bytes) {
                Ok(text) => (text.to_string(), Encoding::Utf8),
                // ANSI: latin-1 maps every byte to U+0000..U+00FF and back,
                // so the untouched lines re-encode byte-exactly.
                Err(_) => (bytes.iter().map(|&b| b as char).collect(), Encoding::Ansi),
            }
        };

        // Split into lines, remembering each line's own terminator so
        // mixed-ending files round-trip exactly.
        let mut lines = Vec::new();
        let mut dominant_end = LineEnd::None;
        let mut rest = text.as_str();
        while let Some(position) = rest.find('\n') {
            let (line, after) = rest.split_at(position);
            let line = line.strip_suffix('\r').unwrap_or(line);
            let end = if rest[..position].ends_with('\r') {
                LineEnd::CrLf
            } else {
                LineEnd::Lf
            };
            if matches!(dominant_end, LineEnd::None) {
                dominant_end = end;
            }
            lines.push((line.to_string(), end));
            rest = &after[1..];
        }
        if !rest.is_empty() {
            lines.push((rest.to_string(), LineEnd::None));
        }
        if matches!(dominant_end, LineEnd::None) {
            dominant_end = LineEnd::CrLf; // new files: the Windows default
        }

        // Ambiguous layout: two [.ShellClassInfo] sections would make the
        // icon key's meaning undefined — refuse rather than guess.
        let class_sections = lines
            .iter()
            .filter(|(line, _)| is_section_header(line, ".ShellClassInfo"))
            .count();
        if class_sections > 1 {
            return Err(IniError(
                "desktop.ini has more than one [.ShellClassInfo] section.".into(),
            ));
        }

        Ok(Self {
            lines,
            encoding,
            utf8_bom,
            dominant_end,
        })
    }

    /// A brand-new file: UTF-8 without BOM (the content this tool writes
    /// is pure ASCII, which every Explorer version reads correctly; the
    /// encoding is recorded so later edits keep it).
    pub fn new_file() -> Self {
        Self {
            lines: Vec::new(),
            encoding: Encoding::Utf8,
            utf8_bom: false,
            dominant_end: LineEnd::CrLf,
        }
    }

    /// The `IconResource` value inside `[.ShellClassInfo]`, if present.
    pub fn icon_resource(&self) -> Option<&str> {
        let section = self.class_section_range()?;
        self.lines[section]
            .iter()
            .find(|(line, _)| key_name(line).eq_ignore_ascii_case("IconResource"))
            .map(|(line, _)| value_after_equal(line))
    }

    /// Sets `IconResource=<value>` (adding the `,0` index) inside
    /// `[.ShellClassInfo]`, creating the section at the end if needed.
    pub fn set_icon_resource(&mut self, value: &str) {
        let new_line = format!("IconResource={value},0");
        if let Some(section) = self.class_section_range() {
            for index in section.clone() {
                if key_name(&self.lines[index].0).eq_ignore_ascii_case("IconResource") {
                    let end = self.lines[index].1;
                    self.lines[index] = (new_line, end);
                    return;
                }
            }
            // Insert right after the section header.
            let end = self.dominant_end;
            self.lines.insert(section.start + 1, (new_line, end));
        } else {
            let end = self.dominant_end;
            // Make sure the previous last line ends with a terminator.
            if let Some(last) = self.lines.last_mut()
                && last.1 == LineEnd::None
            {
                last.1 = end;
            }
            self.lines.push(("[.ShellClassInfo]".to_string(), end));
            self.lines.push((new_line, end));
        }
    }

    /// Removes every `IconResource` line from `[.ShellClassInfo]`.
    pub fn remove_icon_resource(&mut self) {
        let Some(section) = self.class_section_range() else {
            return;
        };
        let end = self.dominant_end;
        // Collect the line contents to drop, then rebuild (borrow rules).
        let mut keep: Vec<(String, LineEnd)> = Vec::new();
        for (index, line) in self.lines.iter().enumerate() {
            let in_section = section.contains(&index);
            let is_icon = in_section && key_name(&line.0).eq_ignore_ascii_case("IconResource");
            if is_icon {
                // If the removed line was last without terminator, the new
                // last line must not gain one out of nowhere — handled by
                // the trailing fixup below.
                continue;
            }
            keep.push(line.clone());
        }
        self.lines = keep;
        let _ = end;
    }

    /// True when the file no longer carries any `Key=Value` line — only
    /// blank lines and empty section headers — i.e. nothing meaningful.
    pub fn is_meaningless(&self) -> bool {
        !self
            .lines
            .iter()
            .any(|(line, _)| line.contains('=') && !line.trim_start().starts_with(';'))
    }

    /// The line range (indices) of the `[.ShellClassInfo]` section.
    fn class_section_range(&self) -> Option<std::ops::Range<usize>> {
        let start = self
            .lines
            .iter()
            .position(|(line, _)| is_section_header(line, ".ShellClassInfo"))?;
        let end = self.lines[start + 1..]
            .iter()
            .position(|(line, _)| line.trim_start().starts_with('['))
            .map(|offset| start + 1 + offset)
            .unwrap_or(self.lines.len());
        Some(start..end)
    }

    /// Serializes back to bytes in the ORIGINAL encoding with the ORIGINAL
    /// per-line endings.
    pub fn serialize(&self) -> Vec<u8> {
        let mut text = String::new();
        for (line, end) in &self.lines {
            text.push_str(line);
            match end {
                LineEnd::CrLf => text.push_str("\r\n"),
                LineEnd::Lf => text.push('\n'),
                LineEnd::None => {}
            }
        }
        match self.encoding {
            Encoding::Utf16Le => {
                let mut out = Vec::with_capacity((text.len() + 1) * 2);
                out.extend_from_slice(&[0xFF, 0xFE]);
                for unit in text.encode_utf16() {
                    out.extend_from_slice(&unit.to_le_bytes());
                }
                out
            }
            Encoding::Utf8 => {
                let mut out = Vec::with_capacity(text.len() + 3);
                if self.utf8_bom {
                    out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
                }
                out.extend_from_slice(text.as_bytes());
                out
            }
            Encoding::Ansi => text.chars().map(|c| c as u32 as u8).collect(),
        }
    }
}

/// True when the line is the section header `[name]` (case-insensitive).
fn is_section_header(line: &str, name: &str) -> bool {
    let trimmed = line.trim();
    trimmed
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .is_some_and(|header| header.trim().eq_ignore_ascii_case(name))
}

/// The key name of a `Key=Value` line (empty for non-key lines).
fn key_name(line: &str) -> &str {
    match line.split_once('=') {
        Some((name, _)) => name.trim(),
        None => "",
    }
}

/// The value part of a `Key=Value` line, trimmed.
fn value_after_equal(line: &str) -> &str {
    line.split_once('=')
        .map(|(_, value)| value.trim())
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_file_round_trips_minimally() {
        let mut ini = DesktopIni::new_file();
        assert!(ini.icon_resource().is_none());
        ini.set_icon_resource("erx-folder-icon-1a2b3c4d.ico");
        assert_eq!(ini.icon_resource(), Some("erx-folder-icon-1a2b3c4d.ico,0"));
        let bytes = ini.serialize();
        let text = String::from_utf8(bytes).unwrap();
        assert!(
            text.starts_with(
                "[.ShellClassInfo]\r\nIconResource=erx-folder-icon-1a2b3c4d.ico,0\r\n"
            )
        );
    }

    #[test]
    fn existing_keys_and_sections_are_preserved() {
        let original = b"[.ShellClassInfo]\r\nInfoTip=My custom tip\r\nLocalizedResourceName=Album\r\n\r\n[ViewState]\r\nMode=\r\nVid=FolderView\r\n";
        let mut ini = DesktopIni::parse(original).unwrap();
        assert!(ini.icon_resource().is_none());
        ini.set_icon_resource("erx-folder-icon-abcdef01.ico");
        let text = String::from_utf8(ini.serialize()).unwrap();
        assert!(text.contains("InfoTip=My custom tip\r\n"));
        assert!(text.contains("LocalizedResourceName=Album\r\n"));
        assert!(text.contains("[ViewState]\r\nMode=\r\nVid=FolderView\r\n"));
        // The new key landed inside [.ShellClassInfo], before InfoTip.
        let class_index = text.find("[.ShellClassInfo]").unwrap();
        let info_index = text.find("IconResource=").unwrap();
        let tip_index = text.find("InfoTip=").unwrap();
        assert!(class_index < info_index && info_index < tip_index);
    }

    #[test]
    fn replacing_keeps_everything_else_and_stays_idempotent() {
        let original =
            b"[.ShellClassInfo]\r\nIconResource=C:\\old\\folder.ico,0\r\nInfoTip=Keep me\r\n";
        let mut ini = DesktopIni::parse(original).unwrap();
        assert_eq!(ini.icon_resource(), Some(r"C:\old\folder.ico,0"));
        ini.set_icon_resource("erx-folder-icon-11111111.ico");
        let once = ini.serialize();
        assert!(!once.windows(5).any(|w| w == b"old\\f"), "old value gone");
        assert!(
            String::from_utf8(once.clone())
                .unwrap()
                .contains("InfoTip=Keep me\r\n")
        );

        // Re-applying the same icon is byte-identical.
        let mut again = DesktopIni::parse(&once).unwrap();
        again.set_icon_resource("erx-folder-icon-11111111.ico");
        assert_eq!(again.serialize(), once);
    }

    #[test]
    fn utf16_le_files_round_trip_with_bom_and_endings() {
        let mut utf16: Vec<u8> = vec![0xFF, 0xFE];
        for unit in "[.ShellClassInfo]\r\nInfoTip=S\u{00E9}\r\n".encode_utf16() {
            utf16.extend_from_slice(&unit.to_le_bytes());
        }
        let mut ini = DesktopIni::parse(&utf16).unwrap();
        assert_eq!(ini.encoding, Encoding::Utf16Le);
        ini.set_icon_resource("erx-folder-icon-22222222.ico");
        let out = ini.serialize();
        assert!(out.starts_with(&[0xFF, 0xFE]));
        let parsed = DesktopIni::parse(&out).unwrap();
        assert_eq!(
            parsed.icon_resource(),
            Some("erx-folder-icon-22222222.ico,0")
        );
        assert!(parsed.icon_resource().is_some());
        let text_units: Vec<u16> = out[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_le_bytes(*p))
            .collect();
        let text = String::from_utf16(&text_units).unwrap();
        assert!(
            text.contains("InfoTip=S\u{00E9}\r\n"),
            "high ANSI key preserved"
        );
        assert!(text.contains("\r\n"), "CRLF kept");
    }

    #[test]
    fn broken_utf16_is_refused_not_overwritten() {
        // BOM + odd byte count = damaged file.
        let broken = vec![0xFF, 0xFE, 0x41];
        assert!(DesktopIni::parse(&broken).is_err());
        // BOM + lone surrogate.
        let broken: Vec<u8> = vec![0xFF, 0xFE, 0x00, 0xD8];
        assert!(DesktopIni::parse(&broken).is_err());
    }

    #[test]
    fn ansi_files_round_trip_byte_exactly() {
        // Windows-1252-ish bytes: 0xE9 = é, 0x93 = smart quote. latin-1
        // decode/encode keeps every byte.
        let original: Vec<u8> = vec![
            b'[', b'.', b'S', b'h', b'e', b'l', b'l', b'C', b'l', b'a', b's', b's', b'I', b'n',
            b'f', b'o', b']', b'\r', b'\n', b'I', b'n', b'f', b'o', b'T', b'i', b'p', b'=', 0x93,
            0xE9, 0x94, b'\r', b'\n',
        ];
        let mut ini = DesktopIni::parse(&original).unwrap();
        assert_eq!(ini.encoding, Encoding::Ansi);
        ini.set_icon_resource("erx-folder-icon-33333333.ico");
        let out = ini.serialize();
        // The untouched line's bytes survive exactly (including the
        // Windows-1252 high bytes) — our line was inserted between them.
        assert!(out.windows(3).any(|w| w == [0x93, 0xE9, 0x94].as_slice()));
        assert!(out.windows(8).any(|w| w == b"InfoTip="));
        assert!(out.windows(2).any(|w| w == b"\r\n"));
        let reparsed = DesktopIni::parse(&out).unwrap();
        assert_eq!(reparsed.encoding, Encoding::Ansi);
        assert_eq!(
            reparsed.icon_resource(),
            Some("erx-folder-icon-33333333.ico,0")
        );
    }

    #[test]
    fn utf8_bom_is_detected_and_kept() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"[.ShellClassInfo]\r\nInfoTip=caf\xC3\xA9\r\n");
        let mut ini = DesktopIni::parse(&bytes).unwrap();
        assert_eq!(ini.encoding, Encoding::Utf8);
        ini.set_icon_resource("erx-folder-icon-44444444.ico");
        let out = ini.serialize();
        assert!(out.starts_with(&[0xEF, 0xBB, 0xBF]));
        assert!(String::from_utf8_lossy(&out).contains("caf\u{e9}"));
    }

    #[test]
    fn lf_only_files_keep_lf() {
        let original = b"[.ShellClassInfo]\nInfoTip=x\n";
        let mut ini = DesktopIni::parse(original).unwrap();
        ini.set_icon_resource("erx-folder-icon-55555555.ico");
        let text = String::from_utf8(ini.serialize()).unwrap();
        assert!(!text.contains("\r\n"), "no CRLF introduced");
        assert!(text.contains("\n"));
    }

    #[test]
    fn remove_deletes_only_the_icon_key() {
        let original = b"[.ShellClassInfo]\r\nIconResource=erx-folder-icon-66666666.ico,0\r\nInfoTip=Stay\r\nIconResource=erx-folder-icon-77777777.ico,0\r\n";
        let mut ini = DesktopIni::parse(original).unwrap();
        ini.remove_icon_resource();
        assert!(ini.icon_resource().is_none());
        let text = String::from_utf8(ini.serialize()).unwrap();
        assert!(text.contains("InfoTip=Stay\r\n"));
        assert!(!text.contains("erx-folder-icon"));
        assert!(text.contains("[.ShellClassInfo]\r\n"));
    }

    #[test]
    fn meaninglessness_detects_empty_remainders() {
        let only_headers = b"[.ShellClassInfo]\r\n\r\n[ViewState]\r\n";
        let ini = DesktopIni::parse(only_headers).unwrap();
        assert!(ini.is_meaningless());
        let with_info = b"[.ShellClassInfo]\r\nInfoTip=Something\r\n";
        let ini = DesktopIni::parse(with_info).unwrap();
        assert!(!ini.is_meaningless());
        let empty = DesktopIni::new_file();
        assert!(empty.is_meaningless());
    }

    #[test]
    fn duplicate_class_sections_are_refused() {
        let original = b"[.ShellClassInfo]\r\nInfoTip=a\r\n[.ShellClassInfo]\r\nInfoTip=b\r\n";
        assert!(DesktopIni::parse(original).is_err());
    }

    #[test]
    fn case_insensitive_section_and_key_matching() {
        let original = b"[.shellclassinfo]\r\niconresource=erx-folder-icon-88888888.ico,0\r\n";
        let ini = DesktopIni::parse(original).unwrap();
        assert_eq!(ini.icon_resource(), Some("erx-folder-icon-88888888.ico,0"));
    }

    #[test]
    fn file_without_trailing_newline_is_preserved() {
        let original = b"[.ShellClassInfo]\r\nInfoTip=last line no newline";
        let mut ini = DesktopIni::parse(original).unwrap();
        ini.set_icon_resource("erx-folder-icon-99999999.ico");
        let text = String::from_utf8(ini.serialize()).unwrap();
        // The key was inserted after the header; the untouched last line
        // keeps its "no newline at EOF" shape.
        assert!(
            text.contains("[.ShellClassInfo]\r\nIconResource=erx-folder-icon-99999999.ico,0\r\n")
        );
        assert!(text.ends_with("InfoTip=last line no newline"));
    }

    #[test]
    fn empty_file_is_a_valid_start() {
        let ini = DesktopIni::parse(b"").unwrap();
        assert!(ini.icon_resource().is_none());
        assert!(ini.is_meaningless());
    }
}
