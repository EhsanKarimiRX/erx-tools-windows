//! Update check backend: pure decision logic (version, release, result),
//! the WinHTTP network layer and the Tauri commands.
//!
//! The check runs only when the user clicks the update button on the home
//! screen — never automatically. One HTTPS GET to GitHub's releases API is
//! the app's only network access (see DECISIONS.md, Prompt 9).

pub mod commands;
pub mod http;
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

    /// Guards the v0.2.2 rename of the installed app to "ERX Tools": the NSIS
    /// installer derives the default install folder, the Start Menu/Desktop
    /// shortcut names and the uninstall registry key from `productName`, and
    /// the window title is user-visible. The identifier must never change
    /// (it keys the app data folders), and no NSIS installer hook may be
    /// added (owner decision, see DECISIONS.md).
    #[test]
    fn product_name_and_title_are_erx_tools() {
        let tauri_conf: serde_json::Value =
            serde_json::from_str(include_str!("../../../tauri.conf.json"))
                .expect("src-tauri/tauri.conf.json must be valid JSON");

        let product_name = tauri_conf["productName"]
            .as_str()
            .expect("tauri.conf.json must have a productName string");
        let window_title = tauri_conf["app"]["windows"][0]["title"]
            .as_str()
            .expect("tauri.conf.json must have an app.windows[0].title string");
        let identifier = tauri_conf["identifier"]
            .as_str()
            .expect("tauri.conf.json must have an identifier string");

        assert_eq!(product_name, "ERX Tools", "productName must be ERX Tools");
        assert_eq!(
            window_title, "ERX Tools",
            "the main window title must be ERX Tools"
        );
        assert_eq!(
            identifier, "com.erxtools.windows",
            "the identifier must not change"
        );
        assert!(
            tauri_conf["bundle"]["windows"]["nsis"]
                .get("installerHooks")
                .is_none(),
            "bundle.windows.nsis.installerHooks must not be present"
        );
    }
}
