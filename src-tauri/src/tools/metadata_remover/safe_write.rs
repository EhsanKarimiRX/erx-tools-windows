//! Helpers for writing output files safely: temporary names, cleaning up
//! stale temporary files, a move that NEVER replaces an existing file, and
//! the output file names (nameERX.ext, nameERX (1).ext, ...).
//!
//! Why not `std::fs::rename`: on Windows it replaces an existing target.
//! `move_no_replace` calls MoveFileExW without MOVEFILE_REPLACE_EXISTING, so
//! Windows itself refuses an existing target, atomically.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::tools::image_convert::naming::sanitize_stem;

/// Extension of temporary files.
pub const TEMP_EXTENSION: &str = "erxtmp";
/// Temporary files older than this are leftovers of a crashed run.
pub const STALE_TEMP_AGE: Duration = Duration::from_secs(3600);
/// Suffix added to every output stem, before the extension.
const OUTPUT_SUFFIX: &str = "ERX";
/// How many numbered names are tried before giving up.
const MAX_NAME_TRIES: u32 = 10_000;

/// Eight random hex characters that make temporary names unique.
pub fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes)
        .map_err(|_| "could not get random data from Windows".to_string())?;
    Ok(format!("{:08x}", u32::from_le_bytes(bytes)))
}

/// `<final path>.<token>.erxtmp`
pub fn temp_path(final_path: &Path, token: &str) -> PathBuf {
    let mut name = final_path.as_os_str().to_os_string();
    name.push(format!(".{token}.{TEMP_EXTENSION}"));
    PathBuf::from(name)
}

/// True for names made by `temp_path`: `<name>.<8 hex>.erxtmp`.
pub fn is_temp_name(name: &str) -> bool {
    let suffix = format!(".{TEMP_EXTENSION}");
    let Some(rest) = name.strip_suffix(suffix.as_str()) else {
        return false;
    };
    let Some((stem, token)) = rest.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty() && token.len() == 8 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Deletes temporary files left behind by a crashed run: regular files with
/// our temporary name that were last changed at least `min_age` ago.
/// Returns how many were removed.
pub fn cleanup_stale_temp(dir: &Path, min_age: Duration) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let now = SystemTime::now();
    let mut removed = 0;
    for entry in entries.flatten() {
        let os_name = entry.file_name();
        let Some(name) = os_name.to_str() else {
            continue;
        };
        if !is_temp_name(name) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let age = metadata
            .modified()
            .ok()
            .map(|changed| now.duration_since(changed).unwrap_or_default());
        if age.is_some_and(|value| value >= min_age) && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Moves `from` to `to`. If `to` already exists the move FAILS with an error
/// of kind `AlreadyExists`; an existing file is never replaced.
pub fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

    let wide = |path: &Path| -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let from_wide = wide(from);
    let to_wide = wide(to);
    // SAFETY: both buffers are valid NUL-terminated UTF-16 strings that live
    // until the call returns; flags 0 means "do not replace an existing file".
    let ok = unsafe { MoveFileExW(from_wide.as_ptr(), to_wide.as_ptr(), 0) };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// The output path for number `number`: `stemERX.ext` for 0, then
/// `stemERX (1).ext`, `stemERX (2).ext`, ... The extension is the one of the
/// source file; the stem is sanitized.
pub fn candidate_path(source: &Path, out_folder: Option<&Path>, number: u32) -> PathBuf {
    let folder = match out_folder {
        Some(folder) => folder.to_path_buf(),
        None => match source.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
            _ => PathBuf::from("."),
        },
    };
    let raw_stem = source
        .file_stem()
        .map_or_else(|| "file".to_string(), |s| s.to_string_lossy().into_owned());
    let mut name = format!("{}{OUTPUT_SUFFIX}", sanitize_stem(&raw_stem));
    if number > 0 {
        name.push_str(&format!(" ({number})"));
    }
    if let Some(extension) = source.extension() {
        name.push('.');
        name.push_str(&extension.to_string_lossy());
    }
    folder.join(name)
}

/// The first output path that does not exist yet. `exists` is injected so
/// tests can simulate collisions. This is only a fast pre-check; the real
/// protection is `move_no_replace`.
pub fn choose_output_path(
    source: &Path,
    out_folder: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    for number in 0..=MAX_NAME_TRIES {
        let candidate = candidate_path(source, out_folder, number);
        if !exists(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("erx-mr-safe-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn output_names_get_the_erx_suffix_and_keep_the_extension() {
        assert_eq!(
            candidate_path(Path::new("C:/pics/photo.jpg"), None, 0),
            PathBuf::from("C:/pics/photoERX.jpg")
        );
        assert_eq!(
            candidate_path(Path::new("C:/pics/photo.JPG"), None, 0),
            PathBuf::from("C:/pics/photoERX.JPG")
        );
        assert_eq!(
            candidate_path(Path::new("C:/pics/photo.jpg"), None, 2),
            PathBuf::from("C:/pics/photoERX (2).jpg")
        );
        assert_eq!(
            candidate_path(Path::new("C:/pics/photo"), None, 0),
            PathBuf::from("C:/pics/photoERX")
        );
        assert_eq!(
            candidate_path(Path::new("C:/pics/photo.jpg"), Some(Path::new("D:/out")), 0),
            PathBuf::from("D:/out/photoERX.jpg")
        );
    }

    #[test]
    fn dirty_and_persian_stems_are_handled() {
        let dirty = candidate_path(Path::new("C:/pics/img<1>.png"), None, 0);
        assert!(dirty.to_str().unwrap().ends_with("img_1_ERX.png"));
        let persian = candidate_path(Path::new("C:/pics/\u{0639}\u{06A9}\u{0633}.jpg"), None, 0);
        assert!(
            persian
                .to_str()
                .unwrap()
                .ends_with("\u{0639}\u{06A9}\u{0633}ERX.jpg")
        );
    }

    #[test]
    fn taken_names_count_upwards() {
        let taken = [
            PathBuf::from("C:/pics/photoERX.jpg"),
            PathBuf::from("C:/pics/photoERX (1).jpg"),
        ];
        let chosen = choose_output_path(Path::new("C:/pics/photo.jpg"), None, |path| {
            taken.iter().any(|candidate| candidate.as_path() == path)
        });
        assert_eq!(chosen, Some(PathBuf::from("C:/pics/photoERX (2).jpg")));

        let none = choose_output_path(Path::new("C:/pics/photo.jpg"), None, |_| true);
        assert_eq!(none, None);
    }

    #[test]
    fn temp_names_are_recognized() {
        assert_eq!(
            temp_path(Path::new("C:/out/a.jpg"), "0123abcd"),
            PathBuf::from("C:/out/a.jpg.0123abcd.erxtmp")
        );
        assert!(is_temp_name("photoERX.jpg.1a2b3c4d.erxtmp"));
        assert!(is_temp_name("x.1a2b3c4d.erxtmp"));
        assert!(!is_temp_name("a.erxtmp"));
        assert!(!is_temp_name("x.1a2b3c4g.erxtmp"));
        assert!(!is_temp_name(".1a2b3c4d.erxtmp"));
        assert!(!is_temp_name("photo.jpg"));
        assert!(!is_temp_name("x.1a2b3c4d.erxtmpx"));
    }

    #[test]
    fn random_tokens_look_right() {
        let token = random_token().unwrap();
        assert_eq!(token.len(), 8);
        assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn stale_temp_files_are_cleaned_but_nothing_else() {
        let dir = temp_dir("cleanup");
        let stale = dir.join("a.jpg.0123abcd.erxtmp");
        let normal = dir.join("keep.txt");
        fs::write(&stale, b"x").unwrap();
        fs::write(&normal, b"y").unwrap();

        // Too young: nothing is removed.
        assert_eq!(cleanup_stale_temp(&dir, STALE_TEMP_AGE), 0);
        assert!(stale.exists());

        // Old enough (age limit zero): only the temporary file goes.
        assert_eq!(cleanup_stale_temp(&dir, Duration::ZERO), 1);
        assert!(!stale.exists());
        assert!(normal.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn moving_never_replaces_an_existing_file() {
        let dir = temp_dir("move");
        let first = dir.join("a.tmp");
        let second = dir.join("b.tmp");
        let target = dir.join("c.jpg");
        fs::write(&first, b"A").unwrap();
        fs::write(&second, b"B").unwrap();

        move_no_replace(&first, &target).unwrap();
        assert!(!first.exists());
        assert_eq!(fs::read(&target).unwrap(), b"A");

        let error = move_no_replace(&second, &target).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&target).unwrap(), b"A");
        assert_eq!(fs::read(&second).unwrap(), b"B");
        fs::remove_dir_all(&dir).unwrap();
    }
}
