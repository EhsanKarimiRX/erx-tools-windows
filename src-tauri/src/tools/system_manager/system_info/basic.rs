//! Windows version, processor and uptime.

use std::ptr;

use windows_sys::Win32::System::SystemInformation::{
    GetLogicalProcessorInformationEx, GetTickCount64, RelationProcessorCore,
};
use windows_sys::Win32::System::Threading::{
    GetActiveProcessorCount, GetCurrentProcess, IsWow64Process2,
};

use super::core::{
    NamedDetail, SEP, arch_label, clean_cpu_name, count_core_records, format_uptime,
    windows_product_name,
};
use super::winreg::read_hklm_string;

const OS_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
const CPU_KEY: &str = r"HARDWARE\DESCRIPTION\System\CentralProcessor\0";
const ALL_PROCESSOR_GROUPS: u16 = 0xFFFF;

/// The processor architecture of the PC itself (also correct when this app
/// runs emulated), as an IMAGE_FILE_MACHINE value; 0 = unknown.
fn native_machine() -> u16 {
    let mut process_machine: u16 = 0;
    let mut native: u16 = 0;
    let ok = unsafe { IsWow64Process2(GetCurrentProcess(), &mut process_machine, &mut native) };
    if ok != 0 { native } else { 0 }
}

/// "Windows 11 Pro" with the detail "64-bit (x64) - 24H2 - build 26100".
pub fn os_info() -> Option<NamedDetail> {
    let product = read_hklm_string(OS_KEY, "ProductName")?;
    let build: u32 = read_hklm_string(OS_KEY, "CurrentBuildNumber")
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    let name = windows_product_name(&product, build);
    let mut parts: Vec<String> = Vec::new();
    let arch = arch_label(native_machine());
    if !arch.is_empty() {
        parts.push(arch.to_string());
    }
    if let Some(version) = read_hklm_string(OS_KEY, "DisplayVersion") {
        parts.push(version);
    }
    if build > 0 {
        parts.push(format!("build {build}"));
    }
    Some(NamedDetail {
        name,
        detail: parts.join(SEP),
    })
}

/// Number of physical cores (None when Windows does not say).
fn physical_core_count() -> Option<u32> {
    let mut length: u32 = 0;
    // The first call only reports how many bytes are needed (it fails on purpose).
    unsafe {
        GetLogicalProcessorInformationEx(RelationProcessorCore, ptr::null_mut(), &mut length)
    };
    if length == 0 || length > 1_000_000 {
        return None;
    }
    let mut buffer = vec![0u8; length as usize];
    let ok = unsafe {
        GetLogicalProcessorInformationEx(
            RelationProcessorCore,
            buffer.as_mut_ptr().cast(),
            &mut length,
        )
    };
    if ok == 0 {
        return None;
    }
    let used = buffer.get(..length as usize)?;
    let cores = count_core_records(used);
    if cores == 0 { None } else { Some(cores) }
}

/// Cleaned processor name with the detail "10 cores - 16 threads".
pub fn cpu_info() -> Option<NamedDetail> {
    let raw = read_hklm_string(CPU_KEY, "ProcessorNameString")?;
    let name = clean_cpu_name(&raw);
    if name.is_empty() {
        return None;
    }
    let threads = unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) };
    let mut parts: Vec<String> = Vec::new();
    if let Some(cores) = physical_core_count() {
        parts.push(format!("{cores} cores"));
    }
    if threads > 0 {
        parts.push(format!("{threads} threads"));
    }
    Some(NamedDetail {
        name,
        detail: parts.join(SEP),
    })
}

/// Time since the last full Windows restart (with Fast Startup, "Shut down"
/// does not reset it - Task Manager shows the same value).
pub fn uptime_text() -> Option<String> {
    let milliseconds = unsafe { GetTickCount64() };
    Some(format_uptime(milliseconds / 1000))
}
