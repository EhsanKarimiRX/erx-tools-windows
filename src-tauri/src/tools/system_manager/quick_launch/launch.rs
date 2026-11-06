//! Windows launch layer for Quick Launch: full paths from
//! GetSystemDirectoryW (a poisoned PATH cannot hijack them), ShellExecuteExW
//! with verb "open" (or "runas" for the two admin buttons), the Store URI
//! opened directly, and PC Manager detection through the package registry
//! (no process spawning) plus its AppxManifest.xml. All layouts on this
//! machine were verified live — see docs/PROMPT_5_PROGRESS.md.

use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{ERROR_CANCELLED, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, REG_VALUE_TYPE, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW,
    RegGetValueW, RegOpenKeyExW, RegQueryInfoKeyW,
};
use windows_sys::Win32::UI::Shell::{
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use super::core::{
    EnvironmentValues, LaunchSpec, PC_MANAGER_STORE_URI, PcManagerState, PowerShellChoice,
    TargetKind, choose_powershell, extract_app_id, pc_manager_state,
};

/// What happened when the user asked for a launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchOutcome {
    /// ShellExecute accepted the request (that is all we can honestly know).
    Opened,
    /// The user dismissed the UAC prompt (1223) — neutral, not an error.
    Cancelled,
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The Windows System32 directory (full path; a poisoned PATH cannot
/// hijack the launch targets).
pub fn system_dir() -> Result<PathBuf, String> {
    let mut buffer = [0_u16; 512];
    let length = unsafe {
        windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(buffer.as_mut_ptr(), 512)
    };
    if length == 0 || length as usize >= 512 {
        return Err("Couldn't find the Windows System32 folder.".into());
    }
    Ok(PathBuf::from(String::from_utf16_lossy(
        &buffer[..length as usize],
    )))
}

/// The Windows directory (explorer.exe lives there).
pub fn windows_dir() -> Result<PathBuf, String> {
    let mut buffer = [0_u16; 512];
    let length = unsafe {
        windows_sys::Win32::System::SystemInformation::GetWindowsDirectoryW(
            buffer.as_mut_ptr(),
            512,
        )
    };
    if length == 0 || length as usize >= 512 {
        return Err("Couldn't find the Windows folder.".into());
    }
    Ok(PathBuf::from(String::from_utf16_lossy(
        &buffer[..length as usize],
    )))
}

/// The PowerShell choice for the caption and the launch, from the real
/// environment (no `where` spawning).
pub fn resolve_powershell() -> Result<PowerShellChoice, String> {
    let env = EnvironmentValues {
        program_files: std::env::var("ProgramFiles").ok(),
        local_app_data: std::env::var("LOCALAPPDATA").ok(),
        path: std::env::var("PATH").ok(),
        system_root: windows_dir()?.to_string_lossy().into_owned(),
    };
    Ok(choose_powershell(
        &|candidate| Path::new(candidate).is_file(),
        &env,
    ))
}

/// One ShellExecuteExW launch. `fMask` has NOASYNC (safe on a worker
/// thread) and FLAG_NO_UI (errors come back to us, so the app can show an
/// honest message instead of a system dialog).
fn shell_execute(
    file: &str,
    parameters: Option<&str>,
    verb: &str,
) -> Result<LaunchOutcome, String> {
    let file_wide = wide(file);
    let parameters_wide = parameters.map(wide);
    let verb_wide = wide(verb);
    let mut info: SHELLEXECUTEINFOW = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: verb_wide.as_ptr(),
        lpFile: file_wide.as_ptr(),
        lpParameters: parameters_wide
            .as_ref()
            .map(|value| value.as_ptr())
            .unwrap_or_else(std::ptr::null),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };
    let started = unsafe { ShellExecuteExW(&mut info) };
    if started != 0 {
        return Ok(LaunchOutcome::Opened);
    }
    let error = unsafe { windows_sys::Win32::Foundation::GetLastError() };
    if error == ERROR_CANCELLED {
        return Ok(LaunchOutcome::Cancelled);
    }
    Err(format!("Windows could not start it (error code {error})."))
}

/// Launches one allowlisted spec. `pc_manager` carries the resolved
/// family/app id (detected by `detect_pc_manager`).
pub fn launch(
    spec: &LaunchSpec,
    powershell: &PowerShellChoice,
    pc_manager: Option<&(String, String)>,
) -> Result<LaunchOutcome, String> {
    let verb = if spec.admin { "runas" } else { "open" };
    match &spec.kind {
        TargetKind::System32File(name) => {
            let file = system_dir()?.join(name);
            if !file.exists() {
                return Err(format!("'{name}' was not found in System32."));
            }
            shell_execute(&file.to_string_lossy(), None, verb)
        }
        TargetKind::PowerShell => shell_execute(powershell.path(), None, verb),
        TargetKind::PcManager => {
            let (family, app_id) =
                pc_manager.ok_or_else(|| "Microsoft PC Manager is not installed.".to_string())?;
            let explorer = windows_dir()?.join("explorer.exe");
            let target = format!("shell:AppsFolder\\{family}!{app_id}");
            shell_execute(&explorer.to_string_lossy(), Some(&target), "open")
        }
        TargetKind::StoreUri => shell_execute(PC_MANAGER_STORE_URI, None, "open"),
    }
}

/// The Packages registry key where every Store (MSIX) package registers.
const PACKAGES_SUBKEY: &str = r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages";

/// Detects Microsoft PC Manager from the package registry: enumerates the
/// subkeys of the Repository\\Packages key, runs the PURE
/// `pc_manager_state` over the names, then reads the matching package's
/// PackageRootFolder and pulls the main Application Id from
/// AppxManifest.xml. No process spawns. Returns (family name, app id).
pub fn detect_pc_manager() -> Option<(String, String)> {
    let names = package_names()?;
    let family = match pc_manager_state(&names) {
        PcManagerState::Installed { family } => family,
        PcManagerState::NotInstalled => return None,
    };
    // The full name whose family matches (needed for the root folder).
    let full_name = names
        .iter()
        .find(|name| family_name(name).as_deref() == Some(family.as_str()))?;
    let root_folder = read_package_root_folder(full_name)?;
    let manifest_path = Path::new(&root_folder).join("AppxManifest.xml");
    let manifest_text = std::fs::read_to_string(manifest_path).ok()?;
    let app_id = extract_app_id(&manifest_text)?;
    Some((family, app_id))
}

/// Enumerates ALL package subkey names of the Repository\\Packages key.
fn package_names() -> Option<Vec<String>> {
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }
    fn from_wide(buffer: &[u16]) -> String {
        let length = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..length])
    }

    let subkey_wide = wide(PACKAGES_SUBKEY);
    let mut key: HKEY = std::ptr::null_mut();
    let code = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey_wide.as_ptr(),
            0,
            KEY_READ,
            &mut key,
        )
    };
    if code != ERROR_SUCCESS {
        return None;
    }

    let mut names: Vec<String> = Vec::new();
    unsafe {
        let mut subkey_count: u32 = 0;
        let mut max_subkey_len: u32 = 0;
        let mut class_len: u32 = 0;
        let info_code = RegQueryInfoKeyW(
            key,
            std::ptr::null_mut(),
            &mut class_len,
            std::ptr::null(),
            &mut subkey_count,
            &mut max_subkey_len,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        if info_code == ERROR_SUCCESS && subkey_count > 0 {
            let mut buffer = vec![0_u16; max_subkey_len as usize + 1];
            for index in 0..subkey_count {
                let mut name_length = buffer.len() as u32;
                let enum_code = RegEnumKeyExW(
                    key,
                    index,
                    buffer.as_mut_ptr(),
                    &mut name_length,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                if enum_code == ERROR_SUCCESS {
                    names.push(from_wide(&buffer));
                }
            }
        }
        RegCloseKey(key);
    }
    Some(names)
}

/// `Name_Version_Arch_ResourceId_PublisherId` → `Name_PublisherId`.
fn family_name(full_name: &str) -> Option<String> {
    let parts: Vec<&str> = full_name.split('_').collect();
    if parts.len() < 2 {
        return None;
    }
    Some(format!("{}_{}", parts[0], parts[parts.len() - 1]))
}

/// Reads the PackageRootFolder REG_SZ of one package subkey.
fn read_package_root_folder(package_full_name: &str) -> Option<String> {
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }
    let subkey_wide = wide(&format!("{PACKAGES_SUBKEY}\\{package_full_name}"));
    let mut key: HKEY = std::ptr::null_mut();
    let code = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey_wide.as_ptr(),
            0,
            KEY_READ,
            &mut key,
        )
    };
    if code != ERROR_SUCCESS {
        return None;
    }
    let value_wide = wide("PackageRootFolder");
    let mut buffer = [0_u16; 1024];
    let mut size: u32 = (buffer.len() * 2) as u32;
    let mut kind: REG_VALUE_TYPE = 0;
    let code = unsafe {
        RegGetValueW(
            key,
            std::ptr::null(),
            value_wide.as_ptr(),
            RRF_RT_REG_SZ,
            &mut kind,
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    unsafe { RegCloseKey(key) };
    if code != ERROR_SUCCESS {
        return None;
    }
    let chars = (size as usize / 2).min(buffer.len());
    let text = String::from_utf16_lossy(&buffer[..chars]);
    let text = text.trim_end_matches('\0');
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// Everything the info command needs in one call: the PowerShell flavor
/// and whether PC Manager is installed (with its launch coordinates).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickLaunchFacts {
    pub powershell: PowerShellChoice,
    pub pc_manager_installed: bool,
    pub pc_manager_family: Option<String>,
    pub pc_manager_app_id: Option<String>,
}

pub fn quick_launch_facts() -> Result<QuickLaunchFacts, String> {
    let powershell = resolve_powershell()?;
    let detected = detect_pc_manager();
    Ok(QuickLaunchFacts {
        powershell,
        pc_manager_installed: detected.is_some(),
        pc_manager_family: detected.as_ref().map(|(family, _)| family.clone()),
        pc_manager_app_id: detected.map(|(_, app_id)| app_id),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_name_from_real_shape() {
        assert_eq!(
            family_name("Microsoft.MicrosoftPCManager_3.23.0.0_x64__8wekyb3d8bbwe"),
            Some("Microsoft.MicrosoftPCManager_8wekyb3d8bbwe".to_string())
        );
        // Empty resource id still yields five parts.
        assert_eq!(
            family_name("Some.App_1.0.0.0_x64__publisher"),
            Some("Some.App_publisher".to_string())
        );
        assert_eq!(family_name("no-separator"), None);
    }
}
