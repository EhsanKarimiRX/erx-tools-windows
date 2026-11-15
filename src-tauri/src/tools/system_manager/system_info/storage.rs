//! Internal drives: model, kind (NVMe / SATA, SSD / HDD) and size. Each
//! physical drive is opened with ZERO access rights (enough to ask Windows
//! questions, needs no administrator rights) and asked through
//! DeviceIoControl. USB sticks, memory cards and virtual disks are skipped.

use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

use super::bytes::{ascii_at, u32_at, u64_at};
use super::core::{
    NamedDetail, SEP, drive_display_name, drive_kind, format_bytes_decimal, is_internal_bus,
};
use super::winreg::wide;

/// CTL_CODE(IOCTL_STORAGE_BASE, 0x0500, METHOD_BUFFERED, FILE_ANY_ACCESS).
const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;
/// CTL_CODE(IOCTL_DISK_BASE, 0x0028, METHOD_BUFFERED, FILE_ANY_ACCESS).
const IOCTL_DISK_GET_DRIVE_GEOMETRY_EX: u32 = 0x0007_00A0;
const PROPERTY_DEVICE_DESCRIPTOR: u32 = 0;
const PROPERTY_SEEK_PENALTY: u32 = 7;
/// Physical drive numbers 0..MAX_DRIVES are tried; missing numbers are skipped.
const MAX_DRIVES: u32 = 32;

/// STORAGE_PROPERTY_QUERY: property id, query type 0 (standard), 4 spare bytes.
fn property_query(property_id: u32) -> [u8; 12] {
    let mut query = [0u8; 12];
    query[0..4].copy_from_slice(&property_id.to_le_bytes());
    query
}

/// One DeviceIoControl call. Returns the number of bytes written to `output`.
fn ioctl(handle: HANDLE, code: u32, input: &[u8], output: &mut [u8]) -> Option<usize> {
    let mut returned: u32 = 0;
    let input_pointer = if input.is_empty() {
        ptr::null()
    } else {
        input.as_ptr().cast()
    };
    let ok = unsafe {
        DeviceIoControl(
            handle,
            code,
            input_pointer,
            u32::try_from(input.len()).ok()?,
            output.as_mut_ptr().cast(),
            u32::try_from(output.len()).ok()?,
            &mut returned,
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        None
    } else {
        Some(returned as usize)
    }
}

fn query_drive(handle: HANDLE) -> Option<NamedDetail> {
    // Device descriptor: bus type at byte 28, vendor text offset at 12,
    // product text offset at 16 (offsets are from the start of the buffer).
    let mut output = vec![0u8; 2048];
    let returned = ioctl(
        handle,
        IOCTL_STORAGE_QUERY_PROPERTY,
        &property_query(PROPERTY_DEVICE_DESCRIPTOR),
        &mut output,
    )?;
    let descriptor = output.get(..returned)?;
    let bus = u32_at(descriptor, 28)?;
    if !is_internal_bus(bus) {
        return None;
    }
    let vendor = ascii_at(descriptor, u32_at(descriptor, 12)? as usize);
    let product = ascii_at(descriptor, u32_at(descriptor, 16)? as usize);

    // Seek penalty: byte 8 is non-zero for a hard disk, zero for an SSD.
    let mut seek_output = [0u8; 64];
    let seek_penalty = ioctl(
        handle,
        IOCTL_STORAGE_QUERY_PROPERTY,
        &property_query(PROPERTY_SEEK_PENALTY),
        &mut seek_output,
    )
    .and_then(|count| seek_output.get(..count))
    .and_then(|data| data.get(8).copied())
    .map(|flag| flag != 0);

    // Drive size: DISK_GEOMETRY_EX has the total size (i64) at byte 24.
    let mut geometry = vec![0u8; 512];
    let size = ioctl(handle, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, &[], &mut geometry)
        .and_then(|count| geometry.get(..count))
        .and_then(|data| u64_at(data, 24))
        .filter(|bytes| *bytes > 0);

    let mut parts = vec![drive_kind(bus, seek_penalty)];
    if let Some(bytes) = size {
        parts.push(format_bytes_decimal(bytes));
    }
    Some(NamedDetail {
        name: drive_display_name(&vendor, &product),
        detail: parts.join(SEP),
    })
}

/// All internal drives, in drive-number order. Empty when nothing can be read.
pub fn drive_list() -> Vec<NamedDetail> {
    let mut drives = Vec::new();
    for number in 0..MAX_DRIVES {
        let path = wide(&format!(r"\\.\PhysicalDrive{number}"));
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                0,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            continue;
        }
        let drive = query_drive(handle);
        unsafe { CloseHandle(handle) };
        if let Some(drive) = drive {
            drives.push(drive);
        }
    }
    drives
}
