//! Pure Organize logic: the category tables, the classification of one
//! file, the skip rules, and unique-name generation. Everything here works
//! on plain data (no file system, no Windows API) so every rule is unit-
//! testable; `scan.rs` maps the real directory listing onto these types.

/// The three organize targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrganizeTarget {
    Desktop,
    Downloads,
    Custom,
}

impl OrganizeTarget {
    pub fn id(self) -> &'static str {
        match self {
            OrganizeTarget::Desktop => "desktop",
            OrganizeTarget::Downloads => "downloads",
            OrganizeTarget::Custom => "custom",
        }
    }

    pub fn from_id(id: &str) -> Option<OrganizeTarget> {
        match id {
            "desktop" => Some(OrganizeTarget::Desktop),
            "downloads" => Some(OrganizeTarget::Downloads),
            "custom" => Some(OrganizeTarget::Custom),
            _ => None,
        }
    }
}

/// The attributes the skip rules care about, mapped from the raw Windows
/// flags by `scan.rs` (zeros in tests and on other platforms).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EntryFlags {
    pub hidden: bool,
    pub system: bool,
    /// A symlink, junction or any other reparse point — never entered,
    /// never moved, never used as a category folder.
    pub reparse: bool,
    /// OneDrive cloud-only placeholder (any of OFFLINE, RECALL_ON_OPEN,
    /// RECALL_ON_DATA_ACCESS): reading it would download it — never move.
    pub cloud_only: bool,
}

/// A plain top-level entry as the skip rules see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrganizeEntry {
    pub name: String,
    pub flags: EntryFlags,
    pub is_dir: bool,
}

/// Why one file is not moved. Plain English; shown in the preview/report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    HiddenOrSystem,
    OfficeLockFile,
    ReparsePoint,
    PartialDownload,
    CloudOnly,
}

impl SkipReason {
    pub fn text(self) -> &'static str {
        match self {
            SkipReason::HiddenOrSystem => "hidden or system file",
            SkipReason::OfficeLockFile => "temporary Office lock file",
            SkipReason::ReparsePoint => "a link or reparse point",
            SkipReason::PartialDownload => "still downloading",
            SkipReason::CloudOnly => "online-only (OneDrive)",
        }
    }
}

/// What happens to one file. `Category` carries the folder name to move it
/// into (existing folders are reused, never replaced).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    Category(String),
    /// Custom folders only: executable/runtime/unknown files stay put. This
    /// is a decision, not an error ("left in place" in the preview).
    LeaveInPlace,
    Skip(SkipReason),
}

/// The Desktop "Apps" list.
pub const EXT_APPS: &[&str] = &["exe", "msi", "msix", "appx", "lnk", "url", "appref-ms"];

/// The "Documents" list (Desktop, Downloads and Custom share it).
pub const EXT_DOCUMENTS: &[&str] = &[
    "pdf", "doc", "docx", "odt", "rtf", "txt", "md", "xls", "xlsx", "ods", "csv", "ppt", "pptx",
    "odp", "epub",
];

pub const EXT_IMAGES: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "webp", "tif", "tiff", "ico", "svg", "heic", "heif", "avif",
];

pub const EXT_VIDEOS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg",
];

pub const EXT_MUSIC: &[&str] = &["mp3", "wav", "flac", "aac", "ogg", "m4a", "wma", "opus"];

pub const EXT_ARCHIVES: &[&str] = &["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "tgz", "iso"];

/// The Downloads "Installers" list (wider than Desktop's Apps list).
pub const EXT_INSTALLERS: &[&str] = &["exe", "msi", "msix", "msixbundle", "appx", "appxbundle"];

/// Partial-download extensions: a file still being written by a browser.
pub const EXT_PARTIAL: &[&str] = &[
    "crdownload",
    "part",
    "partial",
    "download",
    "opdownload",
    "tmp",
];

/// The lowercase extension of a file name (text after the last dot),
/// Windows-style: a leading dot makes the whole name the file name
/// (".gitignore" has NO extension), everything else follows the last dot.
pub fn extension_of(file_name: &str) -> Option<String> {
    let path = std::path::Path::new(file_name);
    path.extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
}

fn in_list(extension: Option<&str>, list: &[&str]) -> bool {
    extension.is_some_and(|ext| list.contains(&ext))
}

/// The skip rules that apply to every target (spec 5d "Shared safety rules").
/// Order matters: the most specific reason wins.
pub fn skip_reason(entry: &OrganizeEntry) -> Option<SkipReason> {
    // Real folders are never touched and never even reported — handled by
    // the scan, which only feeds files here. A reparse "file" (symlink) is
    // never moved.
    if entry.flags.reparse {
        return Some(SkipReason::ReparsePoint);
    }
    if entry.flags.hidden || entry.flags.system {
        return Some(SkipReason::HiddenOrSystem);
    }
    let lower = entry.name.to_ascii_lowercase();
    if lower == "desktop.ini" {
        return Some(SkipReason::HiddenOrSystem);
    }
    if entry.name.starts_with("~$") {
        return Some(SkipReason::OfficeLockFile);
    }
    if in_list(extension_of(&entry.name).as_deref(), EXT_PARTIAL) {
        return Some(SkipReason::PartialDownload);
    }
    if entry.flags.cloud_only {
        return Some(SkipReason::CloudOnly);
    }
    None
}

/// Classifies one file for one target.
pub fn classify(file_name: &str, target: OrganizeTarget) -> Destination {
    let entry = OrganizeEntry {
        name: file_name.to_string(),
        flags: EntryFlags::default(),
        is_dir: false,
    };
    if let Some(reason) = skip_reason(&entry) {
        return Destination::Skip(reason);
    }
    let ext = extension_of(file_name);
    match target {
        OrganizeTarget::Desktop => {
            if in_list(ext.as_deref(), EXT_APPS) {
                Destination::Category("Apps".to_string())
            } else if in_list(ext.as_deref(), EXT_DOCUMENTS) {
                Destination::Category("Documents".to_string())
            } else {
                Destination::Category("More".to_string())
            }
        }
        OrganizeTarget::Downloads => {
            if in_list(ext.as_deref(), EXT_DOCUMENTS) {
                Destination::Category("Documents".to_string())
            } else if in_list(ext.as_deref(), EXT_IMAGES) {
                Destination::Category("Images".to_string())
            } else if in_list(ext.as_deref(), EXT_VIDEOS) {
                Destination::Category("Videos".to_string())
            } else if in_list(ext.as_deref(), EXT_MUSIC) {
                Destination::Category("Music".to_string())
            } else if in_list(ext.as_deref(), EXT_ARCHIVES) {
                Destination::Category("Archives".to_string())
            } else if in_list(ext.as_deref(), EXT_INSTALLERS) {
                Destination::Category("Installers".to_string())
            } else {
                Destination::Category("More".to_string())
            }
        }
        OrganizeTarget::Custom => {
            if in_list(ext.as_deref(), EXT_DOCUMENTS) {
                Destination::Category("Documents".to_string())
            } else if in_list(ext.as_deref(), EXT_IMAGES) {
                Destination::Category("Images".to_string())
            } else if in_list(ext.as_deref(), EXT_VIDEOS) {
                Destination::Category("Videos".to_string())
            } else if in_list(ext.as_deref(), EXT_MUSIC) {
                Destination::Category("Music".to_string())
            } else if in_list(ext.as_deref(), EXT_ARCHIVES) {
                Destination::Category("Archives".to_string())
            } else {
                // Moving runtime files could break a program if the user
                // picked the wrong folder — executables, libraries, scripts
                // and anything unknown stay where they are.
                Destination::LeaveInPlace
            }
        }
    }
}

/// Builds the next free destination name: `report.pdf`, `report (2).pdf`,
/// … up to 999 tries. `exists` must report whether the FULL destination
/// path already exists (directories included).
pub fn unique_name(original: &str, exists: &dyn Fn(&str) -> bool) -> Result<String, String> {
    if !exists(original) {
        return Ok(original.to_string());
    }
    let (stem, ext) = match std::path::Path::new(original).extension() {
        Some(ext) => {
            let stem = &original[..original.len() - ext.len() - 1];
            (stem.to_string(), format!(".{}", ext.to_string_lossy()))
        }
        None => (original.to_string(), String::new()),
    };
    for n in 2..=999 {
        let candidate = format!("{stem} ({n}){ext}");
        if !exists(&candidate) {
            return Ok(candidate);
        }
    }
    Err(format!(
        "No free name found for '{original}' after 999 tries."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, flags: EntryFlags) -> OrganizeEntry {
        OrganizeEntry {
            name: name.to_string(),
            flags,
            is_dir: false,
        }
    }

    #[test]
    fn extensions_follow_the_last_dot_case_insensitively() {
        assert_eq!(extension_of("Report.PDF"), Some("pdf".into()));
        assert_eq!(extension_of("a.tar.gz"), Some("gz".into()));
        assert_eq!(extension_of("noext"), None);
        // Windows-style: a leading dot is not an extension.
        assert_eq!(extension_of(".gitignore"), None);
        assert_eq!(extension_of("archive.TAR.GZ"), Some("gz".into()));
    }

    #[test]
    fn desktop_categories() {
        assert_eq!(
            classify("setup.EXE", OrganizeTarget::Desktop),
            Destination::Category("Apps".to_string())
        );
        assert_eq!(
            classify("tool.lnk", OrganizeTarget::Desktop),
            Destination::Category("Apps".to_string())
        );
        assert_eq!(
            classify("page.url", OrganizeTarget::Desktop),
            Destination::Category("Apps".to_string())
        );
        assert_eq!(
            classify("notes.md", OrganizeTarget::Desktop),
            Destination::Category("Documents".to_string())
        );
        // A .png on the Desktop goes to More (Desktop has no Images category).
        assert_eq!(
            classify("photo.png", OrganizeTarget::Desktop),
            Destination::Category("More".to_string())
        );
        assert_eq!(
            classify("data.xyz", OrganizeTarget::Desktop),
            Destination::Category("More".to_string())
        );
        assert_eq!(
            classify("noext", OrganizeTarget::Desktop),
            Destination::Category("More".to_string())
        );
    }

    #[test]
    fn downloads_categories() {
        assert_eq!(
            classify("report.pdf", OrganizeTarget::Downloads),
            Destination::Category("Documents".to_string())
        );
        assert_eq!(
            classify("p.jpg", OrganizeTarget::Downloads),
            Destination::Category("Images".to_string())
        );
        assert_eq!(
            classify("v.mkv", OrganizeTarget::Downloads),
            Destination::Category("Videos".to_string())
        );
        assert_eq!(
            classify("s.mp3", OrganizeTarget::Downloads),
            Destination::Category("Music".to_string())
        );
        assert_eq!(
            classify("a.7z", OrganizeTarget::Downloads),
            Destination::Category("Archives".to_string())
        );
        // Downloads .exe → Installers (not Apps; that list is Desktop-only).
        assert_eq!(
            classify("installer.exe", OrganizeTarget::Downloads),
            Destination::Category("Installers".to_string())
        );
        assert_eq!(
            classify("bundle.msixbundle", OrganizeTarget::Downloads),
            Destination::Category("Installers".to_string())
        );
        assert_eq!(
            classify("x.xyz", OrganizeTarget::Downloads),
            Destination::Category("More".to_string())
        );
    }

    #[test]
    fn custom_categories_leave_runtime_and_unknown_files_in_place() {
        assert_eq!(
            classify("doc.pdf", OrganizeTarget::Custom),
            Destination::Category("Documents".to_string())
        );
        assert_eq!(
            classify("i.png", OrganizeTarget::Custom),
            Destination::Category("Images".to_string())
        );
        assert_eq!(
            classify("v.mp4", OrganizeTarget::Custom),
            Destination::Category("Videos".to_string())
        );
        assert_eq!(
            classify("m.flac", OrganizeTarget::Custom),
            Destination::Category("Music".to_string())
        );
        assert_eq!(
            classify("a.zip", OrganizeTarget::Custom),
            Destination::Category("Archives".to_string())
        );
        // Everything else stays: executables, libraries, ini, scripts, unknown.
        for name in [
            "prog.exe",
            "lib.dll",
            "driver.sys",
            "config.ini",
            "run.sh",
            "file.xyz",
            "noext",
        ] {
            assert_eq!(
                classify(name, OrganizeTarget::Custom),
                Destination::LeaveInPlace,
                "{name}"
            );
        }
        // .tar.gz → by the last extension (gz → Archives).
        assert_eq!(
            classify("b.tar.gz", OrganizeTarget::Custom),
            Destination::Category("Archives".to_string())
        );
    }

    #[test]
    fn skip_rules_hidden_system_desktop_ini_lockfile() {
        assert_eq!(
            skip_reason(&entry(
                "a.txt",
                EntryFlags {
                    hidden: true,
                    ..Default::default()
                }
            )),
            Some(SkipReason::HiddenOrSystem)
        );
        assert_eq!(
            skip_reason(&entry(
                "a.txt",
                EntryFlags {
                    system: true,
                    ..Default::default()
                }
            )),
            Some(SkipReason::HiddenOrSystem)
        );
        assert_eq!(
            skip_reason(&entry("Desktop.INI", EntryFlags::default())),
            Some(SkipReason::HiddenOrSystem)
        );
        assert_eq!(
            skip_reason(&entry("~$report.docx", EntryFlags::default())),
            Some(SkipReason::OfficeLockFile)
        );
    }

    #[test]
    fn skip_rules_partial_reparse_cloud_only() {
        for name in [
            "f.crdownload",
            "f.part",
            "f.partial",
            "f.download",
            "f.opdownload",
            "f.tmp",
            "f.TMP",
        ] {
            assert_eq!(
                skip_reason(&entry(name, EntryFlags::default())),
                Some(SkipReason::PartialDownload),
                "{name}"
            );
        }
        assert_eq!(
            skip_reason(&entry(
                "link.txt",
                EntryFlags {
                    reparse: true,
                    ..Default::default()
                }
            )),
            Some(SkipReason::ReparsePoint)
        );
        assert_eq!(
            skip_reason(&entry(
                "cloud.docx",
                EntryFlags {
                    cloud_only: true,
                    ..Default::default()
                }
            )),
            Some(SkipReason::CloudOnly)
        );
        // A clean file is never skipped.
        assert_eq!(
            skip_reason(&entry("clean.txt", EntryFlags::default())),
            None
        );
    }

    #[test]
    fn skip_rules_beat_classification() {
        assert_eq!(
            classify("still.crdownload", OrganizeTarget::Downloads),
            Destination::Skip(SkipReason::PartialDownload)
        );
        assert_eq!(
            classify("~$x.docx", OrganizeTarget::Custom),
            Destination::Skip(SkipReason::OfficeLockFile)
        );
    }

    #[test]
    fn unique_name_appends_two_three() {
        let taken = ["report.pdf", "report (2).pdf", "report (3).pdf"];
        let exists = |name: &str| taken.contains(&name);
        assert_eq!(
            unique_name("report.pdf", &exists),
            Ok("report (4).pdf".into())
        );
    }

    #[test]
    fn unique_name_without_extension() {
        let exists = |name: &str| name == "README";
        assert_eq!(unique_name("README", &exists), Ok("README (2)".into()));
    }

    #[test]
    fn unique_name_first_choice_free() {
        let exists = |_name: &str| false;
        assert_eq!(unique_name("report.pdf", &exists), Ok("report.pdf".into()));
    }

    #[test]
    fn unique_name_gives_up_after_999() {
        let exists = |_name: &str| true;
        assert!(unique_name("a.txt", &exists).is_err());
    }

    #[test]
    fn target_ids_round_trip() {
        for target in [
            OrganizeTarget::Desktop,
            OrganizeTarget::Downloads,
            OrganizeTarget::Custom,
        ] {
            assert_eq!(OrganizeTarget::from_id(target.id()), Some(target));
        }
        assert_eq!(OrganizeTarget::from_id("bogus"), None);
    }
}
