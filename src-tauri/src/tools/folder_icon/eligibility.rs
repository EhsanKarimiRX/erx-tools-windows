//! Folder eligibility — the ONE data-driven place that decides whether a
//! folder may receive an icon (spec 4.3). Pure logic, unit tested with
//! injected paths and environment facts so no test depends on the real
//! machine's layout.
//!
//! The Windows-specific fact gathering (known-folder API, drive types,
//! reparse attributes) lives in `platform.rs` and feeds this module.
//!
//! Result model:
//! - `Allowed` — apply is safe.
//! - `AllowedWithWarning` — apply is allowed but the user should know
//!   something (cloud sync may drop or reset the icon).
//! - `Blocked(reason)` — the UI shows "Icon change is not available for
//!   this folder." plus the reason's short English line.
//!
//! Inputs must be RESOLVED first: the caller canonicalizes, strips the
//! `\\?\` prefix and compares case-insensitively (see `strip_verbatim` and
//! the component-wise compare helpers).

// Temporary: the verdict types are consumed by the `folder_icon_inspect`
// command landing in 4B-4 (same pattern as error.rs). Remove then.
#![allow(dead_code)]

use serde::Serialize;
use std::path::{Path, PathBuf};

/// Why a folder is blocked. The message is the small secondary line under
/// the main "Icon change is not available for this folder." text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockReason {
    /// Files, missing paths (a .zip file is a file to the Shell).
    NotADirectory,
    /// D:\ — the folder IS a drive.
    DriveRoot,
    /// Windows, Program Files, $Recycle.Bin & friends (and everything
    /// under them), or the ProgramData/Users roots themselves.
    ProtectedSystemLocation,
    /// The user profile root and the known user folders themselves
    /// (Desktop, Documents, …, AppData levels, OneDrive root), including
    /// redirected locations. Their SUBFOLDERS stay allowed.
    KnownUserFolder,
    /// Symlinks, junctions and mount points as the dropped folder.
    ReparsePoint,
    /// UNC paths, mapped network drives, WebDAV.
    NetworkLocation,
    /// The icon file path would not fit the Shell's MAX_PATH limit.
    PathTooLong,
}

impl BlockReason {
    /// The secondary reason line shown in the UI.
    pub fn message(self) -> &'static str {
        match self {
            BlockReason::NotADirectory => {
                "That is not a folder or a shortcut (or it does not exist)."
            }
            BlockReason::DriveRoot => "Drive roots cannot carry folder icons.",
            BlockReason::ProtectedSystemLocation => "This is a protected Windows location.",
            BlockReason::KnownUserFolder => "Windows manages this folder's desktop.ini itself.",
            BlockReason::ReparsePoint => {
                "This folder is a link (symlink or junction); the target is ambiguous."
            }
            BlockReason::NetworkLocation => {
                "Windows does not reliably honor desktop.ini on network locations."
            }
            BlockReason::PathTooLong => "The folder path is too long for the icon file.",
        }
    }
}

/// Why an eligible folder still carries a non-blocking warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningCode {
    /// Inside OneDrive (or another detected cloud-sync root): sync clients
    /// may skip or reset hidden system files such as desktop.ini.
    CloudSyncRoot,
}

impl WarningCode {
    /// The non-blocking warning line shown in the UI.
    pub fn message(self) -> &'static str {
        match self {
            WarningCode::CloudSyncRoot => {
                "This folder is inside a cloud-sync location. The sync client may not keep the icon."
            }
        }
    }
}

/// The verdict for one folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum Eligibility {
    Allowed,
    AllowedWithWarning(WarningCode),
    Blocked(BlockReason),
}

/// The filesystem facts about one resolved path, gathered by the platform
/// layer and injected in tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathFacts {
    pub exists: bool,
    pub is_dir: bool,
    /// Symlink, junction or mount point (FILE_ATTRIBUTE_REPARSE_POINT).
    pub is_reparse_point: bool,
}

/// The machine environment the rules compare against: locations from the
/// Windows known-folder API (never hardcoded) plus remote drives.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    /// Known user folders blocked THEMSELVES (subfolders stay allowed):
    /// profile root, Desktop, Documents, …, AppData levels, OneDrive root.
    pub known_folders: Vec<PathBuf>,
    /// System locations blocked themselves AND everything under them:
    /// the Windows directory, Program Files, Program Files (x86).
    pub system_under: Vec<PathBuf>,
    /// System roots blocked ONLY as the root (subfolders stay allowed):
    /// the ProgramData root, the Users root.
    pub system_root_only: Vec<PathBuf>,
    /// Cloud-sync roots (OneDrive and friends); everything inside warns.
    pub cloud_roots: Vec<PathBuf>,
    /// Drive letters reported remote by GetDriveTypeW, e.g. "E:".
    pub remote_drives: Vec<String>,
}

/// The icon file name the apply step writes INSIDE the target folder:
/// `erx-folder-icon-` + 8 hex digits + `.ico` (content-derived hash).
pub const ICON_FILE_NAME_LEN: usize = "erx-folder-icon-12345678.ico".len();

/// Safe maximum for the FULL icon file path in UTF-16 code units:
/// MAX_PATH (260) minus the terminating NUL. Explorer parses desktop.ini
/// `IconResource` without long-path awareness, so the whole icon path
/// (folder + separator + our 28-char file name) must fit this limit.
pub const MAX_ICON_FILE_PATH: usize = 259;

/// System folder names that live on a DRIVE ROOT and are protected
/// themselves and everything under them (matched case-insensitively as the
/// first folder after the root). No known-folder IDs exist for these; the
/// names are fixed by Windows on every drive.
pub const DRIVE_ROOT_SYSTEM_NAMES: [&str; 8] = [
    "$Recycle.Bin",
    "System Volume Information",
    "Recovery",
    "Boot",
    "Config.Msi",
    "MSOCache",
    "PerfLogs",
    "Documents and Settings",
];

/// Evaluates one resolved folder. The order is deliberate: cheap
/// structural checks first, then location rules, then the reparse and
/// warning checks.
pub fn evaluate(path: &Path, facts: PathFacts, env: &Environment) -> Eligibility {
    // 0. Shortcut files (.lnk) have their own, smaller rule set.
    if facts.exists && !facts.is_dir && is_shortcut_file(path) {
        return evaluate_shortcut(path, env);
    }

    // 1. Must be an existing directory.
    if !facts.exists || !facts.is_dir {
        return Eligibility::Blocked(BlockReason::NotADirectory);
    }

    // 2. Network locations: UNC paths and WebDAV (both UNC forms) first,
    //    then mapped drives reported remote by GetDriveTypeW.
    if is_unc(path) || is_on_remote_drive(path, env) {
        return Eligibility::Blocked(BlockReason::NetworkLocation);
    }

    // 3. Drive roots (C:\, C:, D:\ — with network roots already handled).
    if is_drive_root(path) {
        return Eligibility::Blocked(BlockReason::DriveRoot);
    }

    // 4. The icon file path must fit the Shell's MAX_PATH.
    if utf16_len(path) + 1 + ICON_FILE_NAME_LEN > MAX_ICON_FILE_PATH {
        return Eligibility::Blocked(BlockReason::PathTooLong);
    }

    // 5. Protected system locations, everything under them.
    if env
        .system_under
        .iter()
        .any(|root| same_or_under_ci(path, root))
    {
        return Eligibility::Blocked(BlockReason::ProtectedSystemLocation);
    }

    // 6. The fixed drive-root system names ($Recycle.Bin & friends),
    //    themselves and everything under them.
    if is_under_drive_root_system_name(path) {
        return Eligibility::Blocked(BlockReason::ProtectedSystemLocation);
    }

    // 7. System roots that block only themselves (ProgramData, Users).
    if env
        .system_root_only
        .iter()
        .any(|root| same_path_ci(path, root))
    {
        return Eligibility::Blocked(BlockReason::ProtectedSystemLocation);
    }

    // 8. Known user folders block only themselves (subfolders allowed).
    if env
        .known_folders
        .iter()
        .any(|root| same_path_ci(path, root))
    {
        return Eligibility::Blocked(BlockReason::KnownUserFolder);
    }

    // 9. Links as the dropped folder are ambiguous.
    if facts.is_reparse_point {
        return Eligibility::Blocked(BlockReason::ReparsePoint);
    }

    // 10. Cloud-sync locations warn but stay allowed.
    if env
        .cloud_roots
        .iter()
        .any(|root| same_or_under_ci(path, root))
    {
        return Eligibility::AllowedWithWarning(WarningCode::CloudSyncRoot);
    }

    Eligibility::Allowed
}

/// True for a shortcut file (.lnk), judged by the extension only.
pub fn is_shortcut_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
}

/// Rules for a shortcut FILE. Known user folders such as the Desktop are
/// protected only as folders, so a shortcut directly on the Desktop is fine.
/// Protected system locations and network locations stay blocked; cloud-sync
/// locations warn (the icon file lives on this PC only).
fn evaluate_shortcut(path: &Path, env: &Environment) -> Eligibility {
    if is_unc(path) || is_on_remote_drive(path, env) {
        return Eligibility::Blocked(BlockReason::NetworkLocation);
    }
    if env
        .system_under
        .iter()
        .any(|root| same_or_under_ci(path, root))
        || is_under_drive_root_system_name(path)
    {
        return Eligibility::Blocked(BlockReason::ProtectedSystemLocation);
    }
    if env
        .cloud_roots
        .iter()
        .any(|root| same_or_under_ci(path, root))
    {
        return Eligibility::AllowedWithWarning(WarningCode::CloudSyncRoot);
    }
    Eligibility::Allowed
}

/// Resolves a path for the rules: canonicalizes (which also collapses
/// `..`), strips the `\\?\` verbatim prefix, and falls back to the given
/// path when it does not exist yet (the rules report it as missing).
pub fn resolve(path: &Path) -> PathBuf {
    let canonical = std::fs::canonicalize(path).map(|resolved| strip_verbatim(&resolved));
    canonical.unwrap_or_else(|_| strip_verbatim(path))
}

/// Strips the `\\?\` (and `\\?\UNC\`) prefix that canonicalize prepends.
pub fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.as_os_str().to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        PathBuf::from(text.into_owned())
    }
}

/// True when the path is a UNC path (\\server\share\…), which covers
/// WebDAV's \\server@SSL\DavWWWRoot form too.
fn is_unc(path: &Path) -> bool {
    matches!(
        path.components().next(),
        Some(std::path::Component::Prefix(prefix))
            if matches!(
                prefix.kind(),
                std::path::Prefix::UNC(_, _) | std::path::Prefix::VerbatimUNC(_, _)
            )
    )
}

/// True when the path's drive letter is reported remote in the environment.
fn is_on_remote_drive(path: &Path, env: &Environment) -> bool {
    let text = path.as_os_str().to_string_lossy();
    let Some((letter, rest)) = text.split_once(':') else {
        return false;
    };
    if rest.contains(':') || !letter.chars().all(|c| c.is_ascii_alphabetic()) || letter.is_empty() {
        return false;
    }
    let drive = format!("{}:", letter.to_ascii_uppercase());
    env.remote_drives
        .iter()
        .any(|remote| remote.eq_ignore_ascii_case(&drive))
}

/// True for "C:\", "C:" and "D:\" — a prefix with at most the root
/// directory and no real folder component.
fn is_drive_root(path: &Path) -> bool {
    let mut components = path.components();
    let Some(first) = components.next() else {
        return false;
    };
    let has_prefix = matches!(first, std::path::Component::Prefix(_));
    if !has_prefix {
        return false;
    }
    match components.next() {
        None => true,                                                       // "C:"
        Some(std::path::Component::RootDir) => components.next().is_none(), // "C:\"
        _ => false,
    }
}

/// UTF-16 length — the unit Windows' MAX_PATH counts.
fn utf16_len(path: &Path) -> usize {
    path.as_os_str().to_string_lossy().encode_utf16().count()
}

/// Case-insensitive component-wise comparison (Windows file systems are
/// case-insensitive; the names compared here are ASCII).
fn same_path_ci(path: &Path, root: &Path) -> bool {
    let a: Vec<_> = path.components().map(string_of).collect();
    let b: Vec<_> = root.components().map(string_of).collect();
    a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| x.eq_ignore_ascii_case(y))
}

/// Case-insensitive "path is the root or somewhere under it".
fn same_or_under_ci(path: &Path, root: &Path) -> bool {
    let a: Vec<_> = path.components().map(string_of).collect();
    let b: Vec<_> = root.components().map(string_of).collect();
    a.len() >= b.len() && a.iter().zip(&b).all(|(x, y)| x.eq_ignore_ascii_case(y))
}

fn string_of(component: std::path::Component) -> String {
    component.as_os_str().to_string_lossy().into_owned()
}

/// True when the path's first folder directly on a drive root matches one
/// of the fixed protected system names (so the folder and everything under
/// it is protected). "C:\Users\$Recycle.Bin" does NOT match — only paths
/// directly on the root do.
fn is_under_drive_root_system_name(path: &Path) -> bool {
    let mut components = path.components();
    let Some(std::path::Component::Prefix(_)) = components.next() else {
        return false;
    };
    if !matches!(components.next(), Some(std::path::Component::RootDir)) {
        return false;
    }
    let Some(first) = components.next() else {
        return false;
    };
    let name = first.as_os_str().to_string_lossy();
    DRIVE_ROOT_SYSTEM_NAMES
        .iter()
        .any(|system| name.eq_ignore_ascii_case(system))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A typical test environment, modeled on a real Windows machine but
    /// with invented paths (no test depends on the real machine).
    fn env() -> Environment {
        let known: Vec<PathBuf> = [
            r"C:\Users\Ehsan",
            r"C:\Users\Ehsan\Desktop",
            r"C:\Users\Ehsan\Documents",
            r"C:\Users\Ehsan\Downloads",
            r"C:\Users\Ehsan\Music",
            r"C:\Users\Ehsan\Pictures",
            r"C:\Users\Ehsan\Videos",
            r"C:\Users\Ehsan\Favorites",
            r"C:\Users\Ehsan\Links",
            r"C:\Users\Ehsan\Saved Games",
            r"C:\Users\Ehsan\Searches",
            r"C:\Users\Ehsan\Contacts",
            r"C:\Users\Ehsan\AppData",
            r"C:\Users\Ehsan\AppData\Roaming",
            r"C:\Users\Ehsan\AppData\Local",
            r"C:\Users\Ehsan\OneDrive",
            // A REDIRECTED Documents folder (moved to another drive).
            r"D:\MyDocuments",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        Environment {
            known_folders: known,
            system_under: vec![
                PathBuf::from(r"C:\Windows"),
                PathBuf::from(r"C:\Program Files"),
                PathBuf::from(r"C:\Program Files (x86)"),
            ],
            system_root_only: vec![PathBuf::from(r"C:\ProgramData"), PathBuf::from(r"C:\Users")],
            cloud_roots: vec![PathBuf::from(r"C:\Users\Ehsan\OneDrive")],
            remote_drives: vec!["N:".to_string()],
        }
    }

    fn dir(exists: bool) -> PathFacts {
        PathFacts {
            exists,
            is_dir: exists,
            is_reparse_point: false,
        }
    }

    fn allowed(path: &str) {
        assert_eq!(
            evaluate(Path::new(path), dir(true), &env()),
            Eligibility::Allowed,
            "expected {path} to be allowed"
        );
    }

    fn blocked(path: &str, reason: BlockReason) {
        assert_eq!(
            evaluate(Path::new(path), dir(true), &env()),
            Eligibility::Blocked(reason),
            "expected {path} to be blocked"
        );
    }

    fn file(exists: bool) -> PathFacts {
        PathFacts {
            exists,
            is_dir: false,
            is_reparse_point: false,
        }
    }

    #[test]
    fn shortcut_files_follow_their_own_rules() {
        let check = |path: &str| evaluate(Path::new(path), file(true), &env());
        assert_eq!(
            check(r"C:\Users\Ehsan\Desktop\App.lnk"),
            Eligibility::Allowed
        );
        assert_eq!(check(r"D:\tools\App.LNK"), Eligibility::Allowed);
        assert_eq!(
            check(r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\App.lnk"),
            Eligibility::Allowed
        );
        assert_eq!(
            check(r"C:\Windows\Temp\App.lnk"),
            Eligibility::Blocked(BlockReason::ProtectedSystemLocation)
        );
        assert_eq!(
            check(r"C:\Program Files\Tool\App.lnk"),
            Eligibility::Blocked(BlockReason::ProtectedSystemLocation)
        );
        assert_eq!(
            check(r"\\server\share\App.lnk"),
            Eligibility::Blocked(BlockReason::NetworkLocation)
        );
        assert_eq!(
            check(r"N:\work\App.lnk"),
            Eligibility::Blocked(BlockReason::NetworkLocation)
        );
        assert_eq!(
            check(r"C:\Users\Ehsan\OneDrive\Desktop\App.lnk"),
            Eligibility::AllowedWithWarning(WarningCode::CloudSyncRoot)
        );
        // Every other kind of file stays blocked.
        for path in [
            r"C:\dev\App.url",
            r"C:\dev\notes.txt",
            r"C:\dev\App.lnk.txt",
        ] {
            assert_eq!(
                check(path),
                Eligibility::Blocked(BlockReason::NotADirectory),
                "{path}"
            );
        }
        // A missing .lnk is blocked like any missing path.
        assert_eq!(
            evaluate(Path::new(r"C:\dev\gone.lnk"), file(false), &env()),
            Eligibility::Blocked(BlockReason::NotADirectory)
        );
    }

    #[test]
    fn ordinary_folders_are_allowed() {
        allowed(r"D:\projects\my-tool");
        allowed(r"C:\dev\test-folders\one");
        allowed(r"C:\Users\Ehsan\Projects\site");
        allowed(r"C:\ProgramData\my-company\tool"); // under a root-only system path
        allowed(r"C:\Users\Ehsan\Desktop\album"); // subfolder of a known folder
    }

    #[test]
    fn non_directories_are_blocked() {
        // Files (a .zip file is a file to the Shell) and missing paths.
        for path in [r"C:\dev\report.zip", r"C:\dev\notes.txt", r"C:\dev\f"] {
            let file = PathFacts {
                exists: true,
                is_dir: false,
                is_reparse_point: false,
            };
            assert_eq!(
                evaluate(Path::new(path), file, &env()),
                Eligibility::Blocked(BlockReason::NotADirectory),
                "{path}"
            );
        }
        // A missing path: exists = false.
        assert_eq!(
            evaluate(Path::new(r"C:\dev\missing"), dir(false), &env()),
            Eligibility::Blocked(BlockReason::NotADirectory)
        );
    }

    #[test]
    fn drive_roots_are_blocked() {
        blocked(r"C:\", BlockReason::DriveRoot);
        blocked(r"D:\", BlockReason::DriveRoot);
        blocked(r"C:", BlockReason::DriveRoot);
    }

    #[test]
    fn network_locations_are_blocked() {
        blocked(r"\\server\share\folder", BlockReason::NetworkLocation);
        // WebDAV shape.
        blocked(
            r"\\wwwroot@SSL\DavWWWRoot\docs",
            BlockReason::NetworkLocation,
        );
        // A mapped (remote) drive.
        blocked(r"N:\work\folder", BlockReason::NetworkLocation);
        blocked(r"N:\", BlockReason::NetworkLocation);
        // A local path whose letters merely contain a remote drive's letter.
        allowed(r"C:\N-drive-like\folder");
    }

    #[test]
    fn system_locations_are_blocked() {
        blocked(r"C:\Windows", BlockReason::ProtectedSystemLocation);
        blocked(
            r"C:\Windows\System32\drivers",
            BlockReason::ProtectedSystemLocation,
        );
        // Case-insensitive matching (trailing dots are resolved away by
        // resolve()'s canonicalize before the rules see the path).
        blocked(r"c:\wInDoWs\fonts", BlockReason::ProtectedSystemLocation);
        blocked(
            r"C:\Program Files\Some App",
            BlockReason::ProtectedSystemLocation,
        );
        blocked(
            r"C:\Program Files (x86)\Old App\sub",
            BlockReason::ProtectedSystemLocation,
        );
        // Fixed drive-root system names, themselves and everything under.
        blocked(r"C:\$Recycle.Bin", BlockReason::ProtectedSystemLocation);
        blocked(
            r"C:\$recycle.bin\S-1-5-18",
            BlockReason::ProtectedSystemLocation,
        );
        blocked(
            r"D:\System Volume Information",
            BlockReason::ProtectedSystemLocation,
        );
        blocked(r"D:\Recovery\abc", BlockReason::ProtectedSystemLocation);
        blocked(r"E:\PerfLogs", BlockReason::ProtectedSystemLocation);
        blocked(
            r"C:\Documents and Settings",
            BlockReason::ProtectedSystemLocation,
        );
        // Same NAME deeper in the tree is fine.
        allowed(r"C:\dev\Boot");
        allowed(r"C:\Users\Ehsan\Recovery");
        // Root-only system paths: only the root itself.
        blocked(r"C:\ProgramData", BlockReason::ProtectedSystemLocation);
        blocked(r"C:\Users", BlockReason::ProtectedSystemLocation);
    }

    #[test]
    fn known_user_folders_block_themselves_but_not_subfolders() {
        blocked(r"C:\Users\Ehsan", BlockReason::KnownUserFolder);
        blocked(r"C:\Users\Ehsan\Desktop", BlockReason::KnownUserFolder);
        blocked(r"C:\Users\Ehsan\Documents", BlockReason::KnownUserFolder);
        blocked(r"C:\Users\Ehsan\AppData", BlockReason::KnownUserFolder);
        blocked(
            r"C:\Users\Ehsan\AppData\Roaming",
            BlockReason::KnownUserFolder,
        );
        blocked(
            r"C:\Users\Ehsan\AppData\Local",
            BlockReason::KnownUserFolder,
        );
        blocked(r"C:\Users\Ehsan\OneDrive", BlockReason::KnownUserFolder);
        // Redirected locations are covered by the API-provided paths.
        blocked(r"D:\MyDocuments", BlockReason::KnownUserFolder);
        // Case-insensitive and trailing separators.
        blocked(r"c:\users\ehsan\DESKTOP", BlockReason::KnownUserFolder);
        // Subfolders stay allowed.
        allowed(r"C:\Users\Ehsan\Desktop\trip-photos");
        allowed(r"C:\Users\Ehsan\Documents\invoices");
        allowed(r"D:\MyDocuments\scans");
        // A different user's known folder list must not block others: the
        // injected environment only knows Ehsan's folders.
        allowed(r"C:\Users\Other\Desktop");
    }

    #[test]
    fn reparse_points_are_blocked() {
        let link = PathFacts {
            exists: true,
            is_dir: true,
            is_reparse_point: true,
        };
        assert_eq!(
            evaluate(Path::new(r"C:\dev\link"), link, &env()),
            Eligibility::Blocked(BlockReason::ReparsePoint)
        );
    }

    #[test]
    fn cloud_sync_folders_warn_but_stay_allowed() {
        assert_eq!(
            evaluate(
                Path::new(r"C:\Users\Ehsan\OneDrive\work"),
                dir(true),
                &env()
            ),
            Eligibility::AllowedWithWarning(WarningCode::CloudSyncRoot)
        );
        assert_eq!(
            evaluate(
                Path::new(r"C:\Users\Ehsan\OneDrive\work\deep"),
                dir(true),
                &env()
            ),
            Eligibility::AllowedWithWarning(WarningCode::CloudSyncRoot)
        );
    }

    #[test]
    fn paths_too_long_for_the_shell_are_blocked() {
        // The icon file path is folder + '\' + 28 chars and must stay
        // within 259 UTF-16 units.
        let base = r"C:\dev\";
        let fits = format!("{base}{}", "d".repeat(230 - base.len()));
        let too_long = format!("{base}{}", "d".repeat(231 - base.len()));
        assert!(utf16_len(Path::new(&fits)) + 1 + ICON_FILE_NAME_LEN <= MAX_ICON_FILE_PATH);
        assert!(utf16_len(Path::new(&too_long)) + 1 + ICON_FILE_NAME_LEN > MAX_ICON_FILE_PATH);
        allowed(&fits);
        blocked(&too_long, BlockReason::PathTooLong);

        // Non-ASCII names count their UTF-16 units, not bytes.
        let persian = format!("{base}ی{}", "د".repeat(200));
        assert_eq!(utf16_len(Path::new(&persian)), 7 + 1 + 200);
    }

    #[test]
    fn verbatim_prefixes_are_stripped() {
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\C:\dev\test")),
            PathBuf::from(r"C:\dev\test")
        );
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\UNC\server\share\f")),
            PathBuf::from(r"\\server\share\f")
        );
        assert_eq!(
            strip_verbatim(Path::new(r"C:\dev\test")),
            PathBuf::from(r"C:\dev\test")
        );
    }

    #[test]
    fn messages_are_friendly_sentences() {
        for reason in [
            BlockReason::NotADirectory,
            BlockReason::DriveRoot,
            BlockReason::ProtectedSystemLocation,
            BlockReason::KnownUserFolder,
            BlockReason::ReparsePoint,
            BlockReason::NetworkLocation,
            BlockReason::PathTooLong,
        ] {
            let message = reason.message();
            assert!(message.ends_with('.'), "{message}");
            assert!(message.len() > 10, "{message}");
        }
        assert!(WarningCode::CloudSyncRoot.message().ends_with('.'));
    }

    #[test]
    fn check_order_prefers_the_earlier_rule() {
        // A reparse point that would also be a known folder reports the
        // known-folder reason (rule 8 runs before rule 9).
        let link = PathFacts {
            exists: true,
            is_dir: true,
            is_reparse_point: true,
        };
        assert_eq!(
            evaluate(Path::new(r"C:\Users\Ehsan\Desktop"), link, &env()),
            Eligibility::Blocked(BlockReason::KnownUserFolder)
        );
    }
}
