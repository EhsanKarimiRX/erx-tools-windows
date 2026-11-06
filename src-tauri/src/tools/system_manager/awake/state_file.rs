//! The Awake state file (`awake_state.json` in the app data directory).
//!
//! Load is tolerant: unknown JSON fields are ignored and missing optional
//! fields default to null. Garbage (unparsable JSON, wrong types, an unknown
//! version) is an `Err`, never a panic. Save is atomic: the full file is
//! written to a `.tmp` sibling first, then renamed over the real file, so a
//! crash can never leave a half-written state behind.

use std::fs;
use std::path::{Path, PathBuf};

use super::core::AwakeStateFile;

/// Bump when the format changes; unknown versions are rejected, not guessed.
pub const STATE_VERSION: u32 = 1;

/// `<app data dir>/awake_state.json`.
pub fn state_path(state_dir: &Path) -> PathBuf {
    state_dir.join("awake_state.json")
}

/// Reads the state file. `Ok(None)` = no state file (Awake is off).
pub fn load(path: &Path) -> Result<Option<AwakeStateFile>, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Awake could not read its state file: {error}. Please check the folder's permissions."
            ));
        }
    };
    let state: AwakeStateFile = serde_json::from_str(&text).map_err(|error| {
        format!(
            "Awake found a damaged state file it cannot understand ({error}). Turning Awake off and on again will rewrite it."
        )
    })?;
    if state.version != STATE_VERSION {
        return Err(format!(
            "Awake found a state file from a different version ({} instead of {STATE_VERSION}). Please turn Awake off and on again.",
            state.version
        ));
    }
    Ok(Some(state))
}

/// Writes the state file atomically: serialize fully into `awake_state.json.tmp`,
/// flush it to disk, then rename over the real file.
pub fn save(path: &Path, state: &AwakeStateFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Awake could not create its data folder: {error}"))?;
    }
    let text = serde_json::to_string_pretty(state)
        .map_err(|error| format!("Awake could not serialize its state: {error}"))?;
    let tmp = path.with_extension("json.tmp");
    let mut file = fs::File::create(&tmp)
        .map_err(|error| format!("Awake could not write its state file: {error}"))?;
    std::io::Write::write_all(&mut file, text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("Awake could not write its state file: {error}"))?;
    drop(file);
    fs::rename(&tmp, path)
        .map_err(|error| format!("Awake could not finalize its state file: {error}"))?;
    Ok(())
}

/// Deletes the state file; a missing file is fine (already off).
pub fn delete(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Awake could not delete its state file: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::system_manager::awake::core::PowerValues;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn sample_state() -> AwakeStateFile {
        AwakeStateFile {
            version: STATE_VERSION,
            scheme_guid: "381b4222-f694-41f0-9685-ff5bb260df2e".into(),
            started_at_unix: 1_762_400_000,
            ends_at_unix: None,
            keep_screen_on: true,
            originals: PowerValues {
                sleep_ac: Some(1800),
                ..PowerValues::default()
            },
            applied: PowerValues {
                sleep_ac: Some(0),
                ..PowerValues::default()
            },
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "erx-awake-test-{}-{}-{}",
            std::process::id(),
            tag,
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn state_round_trip() {
        let dir = temp_dir("roundtrip");
        let path = state_path(&dir);
        assert_eq!(load(&path), Ok(None), "missing file reads as off");

        let state = sample_state();
        save(&path, &state).expect("save");
        assert_eq!(load(&path), Ok(Some(state)));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupted_file_is_an_error_not_a_panic() {
        let dir = temp_dir("corrupt");
        let path = state_path(&dir);
        fs::write(&path, "{not json at all").expect("write garbage");
        assert!(load(&path).is_err());

        fs::write(&path, "[]").expect("write wrong shape");
        assert!(load(&path).is_err());

        // A wrong version is rejected too.
        let mut state = sample_state();
        state.version = 99;
        save(&path, &state).expect("save");
        assert!(load(&path).is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn extra_fields_are_tolerated() {
        let dir = temp_dir("extra");
        let path = state_path(&dir);
        fs::write(
            &path,
            r#"{"version":1,"scheme_guid":"381b4222-f694-41f0-9685-ff5bb260df2e",
                "started_at_unix":100,"ends_at_unix":null,"keep_screen_on":true,
                "originals":{},"applied":{},"some_future_field":42}"#,
        )
        .expect("write");
        let state = load(&path).expect("tolerant load").expect("some");
        assert_eq!(state.started_at_unix, 100);
        assert_eq!(state.originals, PowerValues::default());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn atomic_write_leaves_no_half_file() {
        let dir = temp_dir("atomic");
        let path = state_path(&dir);

        // First save, then save over it: the content must be fully replaced
        // and no .tmp sibling may remain in either case.
        save(&path, &sample_state()).expect("first save");
        assert!(
            !path.with_extension("json.tmp").exists(),
            "no tmp after first save"
        );

        let mut second = sample_state();
        second.started_at_unix = 42;
        save(&path, &second).expect("overwrite save");
        assert!(
            !path.with_extension("json.tmp").exists(),
            "no tmp after overwrite"
        );
        assert_eq!(load(&path), Ok(Some(second)));
        fs::remove_dir_all(&dir).ok();
    }
}
