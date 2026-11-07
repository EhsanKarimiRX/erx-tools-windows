//! Update check backend: pure decision logic (version, release, result)
//! plus the Windows HTTP client and the Tauri commands that use them.
//!
//! The check runs only when the user clicks the update button on the home
//! screen — never automatically. One HTTPS GET to GitHub's releases API is
//! the app's only network access (see DECISIONS.md, Prompt 9).

// Nothing consumes this module outside its own tests until 9c registers the
// commands (same pattern as folder_icon/probe.rs's allow(dead_code));
// removed when the commands land in 9c.
#![allow(dead_code)]

pub mod release;
pub mod result;
pub mod version;

#[cfg(test)]
mod consistency_tests {
    /// Guards the three places that carry the app version: `tauri.conf.json`
    /// (the source the home screen shows), root `package.json` and
    /// `Cargo.toml` (read via `CARGO_PKG_VERSION`). Fails loudly if a future
    /// release bumps only one of them. The `include_str!` paths resolve at
    /// compile time, so a moved file breaks the build, not the release.
    #[test]
    fn version_is_consistent_across_config_files() {
        let tauri_conf: serde_json::Value =
            serde_json::from_str(include_str!("../../../tauri.conf.json"))
                .expect("src-tauri/tauri.conf.json must be valid JSON");
        let package_json: serde_json::Value =
            serde_json::from_str(include_str!("../../../../package.json"))
                .expect("package.json must be valid JSON");

        let tauri_version = tauri_conf["version"]
            .as_str()
            .expect("tauri.conf.json must have a version string");
        let package_version = package_json["version"]
            .as_str()
            .expect("package.json must have a version string");
        let cargo_version = env!("CARGO_PKG_VERSION");

        assert_eq!(
            tauri_version, package_version,
            "tauri.conf.json and package.json versions differ"
        );
        assert_eq!(
            tauri_version, cargo_version,
            "tauri.conf.json and Cargo.toml (CARGO_PKG_VERSION) versions differ"
        );
    }
}
