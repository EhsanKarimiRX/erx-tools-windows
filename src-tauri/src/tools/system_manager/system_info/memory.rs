//! Installed RAM: the total, plus the modules the BIOS (SMBIOS) reports.

use std::ptr;

use windows_sys::Win32::System::SystemInformation::{
    GetPhysicallyInstalledSystemMemory, GetSystemFirmwareTable, GlobalMemoryStatusEx,
    MEMORYSTATUSEX,
};

use super::core::{
    MemoryInfo, ModuleInfo, SEP, format_gib_from_kib, format_module_size, memory_type_name,
};
use super::smbios::{MemoryModule, parse_memory_modules};

/// Provider signature 'RSMB' = the raw SMBIOS table.
const FIRMWARE_PROVIDER_RSMB: u32 = 0x5253_4D42;

fn smbios_bytes() -> Option<Vec<u8>> {
    let size = unsafe { GetSystemFirmwareTable(FIRMWARE_PROVIDER_RSMB, 0, ptr::null_mut(), 0) };
    if size == 0 || size > 4_000_000 {
        return None;
    }
    let mut buffer = vec![0u8; size as usize];
    let written = unsafe {
        GetSystemFirmwareTable(FIRMWARE_PROVIDER_RSMB, 0, buffer.as_mut_ptr().cast(), size)
    };
    if written == 0 || written > size {
        return None;
    }
    buffer.truncate(written as usize);
    Some(buffer)
}

/// Installed RAM in kilobytes. Uses the BIOS-reported value; falls back to
/// what Windows can use (a little less than installed).
fn installed_kib() -> Option<u64> {
    let mut kib: u64 = 0;
    let ok = unsafe { GetPhysicallyInstalledSystemMemory(&mut kib) };
    if ok != 0 && kib > 0 {
        return Some(kib);
    }
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    if ok != 0 && status.ullTotalPhys > 0 {
        Some(status.ullTotalPhys / 1024)
    } else {
        None
    }
}

fn module_info(module: &MemoryModule) -> ModuleInfo {
    let mut parts: Vec<String> = Vec::new();
    if let Some(name) = memory_type_name(module.type_code) {
        parts.push(name.to_string());
    }
    if let Some(speed) = module.speed_mts {
        parts.push(format!("{speed} MT/s"));
    }
    ModuleInfo {
        size: format_module_size(module.size_mb),
        detail: parts.join(SEP),
    }
}

pub fn memory_info() -> MemoryInfo {
    let modules = smbios_bytes()
        .map(|raw| parse_memory_modules(&raw))
        .unwrap_or_default();
    let total = match installed_kib() {
        Some(kib) => Some(format_gib_from_kib(kib)),
        None if !modules.is_empty() => {
            let megabytes: u64 = modules.iter().map(|module| u64::from(module.size_mb)).sum();
            Some(format_gib_from_kib(megabytes * 1024))
        }
        None => None,
    };
    MemoryInfo {
        total,
        modules: modules.iter().map(module_info).collect(),
    }
}
