//! Tauri command for the system information card. The card is read again
//! every time it is asked for (every time System Manager opens, and by the
//! refresh button): no polling, no stored state, nothing leaves the PC.

use super::card::{SystemCard, build_card};
use super::core::SystemInfo;
use crate::tools::ToolError;

/// Every part is read on its own: a part that cannot be read is simply
/// missing (None / empty) and never stops the others.
#[cfg(windows)]
fn read_system_info() -> SystemInfo {
    use super::{basic, battery, gpu, memory, storage};
    SystemInfo {
        os: basic::os_info(),
        cpu: basic::cpu_info(),
        gpus: gpu::gpu_list(),
        memory: memory::memory_info(),
        drives: storage::drive_list(),
        battery: battery::battery_info(),
        uptime: basic::uptime_text(),
    }
}

#[cfg(not(windows))]
fn read_system_info() -> SystemInfo {
    SystemInfo::default()
}

/// Reads the hardware overview. Runs off the UI thread.
#[tauri::command(async)]
pub fn system_info_get() -> Result<SystemCard, ToolError> {
    Ok(build_card(&read_system_info()))
}
