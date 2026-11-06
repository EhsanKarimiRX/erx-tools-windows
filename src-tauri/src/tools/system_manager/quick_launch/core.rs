//! Pure Quick Launch logic: the fixed ID → target table (the frontend can
//! never send a path or a command), the PowerShell flavor choice, the
//! Microsoft PC Manager package detection, and the AppxManifest AppId
//! extraction. Everything is unit-testable; the Windows calls live in
//! `launch.rs`.
//!
//! Security shape (spec 5e): the UI sends ONLY one of the ids below; Rust
//! maps it to a hard-coded allowlisted target. An unknown id is an error.

/// The allowlisted Store deep link (search, needs no product ID — G8: no
/// product IDs from memory).
pub const PC_MANAGER_STORE_URI: &str = "ms-windows-store://search/?query=Microsoft%20PC%20Manager";

/// Where a button opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetKind {
    /// A file inside the Windows System32 directory (exe or .msc).
    System32File(&'static str),
    /// PowerShell: prefer PowerShell 7, fall back to Windows PowerShell.
    PowerShell,
    /// explorer.exe "shell:AppsFolder\<family>!<app id>".
    PcManager,
    /// The allowlisted Store search URI, opened directly.
    StoreUri,
}

/// One allowlisted launch: which id, what the button says, whether it must
/// request elevation (verb "runas"), and where it points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub admin: bool,
    pub kind: TargetKind,
}

/// The fixed allowlist. Unknown ids never reach a path.
pub fn launch_spec(id: &str) -> Option<LaunchSpec> {
    let (id, label, admin, kind) = match id {
        "control_panel" => (
            "control_panel",
            "Control Panel",
            false,
            TargetKind::System32File("control.exe"),
        ),
        "defrag" => (
            "defrag",
            "Defragment and Optimize Drives",
            false,
            TargetKind::System32File("dfrgui.exe"),
        ),
        "cmd" => (
            "cmd",
            "Command Prompt",
            false,
            TargetKind::System32File("cmd.exe"),
        ),
        "cmd_admin" => (
            "cmd_admin",
            "Command Prompt",
            true,
            TargetKind::System32File("cmd.exe"),
        ),
        "powershell" => ("powershell", "PowerShell", false, TargetKind::PowerShell),
        "powershell_admin" => (
            "powershell_admin",
            "PowerShell",
            true,
            TargetKind::PowerShell,
        ),
        "device_manager" => (
            "device_manager",
            "Device Manager",
            false,
            TargetKind::System32File("devmgmt.msc"),
        ),
        "services" => (
            "services",
            "Services",
            false,
            TargetKind::System32File("services.msc"),
        ),
        "pc_manager" => (
            "pc_manager",
            "Microsoft PC Manager",
            false,
            TargetKind::PcManager,
        ),
        "pc_manager_store" => (
            "pc_manager_store",
            "Microsoft Store page",
            false,
            TargetKind::StoreUri,
        ),
        _ => return None,
    };
    Some(LaunchSpec {
        id,
        label,
        admin,
        kind,
    })
}

/// The environment values the PowerShell choice needs (filled from the
/// machine in `launch.rs`; plain strings here for tests).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvironmentValues {
    pub program_files: Option<String>,
    pub local_app_data: Option<String>,
    pub path: Option<String>,
    /// e.g. "C:\Windows" — the fallback lives under its System32.
    pub system_root: String,
}

/// Which PowerShell will start, and its full path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerShellChoice {
    PowerShell7(String),
    WindowsPowerShell(String),
}

impl PowerShellChoice {
    pub fn label(&self) -> &'static str {
        match self {
            PowerShellChoice::PowerShell7(_) => "PowerShell 7",
            PowerShellChoice::WindowsPowerShell(_) => "Windows PowerShell",
        }
    }

    pub fn path(&self) -> &str {
        match self {
            PowerShellChoice::PowerShell7(path) | PowerShellChoice::WindowsPowerShell(path) => path,
        }
    }
}

/// Prefers PowerShell 7 in this order (spec 5e): %ProgramFiles%\PowerShell\7,
/// the %LOCALAPPDATA% Store alias, then the PATH entries — `where` is never
/// spawned. Windows PowerShell is the honest fallback.
pub fn choose_powershell(
    exists: &dyn Fn(&str) -> bool,
    env: &EnvironmentValues,
) -> PowerShellChoice {
    if let Some(program_files) = &env.program_files {
        let candidate = format!("{program_files}\\PowerShell\\7\\pwsh.exe");
        if exists(&candidate) {
            return PowerShellChoice::PowerShell7(candidate);
        }
    }
    if let Some(local_app_data) = &env.local_app_data {
        let candidate = format!("{local_app_data}\\Microsoft\\WindowsApps\\pwsh.exe");
        if exists(&candidate) {
            return PowerShellChoice::PowerShell7(candidate);
        }
    }
    if let Some(path) = &env.path {
        for entry in path.split(';') {
            let entry = entry.trim().trim_matches('"');
            if entry.is_empty() {
                continue;
            }
            let candidate = format!("{entry}\\pwsh.exe");
            if exists(&candidate) {
                return PowerShellChoice::PowerShell7(candidate);
            }
        }
    }
    let fallback = format!(
        "{}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
        env.system_root.trim_end_matches('\\')
    );
    PowerShellChoice::WindowsPowerShell(fallback)
}

/// Whether Microsoft PC Manager is installed, and its package family name
/// ("Name_PublisherId") when it is. Works from the LIST of full package
/// names only (pure); the app id comes from the manifest in `launch.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PcManagerState {
    Installed { family: String },
    NotInstalled,
}

/// A full package name has the shape
/// `Name_Version_Arch_ResourceId_PublisherId` (ResourceId may be empty, so
/// the split can contain an empty part); the family name is
/// `Name_PublisherId`. Verified live — see docs/PROMPT_5_PROGRESS.md.
pub fn pc_manager_state(installed_packages: &[String]) -> PcManagerState {
    for full_name in installed_packages {
        let parts: Vec<&str> = full_name.split('_').collect();
        if parts.len() < 2 {
            continue;
        }
        let name = parts[0];
        let publisher = parts[parts.len() - 1];
        if name.to_ascii_lowercase().contains("pcmanager") {
            return PcManagerState::Installed {
                family: format!("{name}_{publisher}"),
            };
        }
    }
    PcManagerState::NotInstalled
}

/// The Application Id of the MAIN app from an AppxManifest.xml: the first
/// `<Application … Id="…">` tag (NOT `<Applications>` — the plural wrapper
/// comes first in every real manifest). Verified live on PC Manager, whose
/// main entry is `Id="App" Executable="PCManager\MSPCManager.exe"`.
pub fn extract_app_id(manifest_text: &str) -> Option<String> {
    let mut search_from = 0_usize;
    loop {
        let position = manifest_text[search_from..].find("<Application")?;
        let absolute = search_from + position;
        let after = &manifest_text[absolute + "<Application".len()..];
        match after.chars().next()? {
            // "<Applications>" is the wrapper, not an app entry: keep looking.
            letter if letter.is_ascii_alphabetic() => {
                search_from = absolute + "<Application".len();
                continue;
            }
            _ => {}
        }
        let tag_end = manifest_text[absolute..].find('>')? + absolute;
        let tag = &manifest_text[absolute..=tag_end];
        let id_start = tag.find("Id=\"")? + "Id=\"".len();
        let rest = &tag[id_start..];
        let id_end = rest.find('"')?;
        return Some(rest[..id_end].to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_id_maps_and_unknown_rejected() {
        let ids = [
            "control_panel",
            "defrag",
            "cmd",
            "cmd_admin",
            "powershell",
            "powershell_admin",
            "device_manager",
            "services",
            "pc_manager",
            "pc_manager_store",
        ];
        for id in ids {
            let spec = launch_spec(id).unwrap_or_else(|| panic!("{id} must map"));
            assert_eq!(spec.id, id);
        }
        assert!(launch_spec("bogus").is_none());
        assert!(launch_spec("").is_none());
        assert!(
            launch_spec("C:\\Windows\\System32\\cmd.exe").is_none(),
            "no paths, ever"
        );
        assert!(launch_spec("cmd.exe /c evil").is_none());
    }

    #[test]
    fn admin_ids_use_the_admin_flag_only_for_them() {
        assert!(launch_spec("cmd_admin").unwrap().admin);
        assert!(launch_spec("powershell_admin").unwrap().admin);
        for id in [
            "control_panel",
            "defrag",
            "cmd",
            "powershell",
            "device_manager",
            "services",
            "pc_manager",
        ] {
            assert!(!launch_spec(id).unwrap().admin, "{id} is not admin");
        }
    }

    #[test]
    fn system32_targets_are_exact_file_names() {
        assert_eq!(
            launch_spec("control_panel").unwrap().kind,
            TargetKind::System32File("control.exe")
        );
        assert_eq!(
            launch_spec("defrag").unwrap().kind,
            TargetKind::System32File("dfrgui.exe")
        );
        assert_eq!(
            launch_spec("device_manager").unwrap().kind,
            TargetKind::System32File("devmgmt.msc")
        );
        assert_eq!(
            launch_spec("services").unwrap().kind,
            TargetKind::System32File("services.msc")
        );
    }

    #[test]
    fn store_uri_is_exactly_the_allowlisted_string() {
        assert_eq!(
            launch_spec("pc_manager_store").unwrap().kind,
            TargetKind::StoreUri
        );
        assert_eq!(
            PC_MANAGER_STORE_URI,
            "ms-windows-store://search/?query=Microsoft%20PC%20Manager"
        );
    }

    #[test]
    fn powershell_prefers_program_files_location() {
        let env = EnvironmentValues {
            program_files: Some("C:\\Program Files".into()),
            local_app_data: Some("C:\\Users\\x\\AppData\\Local".into()),
            path: Some("C:\\other".into()),
            system_root: "C:\\Windows".into(),
        };
        let seen = ["C:\\Program Files\\PowerShell\\7\\pwsh.exe"];
        let exists = |candidate: &str| seen.contains(&candidate);
        let choice = choose_powershell(&exists, &env);
        assert_eq!(
            choice,
            PowerShellChoice::PowerShell7("C:\\Program Files\\PowerShell\\7\\pwsh.exe".into())
        );
        assert_eq!(choice.label(), "PowerShell 7");
    }

    #[test]
    fn powershell_store_alias_before_path() {
        let env = EnvironmentValues {
            program_files: Some("C:\\Program Files".into()),
            local_app_data: Some("C:\\Users\\x\\AppData\\Local".into()),
            path: Some("C:\\dir1;C:\\dir2".into()),
            system_root: "C:\\Windows".into(),
        };
        let seen = [
            "C:\\Users\\x\\AppData\\Local\\Microsoft\\WindowsApps\\pwsh.exe",
            "C:\\dir2\\pwsh.exe",
        ];
        let exists = |candidate: &str| seen.contains(&candidate);
        assert!(matches!(
            choose_powershell(&exists, &env),
            PowerShellChoice::PowerShell7(_)
        ));
    }

    #[test]
    fn powershell_path_scan_finds_pwsh() {
        let env = EnvironmentValues {
            program_files: Some("C:\\Program Files".into()),
            local_app_data: None,
            path: Some("\"C:\\quoted dir\";;C:\\plain".into()),
            system_root: "C:\\Windows".into(),
        };
        let seen = ["C:\\plain\\pwsh.exe"];
        let exists = |candidate: &str| seen.contains(&candidate);
        assert_eq!(
            choose_powershell(&exists, &env),
            PowerShellChoice::PowerShell7("C:\\plain\\pwsh.exe".into())
        );
    }

    #[test]
    fn powershell_falls_back_to_windows_powershell() {
        let env = EnvironmentValues {
            program_files: None,
            local_app_data: None,
            path: Some("C:\\nothing-here".into()),
            system_root: "C:\\WINDOWS".into(),
        };
        let exists = |_candidate: &str| false;
        let choice = choose_powershell(&exists, &env);
        assert_eq!(
            choice,
            PowerShellChoice::WindowsPowerShell(
                "C:\\WINDOWS\\System32\\WindowsPowerShell\\v1.0\\powershell.exe".into()
            )
        );
        assert_eq!(choice.label(), "Windows PowerShell");
    }

    #[test]
    fn pc_manager_state_from_real_shaped_names() {
        // The real package name found on this machine (verified live).
        let packages = vec![
            "1527c705-839a-4832-9118-54d4Bd6a0c89_10.0.19640.1000_neutral_neutral_cw5n1h2txyewy"
                .to_string(),
            "AIToolkit.Inference.Agent_1.1.0.0_x64__8wekyb3d8bbwe".to_string(),
            "Microsoft.MicrosoftPCManager_3.23.0.0_x64__8wekyb3d8bbwe".to_string(),
        ];
        assert_eq!(
            pc_manager_state(&packages),
            PcManagerState::Installed {
                family: "Microsoft.MicrosoftPCManager_8wekyb3d8bbwe".into()
            }
        );
    }

    #[test]
    fn pc_manager_state_is_case_insensitive_and_absent_safe() {
        assert_eq!(
            pc_manager_state(&["microsoft.microsoftpcmanager_1.0_x64__pub".to_string()]),
            PcManagerState::Installed {
                family: "microsoft.microsoftpcmanager_pub".into()
            }
        );
        assert_eq!(pc_manager_state(&[]), PcManagerState::NotInstalled);
        assert_eq!(
            pc_manager_state(&["SomethingElse_1.0_x64__pub".to_string()]),
            PcManagerState::NotInstalled
        );
        // The spec's rule is a plain substring match, so a lookalike name
        // that CONTAINS "pcmanager" does match — that is intended.
        assert!(matches!(
            pc_manager_state(&["Contoso.PCManagerFanClub_1.0_x64__pub".to_string()]),
            PcManagerState::Installed { .. }
        ));
    }

    #[test]
    fn extract_app_id_reads_the_first_application_tag() {
        // Mirrors the REAL PC Manager manifest shape (wrapper first).
        let manifest = r#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10">
  <Applications>
    <Application Id="App" Executable="PCManager\MSPCManager.exe" EntryPoint="Windows.FullTrustApplication">
    </Application>
    <Application Id="PCManagerCore" Executable="PCManager\MSPCManagerCore.exe">
    </Application>
  </Applications>
</Package>"#;
        assert_eq!(extract_app_id(manifest), Some("App".to_string()));
    }

    #[test]
    fn extract_app_id_handles_missing_or_odd_manifests() {
        assert_eq!(extract_app_id("<Package></Package>"), None);
        assert_eq!(extract_app_id(""), None);
        assert_eq!(
            extract_app_id("<Application Id=\"Solo\"/>"),
            Some("Solo".to_string())
        );
    }
}
