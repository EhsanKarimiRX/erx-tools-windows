//! The read-only scan: resolve the real folder (known-folder API for the
//! Desktop and for the Downloads quick button), list the TOP LEVEL only,
//! map every entry onto the pure logic types, and build the plan the
//! preview shows. Never writes. Cloud-only OneDrive placeholders are
//! skipped BEFORE any content read (an EXIF attempt must never trigger a
//! download).

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::core::{Destination, EntryFlags, OrganizeEntry, OrganizeTarget, skip_reason};
use super::dates;
use super::modes::{DateSource, Mode, ModeMetadata, classify_entry, extension_of};
use super::protected::is_protected;

/// One file that WOULD move, as the preview shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    pub name: String,
    pub size: u64,
    /// Whole seconds since the epoch; the run re-checks this (with size)
    /// so files changed since the preview are skipped, and files that
    /// appeared after the preview are never moved.
    pub modified_unix: i64,
    /// The LOCAL (year, month) the file's chosen date resolves to — only
    /// meaningful in Date mode (None elsewhere or when undetermined).
    pub date_ym: Option<(i32, u8)>,
    /// The destination folder decided by the scan; the run re-checks the
    /// file but trusts this decision (a preview always matches the plan).
    pub category: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedCategory {
    pub name: String,
    pub files: Vec<PlannedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedFile {
    pub name: String,
    pub reason: String,
}

/// The full plan for one scan. Nothing has moved yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanPlan {
    pub target: OrganizeTarget,
    pub root: PathBuf,
    pub on_onedrive: bool,
    /// "format" | "date" | "size" — the UI wording matches the mode.
    pub mode_id: &'static str,
    pub categories: Vec<PlannedCategory>,
    pub skipped: Vec<SkippedFile>,
    /// Files deliberately left where they are (Custom target / protected
    /// runtime types in Date and Size modes).
    pub left_in_place: u32,
}

impl ScanPlan {
    /// Total files the plan would move (tests and assertions).
    #[cfg(test)]
    pub fn total_would_move(&self) -> u32 {
        self.categories
            .iter()
            .map(|category| category.files.len() as u32)
            .sum()
    }
}

/// Wire id of a mode, for the manifest and the UI wording.
pub fn mode_id(mode: &Mode) -> &'static str {
    match mode {
        Mode::Format => "format",
        Mode::Date { .. } => "date",
        Mode::Size => "size",
    }
}

/// True when the folder lives inside OneDrive (env override or any path
/// segment named OneDrive), so the preview can be honest about syncing.
pub fn on_onedrive(path: &Path) -> bool {
    for variable in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        if let Ok(base) = std::env::var(variable)
            && path_starts_under(path, Path::new(&base))
        {
            return true;
        }
    }
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy().to_ascii_lowercase();
        name == "onedrive" || name.starts_with("onedrive -")
    })
}

/// Component-wise, case-insensitive "path is base or under it".
fn path_starts_under(path: &Path, base: &Path) -> bool {
    let base: Vec<String> = base
        .components()
        .filter(|component| {
            !matches!(
                component,
                std::path::Component::RootDir | std::path::Component::CurDir
            )
        })
        .map(|component| component.as_os_str().to_string_lossy().to_ascii_lowercase())
        .collect();
    if base.is_empty() {
        return false;
    }
    let path: Vec<String> = path
        .components()
        .filter(|component| {
            !matches!(
                component,
                std::path::Component::RootDir | std::path::Component::CurDir
            )
        })
        .map(|component| component.as_os_str().to_string_lossy().to_ascii_lowercase())
        .collect();
    path.len() >= base.len()
        && path
            .iter()
            .zip(base.iter())
            .all(|(part, base)| part == base)
}

/// Maps raw Windows attributes onto the pure flags (0 → all clear).
#[cfg(windows)]
pub(crate) fn flags_from_attributes(attributes: u32) -> EntryFlags {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS,
        FILE_ATTRIBUTE_RECALL_ON_OPEN, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_SYSTEM,
    };
    EntryFlags {
        hidden: attributes & FILE_ATTRIBUTE_HIDDEN != 0,
        system: attributes & FILE_ATTRIBUTE_SYSTEM != 0,
        reparse: attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0,
        cloud_only: attributes
            & (FILE_ATTRIBUTE_OFFLINE
                | FILE_ATTRIBUTE_RECALL_ON_OPEN
                | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
            != 0,
    }
}

#[cfg(not(windows))]
pub(crate) fn flags_from_attributes(_attributes: u32) -> EntryFlags {
    EntryFlags::default()
}

/// Raw Windows attribute bits of the entry (0 on other platforms).
#[cfg(windows)]
pub(crate) fn raw_attributes(metadata: &std::fs::Metadata) -> u32 {
    std::os::windows::fs::MetadataExt::file_attributes(metadata)
}

#[cfg(not(windows))]
pub(crate) fn raw_attributes(_metadata: &std::fs::Metadata) -> u32 {
    0
}

pub(crate) fn modified_seconds(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(0))
        .unwrap_or(0)
}

/// The LOCAL (year, month) of a std SystemTime via the Windows time-zone
/// machinery (DST-correct per file date).
fn local_ym_of(time: std::time::SystemTime) -> Option<(i32, u8)> {
    super::modes::filetime_to_local_ym(super::modes::system_time_to_filetime(time))
}

/// Scans one folder (read-only, top level only) for one mode. Real
/// sub-folders are ignored completely — not moved, not reported. The
/// `progress` callback reports how many files have been checked so far
/// (a scan of many photos can take seconds; no fake percentages).
pub fn scan_dir(
    root: &Path,
    target: OrganizeTarget,
    mode: &Mode,
    progress: &mut dyn FnMut(u32),
) -> Result<ScanPlan, String> {
    let read =
        std::fs::read_dir(root).map_err(|error| format!("Couldn't read that folder: {error}"))?;

    let mut categories: Vec<PlannedCategory> = Vec::new();
    let mut skipped: Vec<SkippedFile> = Vec::new();
    let mut left_in_place: u32 = 0;
    let mut checked: u32 = 0;

    let mut category_files = |category: String, file: PlannedFile| {
        if let Some(existing) = categories.iter_mut().find(|c| c.name == category) {
            existing.files.push(file);
        } else {
            categories.push(PlannedCategory {
                name: category,
                files: vec![file],
            });
        }
    };

    for entry in read {
        let Ok(entry) = entry else {
            skipped.push(SkippedFile {
                name: "(unreadable entry)".into(),
                reason: "could not be read".into(),
            });
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(metadata) = entry.metadata() else {
            skipped.push(SkippedFile {
                name: name.clone(),
                reason: "could not be read".into(),
            });
            continue;
        };
        if metadata.is_dir() {
            // Real folders are never touched, renamed or moved, and are
            // not part of the plan at all.
            continue;
        }
        // Safety rules FIRST: hidden/system/reparse/partial/cloud-only
        // files are skipped before any content read, so a cloud-only
        // OneDrive placeholder is never opened for its EXIF data.
        let flags = flags_from_attributes(raw_attributes(&metadata));
        let organize_entry = OrganizeEntry {
            name: name.clone(),
            flags,
            is_dir: false,
        };
        if let Some(reason) = skip_reason(&organize_entry) {
            skipped.push(SkippedFile {
                name,
                reason: reason.text().to_string(),
            });
            continue;
        }

        // What the mode needs to know about this file.
        let size = metadata.len();
        let date_ym = match mode {
            Mode::Date {
                source: DateSource::PhotoTaken,
                ..
            } => {
                let extension = extension_of(&name).unwrap_or_default();
                if dates::exif_eligible(&extension) {
                    dates::photo_date_taken(&root.join(&name))
                } else {
                    None
                }
            }
            Mode::Date {
                source: DateSource::Created,
                ..
            } => metadata.created().ok().and_then(local_ym_of),
            Mode::Date {
                source: DateSource::Modified,
                ..
            } => metadata.modified().ok().and_then(local_ym_of),
            Mode::Format | Mode::Size => None,
        };

        match classify_entry(&name, target, mode, &ModeMetadata { size, date_ym }) {
            Destination::Category(category) => {
                category_files(
                    category.clone(),
                    PlannedFile {
                        name: name.clone(),
                        size,
                        modified_unix: modified_seconds(&metadata),
                        date_ym,
                        category,
                    },
                );
            }
            Destination::LeaveInPlace => left_in_place += 1,
            Destination::Skip(reason) => skipped.push(SkippedFile {
                name,
                reason: reason.text().to_string(),
            }),
        }
        checked += 1;
        progress(checked);
    }

    Ok(ScanPlan {
        target,
        root: root.to_path_buf(),
        on_onedrive: on_onedrive(root),
        mode_id: mode_id(mode),
        categories,
        skipped,
        left_in_place,
    })
}

/// Resolves Desktop or Downloads through the Windows known-folder API, so
/// redirected folders (OneDrive, other drives) are handled by Windows.
#[cfg(windows)]
pub fn known_folder(target: OrganizeTarget) -> Result<PathBuf, String> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_Desktop, FOLDERID_Downloads, SHGetKnownFolderPath,
    };
    use windows_sys::core::PWSTR;

    let folder_id = match target {
        OrganizeTarget::Desktop => &FOLDERID_Desktop,
        OrganizeTarget::Downloads => &FOLDERID_Downloads,
        OrganizeTarget::Custom => {
            return Err("The Custom folder is chosen in the app, not a known folder.".into());
        }
    };
    unsafe {
        let mut path: PWSTR = std::ptr::null_mut();
        let result = SHGetKnownFolderPath(folder_id, 0, std::ptr::null_mut(), &mut path);
        if result != 0 {
            return Err(format!(
                "Couldn't find the {target:?} folder (Windows error 0x{result:x})."
            ));
        }
        if path.is_null() {
            return Err(format!(
                "Couldn't find the {target:?} folder: Windows returned none."
            ));
        }
        let mut length = 0_usize;
        while *path.add(length) != 0 {
            length += 1;
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(path, length));
        CoTaskMemFree(path.cast());
        Ok(PathBuf::from(text))
    }
}

/// The Desktop tab's folder (always Format-style three-category logic).
pub fn resolve_desktop_root() -> Result<PathBuf, String> {
    #[cfg(windows)]
    return known_folder(OrganizeTarget::Desktop);
    #[cfg(not(windows))]
    return Err("ERX Tools System Manager needs Windows.".into());
}

/// Case-insensitive comparison text for "is this the same folder".
fn canonical_text(path: &Path) -> String {
    path.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase()
}

/// Resolves the Folder tab's chosen folder: refuses the user's Desktop
/// folder (that is what the Desktop tab is for), and classifies the result
/// as the Downloads target (same trust level as Format mode there — chosen
/// via the quick button or by picking that folder) or the Custom target.
pub fn resolve_folder_tab_root(
    custom_path: Option<&str>,
) -> Result<(PathBuf, OrganizeTarget), String> {
    let raw = custom_path.ok_or_else(|| "Please choose a folder first.".to_string())?;
    let chosen = PathBuf::from(raw);
    if !chosen.is_dir() {
        return Err("That folder could not be found. Please choose a normal folder.".into());
    }
    // Refuse system locations (drive roots, Windows, Program Files,
    // ProgramData, profile root, AppData) with one plain message.
    if is_protected(&chosen, &super::protected::machine_protected_roots()) {
        return Err("This folder is protected. Please choose a normal folder.".into());
    }
    #[cfg(windows)]
    {
        let chosen_text = canonical_text(&chosen);
        if let Ok(desktop) = known_folder(OrganizeTarget::Desktop)
            && canonical_text(&desktop) == chosen_text
        {
            return Err("This is your Desktop. Use the Desktop tab for it.".into());
        }
        if let Ok(downloads) = known_folder(OrganizeTarget::Downloads)
            && canonical_text(&downloads) == chosen_text
        {
            return Ok((chosen, OrganizeTarget::Downloads));
        }
    }
    Ok((chosen, OrganizeTarget::Custom))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "erx-organize-scan-{}-{}-{}",
            std::process::id(),
            tag,
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn no_progress(_checked: u32) {}

    fn category_files(plan: &ScanPlan, name: &str) -> Vec<String> {
        plan.categories
            .iter()
            .find(|category| category.name == name)
            .map(|category| {
                category
                    .files
                    .iter()
                    .map(|file| file.name.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn scan_files_of_every_kind_format_mode() {
        let dir = temp_dir("kinds");
        fs::write(dir.join("report.pdf"), b"pdf").expect("write");
        fs::write(dir.join("photo.jpg"), b"jpg").expect("write");
        fs::write(dir.join("clip.mp4"), b"mp4").expect("write");
        fs::write(dir.join("song.mp3"), b"mp3").expect("write");
        fs::write(dir.join("pack.zip"), b"zip").expect("write");
        fs::write(dir.join("app.exe"), b"exe").expect("write");
        fs::write(dir.join("strange.xyz"), b"xyz").expect("write");

        let plan = scan_dir(
            &dir,
            OrganizeTarget::Downloads,
            &Mode::Format,
            &mut no_progress,
        )
        .expect("scan");
        assert_eq!(plan.mode_id, "format");
        assert_eq!(plan.total_would_move(), 7);
        assert_eq!(category_files(&plan, "Documents"), vec!["report.pdf"]);
        assert_eq!(category_files(&plan, "Images"), vec!["photo.jpg"]);
        assert_eq!(category_files(&plan, "Videos"), vec!["clip.mp4"]);
        assert_eq!(category_files(&plan, "Music"), vec!["song.mp3"]);
        assert_eq!(category_files(&plan, "Archives"), vec!["pack.zip"]);
        assert_eq!(category_files(&plan, "Installers"), vec!["app.exe"]);
        assert_eq!(category_files(&plan, "More"), vec!["strange.xyz"]);
        assert!(plan.skipped.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scan_ignores_subfolders_and_skips_hidden_partial() {
        let dir = temp_dir("hidden");
        fs::create_dir(dir.join("Subfolder")).expect("mkdir");
        // A file INSIDE the subfolder: the scan is top level only.
        fs::write(
            dir.join("Subfolder").join("inside.txt"),
            b"hidden from scan",
        )
        .expect("write");

        // A hidden file (best effort: the attribute is set on Windows).
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .attributes(0x2) // FILE_ATTRIBUTE_HIDDEN
                .open(dir.join("secret.txt"))
                .and_then(|file| file.sync_all())
                .expect("create hidden file");
        }
        fs::write(dir.join("still.crdownload"), b"partial").expect("write");

        let plan = scan_dir(
            &dir,
            OrganizeTarget::Desktop,
            &Mode::Format,
            &mut no_progress,
        )
        .expect("scan");
        assert_eq!(
            plan.total_would_move(),
            0,
            "the subfolder and its files never appear"
        );
        let reasons: Vec<&str> = plan
            .skipped
            .iter()
            .map(|file| file.reason.as_str())
            .collect();
        assert!(reasons.contains(&"still downloading"), "{reasons:?}");
        #[cfg(windows)]
        assert!(reasons.contains(&"hidden or system file"), "{reasons:?}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scan_date_mode_uses_modified_time_and_progress_counts() {
        let dir = temp_dir("dates");
        // Two known modified dates (whole seconds, local-time tolerant:
        // the assertion accepts either year only around New Year edges,
        // which these dates are far from).
        let file_2019 = dir.join("old.txt");
        fs::write(&file_2019, b"old").expect("write");
        let file_2024 = dir.join("new.txt");
        fs::write(&file_2024, b"new").expect("write");
        // 2019-06-15 12:00:00 UTC and 2024-03-01 12:00:00 UTC as system times.
        let time_2019 = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_560_590_400);
        let time_2024 = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_709_294_400);
        file_2019
            .metadata()
            .and_then(|_| {
                std::fs::File::options()
                    .write(true)
                    .open(&file_2019)
                    .and_then(|file| {
                        file.set_times(
                            std::fs::FileTimes::new()
                                .set_modified(time_2019)
                                .set_accessed(time_2019),
                        )
                    })
            })
            .expect("set 2019 modified time");
        std::fs::File::options()
            .write(true)
            .open(&file_2024)
            .and_then(|file| {
                file.set_times(
                    std::fs::FileTimes::new()
                        .set_modified(time_2024)
                        .set_accessed(time_2024),
                )
            })
            .expect("set 2024 modified time");

        let mut seen: Vec<u32> = Vec::new();
        let plan = scan_dir(
            &dir,
            OrganizeTarget::Custom,
            &Mode::Date {
                source: DateSource::Modified,
                granularity: super::super::modes::Granularity::Year,
            },
            &mut |checked| seen.push(checked),
        )
        .expect("scan");
        assert_eq!(plan.mode_id, "date");
        assert_eq!(seen.last(), Some(&2), "progress reported for every file");
        assert_eq!(category_files(&plan, "2019"), vec!["old.txt"]);
        assert_eq!(category_files(&plan, "2024"), vec!["new.txt"]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scan_photo_taken_mode_sends_non_photos_to_date_not_found() {
        let dir = temp_dir("photo-taken");
        fs::write(dir.join("notes.txt"), b"text").expect("write");
        let plan = scan_dir(
            &dir,
            OrganizeTarget::Custom,
            &Mode::Date {
                source: DateSource::PhotoTaken,
                granularity: super::super::modes::Granularity::Year,
            },
            &mut no_progress,
        )
        .expect("scan");
        // A .txt has no EXIF: "Date not found".
        assert_eq!(category_files(&plan, "Date not found"), vec!["notes.txt"]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scan_size_mode_buckets_real_files() {
        let dir = temp_dir("sizes");
        fs::write(dir.join("tiny.txt"), vec![0u8; 10]).expect("write");
        fs::write(dir.join("small.bin"), vec![0u8; 2_000_000]).expect("write");
        let plan =
            scan_dir(&dir, OrganizeTarget::Custom, &Mode::Size, &mut no_progress).expect("scan");
        assert_eq!(plan.mode_id, "size");
        assert_eq!(category_files(&plan, "Under 1 MB"), vec!["tiny.txt"]);
        assert_eq!(category_files(&plan, "1-10 MB"), vec!["small.bin"]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn on_onedrive_by_path_segment() {
        // A path with a OneDrive segment is detected without any env vars.
        assert!(on_onedrive(Path::new("C:\\Users\\x\\OneDrive\\Desktop")));
        // OneDrive for Business folders are literally "OneDrive - Company".
        assert!(on_onedrive(Path::new(
            "C:\\Users\\x\\onedrive - Company\\Desktop"
        )));
        assert!(!on_onedrive(Path::new("C:\\Users\\x\\Documents")));
        // A folder literally named "OneDriveBackup" is not OneDrive.
        assert!(!on_onedrive(Path::new("C:\\Users\\x\\OneDriveBackup")));
    }

    #[test]
    fn folder_tab_root_refusals() {
        // Protected locations still refused.
        let error = resolve_folder_tab_root(Some("C:\\Windows")).expect_err("must refuse");
        assert!(error.contains("protected"), "{error}");
        let error = resolve_folder_tab_root(Some("C:\\")).expect_err("must refuse");
        assert!(error.contains("protected"), "{error}");
        let error =
            resolve_folder_tab_root(Some("Z:\\nonexistent-erx-test")).expect_err("must refuse");
        assert!(error.contains("could not be found"), "{error}");
        assert!(
            resolve_folder_tab_root(None).is_err(),
            "no path chosen is an error"
        );
        // The Desktop folder is refused with a pointer to the Desktop tab.
        #[cfg(windows)]
        if let Ok(desktop) = known_folder(OrganizeTarget::Desktop) {
            let error = resolve_folder_tab_root(Some(&desktop.to_string_lossy()))
                .expect_err("the Desktop must be refused in the Folder tab");
            assert!(error.contains("Desktop tab"), "{error}");
        }
    }

    #[test]
    fn folder_tab_classifies_downloads_folder() {
        // A non-Downloads folder resolves to the Custom target. The scratch
        // folder must NOT live under the temp dir: it is inside AppData,
        // which the protected-locations rule refuses (per spec).
        let dir = std::env::current_dir()
            .expect("cwd")
            .join("target")
            .join(format!("erx-folder-tab-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("mkdir");
        let (root, target) =
            resolve_folder_tab_root(Some(&dir.to_string_lossy())).expect("resolve");
        assert_eq!(target, OrganizeTarget::Custom);
        assert_eq!(root, dir);
        fs::remove_dir_all(&dir).ok();
        // …and the real Downloads folder (if resolvable) to the Downloads target.
        #[cfg(windows)]
        if let Ok(downloads) = known_folder(OrganizeTarget::Downloads) {
            let (_, target) = resolve_folder_tab_root(Some(&downloads.to_string_lossy()))
                .expect("resolve downloads");
            assert_eq!(target, OrganizeTarget::Downloads);
        }
    }
}
