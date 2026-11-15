//! Battery health. Windows exposes batteries as device interfaces; we open
//! the first one and ask for its design capacity, full-charge capacity and
//! charge cycles. No administrator rights are needed. A PC without a battery
//! returns None.

use std::ptr;

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, HDEVINFO, SP_DEVICE_INTERFACE_DATA,
    SP_DEVICE_INTERFACE_DETAIL_DATA_W, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces,
    SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows_sys::core::GUID;

use super::bytes::u32_at;
use super::core::{BatteryInfo, SEP, battery_health_percent, battery_label};

/// GUID_DEVICE_BATTERY {72631E54-78A4-11D0-BCF7-00AA00B7B32A}.
const GUID_DEVICE_BATTERY: GUID = GUID {
    data1: 0x7263_1E54,
    data2: 0x78A4,
    data3: 0x11D0,
    data4: [0xBC, 0xF7, 0x00, 0xAA, 0x00, 0xB7, 0xB3, 0x2A],
};

/// CTL_CODE(FILE_DEVICE_BATTERY = 0x29, 0x10, METHOD_BUFFERED, FILE_READ_ACCESS).
const IOCTL_BATTERY_QUERY_TAG: u32 = 0x0029_4040;
/// Same device type, function 0x11.
const IOCTL_BATTERY_QUERY_INFORMATION: u32 = 0x0029_4044;

/// Reads the first battery. None = no battery or it cannot be read.
fn read_first_battery() -> Option<BatteryInfo> {
    let set = unsafe {
        SetupDiGetClassDevsW(
            &GUID_DEVICE_BATTERY,
            ptr::null(),
            ptr::null_mut(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
    };
    if (set as isize) == -1 {
        return None;
    }
    let result = read_battery_from_set(set);
    unsafe { SetupDiDestroyDeviceInfoList(set) };
    result
}

fn read_battery_from_set(set: HDEVINFO) -> Option<BatteryInfo> {
    let mut interface: SP_DEVICE_INTERFACE_DATA = unsafe { std::mem::zeroed() };
    interface.cbSize = size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
    let found = unsafe {
        SetupDiEnumDeviceInterfaces(set, ptr::null(), &GUID_DEVICE_BATTERY, 0, &mut interface)
    };
    if found == 0 {
        return None;
    }
    // Ask how big the detail buffer must be (this first call fails on purpose).
    let mut needed: u32 = 0;
    unsafe {
        SetupDiGetDeviceInterfaceDetailW(
            set,
            &interface,
            ptr::null_mut(),
            0,
            &mut needed,
            ptr::null_mut(),
        )
    };
    if needed == 0 || needed > 4096 {
        return None;
    }
    // u64 elements keep the buffer 8-byte aligned.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8) + 1];
    let detail = buffer
        .as_mut_ptr()
        .cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
    unsafe {
        (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
    }
    let ok = unsafe {
        SetupDiGetDeviceInterfaceDetailW(
            set,
            &interface,
            detail,
            u32::try_from(buffer.len() * 8).ok()?,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        return None;
    }
    let path = unsafe { ptr::addr_of!((*detail).DevicePath).cast::<u16>() };
    let handle = unsafe {
        CreateFileW(
            path,
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }
    let info = query_battery(handle);
    unsafe { CloseHandle(handle) };
    info
}

fn query_battery(handle: HANDLE) -> Option<BatteryInfo> {
    // 1. The battery tag (wait time 0 = answer immediately).
    let wait: u32 = 0;
    let mut tag: u32 = 0;
    let mut returned: u32 = 0;
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_BATTERY_QUERY_TAG,
            ptr::addr_of!(wait).cast(),
            4,
            ptr::addr_of_mut!(tag).cast(),
            4,
            &mut returned,
            ptr::null_mut(),
        )
    };
    if ok == 0 || tag == 0 {
        return None;
    }
    // 2. BATTERY_QUERY_INFORMATION: tag, information level 0
    //    (BatteryInformation), at-rate 0.
    let mut query = [0u8; 12];
    query[0..4].copy_from_slice(&tag.to_le_bytes());
    // 3. BATTERY_INFORMATION (36 bytes): capabilities at 0, designed capacity
    //    at 12, full-charge capacity at 16, cycle count at 32.
    let mut output = [0u8; 36];
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_BATTERY_QUERY_INFORMATION,
            query.as_ptr().cast(),
            12,
            output.as_mut_ptr().cast(),
            36,
            &mut returned,
            ptr::null_mut(),
        )
    };
    if ok == 0 || (returned as usize) < output.len() {
        return None;
    }
    let capabilities = u32_at(&output, 0)?;
    let designed = u32_at(&output, 12)?;
    let full = u32_at(&output, 16)?;
    let cycles = u32_at(&output, 32)?;
    let health = battery_health_percent(designed, full, capabilities);
    let mut parts: Vec<String> = Vec::new();
    if cycles > 0 && cycles != u32::MAX {
        parts.push(format!("{cycles} charge cycles"));
    }
    match health {
        Some(percent) => parts.push(battery_label(percent).to_string()),
        None => parts.push("Health not reported".to_string()),
    }
    Some(BatteryInfo {
        health_percent: health,
        detail: parts.join(SEP),
    })
}

/// True when Windows says the PC has a battery (128 = none, 255 = unknown).
fn battery_present() -> bool {
    let mut status: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetSystemPowerStatus(&mut status) };
    ok != 0 && status.BatteryFlag != 128 && status.BatteryFlag != 255
}

/// Battery overview, or None for a PC without a battery.
pub fn battery_info() -> Option<BatteryInfo> {
    match read_first_battery() {
        Some(info) => Some(info),
        None if battery_present() => Some(BatteryInfo {
            health_percent: None,
            detail: "Health not reported".to_string(),
        }),
        None => None,
    }
}
