//! Data types sent to the UI, plus the pure text helpers behind them.
//! Nothing in this file touches Windows: every function is plain logic that
//! is covered by unit tests. The Windows readers live in the other files.

use serde::Serialize;

use super::bytes::u32_at;

/// Separator between the parts of one detail line (space, middle dot, space).
pub const SEP: &str = " \u{00B7} ";

/// Windows storage bus types (STORAGE_BUS_TYPE) that we care about.
pub const BUS_SCSI: u32 = 1;
pub const BUS_ATA: u32 = 3;
pub const BUS_RAID: u32 = 8;
pub const BUS_SAS: u32 = 10;
pub const BUS_SATA: u32 = 11;
pub const BUS_NVME: u32 = 17;
pub const BUS_SCM: u32 = 18;
pub const BUS_UFS: u32 = 19;

/// A name with one detail line (Windows, processor, one graphics card, one
/// drive).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedDetail {
    pub name: String,
    pub detail: String,
}

/// One installed RAM module, for example size "16 GB", detail "DDR5 - 5600 MT/s".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleInfo {
    pub size: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInfo {
    /// Total installed RAM as text, for example "32 GB". None = not reported.
    pub total: Option<String>,
    /// Only the modules that are really installed (empty slots are not listed).
    pub modules: Vec<ModuleInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatteryInfo {
    /// Full-charge capacity divided by design capacity, 0..=100. None = the
    /// battery does not report it.
    pub health_percent: Option<u32>,
    /// For example "312 charge cycles - Good" or "Health not reported".
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub os: Option<NamedDetail>,
    pub cpu: Option<NamedDetail>,
    pub gpus: Vec<NamedDetail>,
    pub memory: MemoryInfo,
    pub drives: Vec<NamedDetail>,
    /// None = this PC has no battery.
    pub battery: Option<BatteryInfo>,
    pub uptime: Option<String>,
}

/// "5.0" becomes "5", "7.8" stays "7.8".
fn trim_decimal(value: f64) -> String {
    if (value - value.round()).abs() < 0.05 {
        format!("{}", value.round() as u64)
    } else {
        format!("{value:.1}")
    }
}

/// Processor name as Windows stores it, without trademark signs, the
/// "CPU @ 2.40GHz" tail and the words CPU / Processor.
pub fn clean_cpu_name(raw: &str) -> String {
    let mut text = raw.to_string();
    for junk in ["(R)", "(r)", "(TM)", "(tm)", "(C)"] {
        text = text.replace(junk, "");
    }
    if let Some(position) = text.find('@') {
        text.truncate(position);
    }
    let words: Vec<&str> = text
        .split_whitespace()
        .filter(|word| !word.eq_ignore_ascii_case("cpu") && !word.eq_ignore_ascii_case("processor"))
        .collect();
    words.join(" ")
}

/// Drive size in the units drive makers use (1 TB = 10^12 bytes).
pub fn format_bytes_decimal(bytes: u64) -> String {
    const MB: f64 = 1_000_000.0;
    const GB: f64 = 1_000_000_000.0;
    const TB: f64 = 1_000_000_000_000.0;
    let value = bytes as f64;
    if value >= TB {
        format!("{} TB", trim_decimal((value / TB * 10.0).round() / 10.0))
    } else if value >= GB {
        format!("{} GB", (value / GB).round() as u64)
    } else {
        format!("{} MB", (value / MB).round() as u64)
    }
}

/// RAM size in the units PC makers use (1 GB = 1024 MiB), from kilobytes.
pub fn format_gib_from_kib(kib: u64) -> String {
    let gib = kib as f64 / 1_048_576.0;
    if gib < 1.0 {
        return format!("{} MB", (kib as f64 / 1024.0).round() as u64);
    }
    format!("{} GB", trim_decimal((gib * 10.0).round() / 10.0))
}

/// RAM module size from megabytes (SMBIOS reports megabytes).
pub fn format_module_size(megabytes: u32) -> String {
    format_gib_from_kib(u64::from(megabytes) * 1024)
}

/// SMBIOS memory type code to a name; None = unknown.
pub fn memory_type_name(code: u8) -> Option<&'static str> {
    match code {
        0x12 => Some("DDR"),
        0x13 => Some("DDR2"),
        0x18 => Some("DDR3"),
        0x1A => Some("DDR4"),
        0x1B => Some("LPDDR"),
        0x1C => Some("LPDDR2"),
        0x1D => Some("LPDDR3"),
        0x1E => Some("LPDDR4"),
        0x22 => Some("DDR5"),
        0x23 => Some("LPDDR5"),
        _ => None,
    }
}

/// Drive name from the vendor and product texts Windows reports. Generic
/// vendor words ("ATA", "NVMe", ...) are dropped.
pub fn drive_display_name(vendor: &str, product: &str) -> String {
    let vendor = vendor.trim();
    let product = product.trim();
    let skip_vendor = vendor.is_empty()
        || ["ata", "nvme", "scsi", "generic"]
            .iter()
            .any(|generic| vendor.eq_ignore_ascii_case(generic));
    let joined = if skip_vendor {
        product.to_string()
    } else {
        format!("{vendor} {product}")
    };
    let name = joined.split_whitespace().collect::<Vec<&str>>().join(" ");
    if name.is_empty() {
        "Unknown drive".to_string()
    } else {
        name
    }
}

pub fn bus_label(bus: u32) -> Option<&'static str> {
    match bus {
        BUS_NVME => Some("NVMe"),
        BUS_SATA | BUS_ATA => Some("SATA"),
        BUS_SAS => Some("SAS"),
        BUS_RAID => Some("RAID"),
        BUS_SCSI => Some("SCSI"),
        _ => None,
    }
}

/// True for drives that sit inside the PC. USB sticks, memory cards, virtual
/// disks and network disks are not internal.
pub fn is_internal_bus(bus: u32) -> bool {
    matches!(
        bus,
        BUS_SCSI | BUS_ATA | BUS_RAID | BUS_SAS | BUS_SATA | BUS_NVME | BUS_SCM | BUS_UFS
    )
}

/// "NVMe SSD", "SATA SSD", "SATA HDD", "SSD", "Drive", ...
/// `seek_penalty`: Some(false) = no moving parts (SSD), Some(true) = hard
/// disk, None = Windows did not say.
pub fn drive_kind(bus: u32, seek_penalty: Option<bool>) -> String {
    let media = match (bus, seek_penalty) {
        (BUS_NVME, _) => "SSD",
        (_, Some(false)) => "SSD",
        (_, Some(true)) => "HDD",
        _ => "Drive",
    };
    match bus_label(bus) {
        Some(label) => format!("{label} {media}"),
        None => media.to_string(),
    }
}

/// Windows reports the capabilities flag below when capacities are relative
/// (percent) instead of real units; health cannot be computed then.
pub const BATTERY_CAPACITY_RELATIVE: u32 = 0x4000_0000;

/// Battery health in percent (full-charge capacity / design capacity),
/// capped at 100. None when the numbers cannot be trusted.
pub fn battery_health_percent(designed: u32, full: u32, capabilities: u32) -> Option<u32> {
    if capabilities & BATTERY_CAPACITY_RELATIVE != 0 {
        return None;
    }
    if designed == 0 || designed == u32::MAX || full == 0 || full == u32::MAX {
        return None;
    }
    let percent = (u64::from(full) * 100 + u64::from(designed) / 2) / u64::from(designed);
    Some(percent.min(100) as u32)
}

pub fn battery_label(percent: u32) -> &'static str {
    if percent >= 80 {
        "Good"
    } else if percent >= 60 {
        "Fair"
    } else {
        "Worn"
    }
}

/// "3 d 4 h", "5 h 2 min", "12 min".
pub fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    if days > 0 {
        format!("{days} d {hours} h")
    } else if hours > 0 {
        format!("{hours} h {minutes} min")
    } else {
        format!("{minutes} min")
    }
}

/// Windows 11 still writes "Windows 10 ..." in the ProductName registry
/// value. Build numbers from 22000 on are Windows 11.
pub fn windows_product_name(raw: &str, build: u32) -> String {
    let name = raw.trim();
    if build >= 22_000 && name.starts_with("Windows 10") {
        name.replacen("Windows 10", "Windows 11", 1)
    } else {
        name.to_string()
    }
}

/// Text for the processor architecture of the PC (IMAGE_FILE_MACHINE value).
pub fn arch_label(machine: u16) -> &'static str {
    match machine {
        0x8664 => "64-bit (x64)",
        0xAA64 => "64-bit (ARM64)",
        0x014C => "32-bit",
        _ => "",
    }
}

/// Detail line of a graphics card: its own memory, or "Integrated graphics"
/// when it has (almost) none.
pub fn gpu_detail(dedicated_bytes: u64) -> String {
    if dedicated_bytes >= 1_073_741_824 {
        format_gib_from_kib(dedicated_bytes / 1024)
    } else {
        "Integrated graphics".to_string()
    }
}

/// Counts the "processor core" records (relationship 0) in the buffer that
/// GetLogicalProcessorInformationEx fills. Every record starts with two u32:
/// relationship and size.
pub fn count_core_records(buffer: &[u8]) -> u32 {
    let mut count = 0u32;
    let mut offset = 0usize;
    while let (Some(relationship), Some(size)) = (
        u32_at(buffer, offset),
        u32_at(buffer, offset.saturating_add(4)),
    ) {
        let size = size as usize;
        if size < 8 {
            break;
        }
        let Some(next) = offset.checked_add(size) else {
            break;
        };
        // A record that does not fit into the buffer is damaged: stop, and do not count it.
        if next > buffer.len() {
            break;
        }
        if relationship == 0 {
            count += 1;
        }
        offset = next;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_names_are_cleaned() {
        assert_eq!(
            clean_cpu_name("13th Gen Intel(R) Core(TM) i7-13620H"),
            "13th Gen Intel Core i7-13620H"
        );
        assert_eq!(
            clean_cpu_name("Intel(R) Core(TM) i5-8250U CPU @ 1.60GHz"),
            "Intel Core i5-8250U"
        );
        assert_eq!(
            clean_cpu_name("AMD Ryzen 7 7840HS w/ Radeon 780M Graphics"),
            "AMD Ryzen 7 7840HS w/ Radeon 780M Graphics"
        );
        assert_eq!(clean_cpu_name("   "), "");
    }

    #[test]
    fn drive_sizes_use_maker_units() {
        assert_eq!(format_bytes_decimal(1_000_204_886_016), "1 TB");
        assert_eq!(format_bytes_decimal(2_000_398_934_016), "2 TB");
        assert_eq!(format_bytes_decimal(1_500_301_910_016), "1.5 TB");
        assert_eq!(format_bytes_decimal(500_107_862_016), "500 GB");
        assert_eq!(format_bytes_decimal(256_060_514_304), "256 GB");
        assert_eq!(format_bytes_decimal(960_197_124_096), "960 GB");
        assert_eq!(format_bytes_decimal(0), "0 MB");
    }

    #[test]
    fn ram_sizes_use_binary_units() {
        assert_eq!(format_gib_from_kib(33_554_432), "32 GB");
        assert_eq!(format_gib_from_kib(16_777_216), "16 GB");
        assert_eq!(format_gib_from_kib(8_388_608), "8 GB");
        assert_eq!(format_gib_from_kib(8_179_712), "7.8 GB");
        assert_eq!(format_gib_from_kib(524_288), "512 MB");
        assert_eq!(format_module_size(16_384), "16 GB");
        assert_eq!(format_module_size(512), "512 MB");
        assert_eq!(format_module_size(1_024), "1 GB");
    }

    #[test]
    fn memory_types_are_named() {
        assert_eq!(memory_type_name(0x22), Some("DDR5"));
        assert_eq!(memory_type_name(0x1A), Some("DDR4"));
        assert_eq!(memory_type_name(0x18), Some("DDR3"));
        assert_eq!(memory_type_name(0x23), Some("LPDDR5"));
        assert_eq!(memory_type_name(0x02), None);
    }

    #[test]
    fn drive_names_drop_generic_vendors() {
        assert_eq!(
            drive_display_name("ATA     ", "Samsung SSD 860 EVO 1TB   "),
            "Samsung SSD 860 EVO 1TB"
        );
        assert_eq!(
            drive_display_name("NVMe", "Samsung SSD 990 PRO 1TB"),
            "Samsung SSD 990 PRO 1TB"
        );
        assert_eq!(
            drive_display_name("WDC", "WD10EZEX-08WN4A0"),
            "WDC WD10EZEX-08WN4A0"
        );
        assert_eq!(drive_display_name("", ""), "Unknown drive");
    }

    #[test]
    fn drive_kinds_and_buses() {
        assert_eq!(drive_kind(BUS_NVME, None), "NVMe SSD");
        assert_eq!(drive_kind(BUS_SATA, Some(false)), "SATA SSD");
        assert_eq!(drive_kind(BUS_SATA, Some(true)), "SATA HDD");
        assert_eq!(drive_kind(BUS_SATA, None), "SATA Drive");
        assert_eq!(drive_kind(BUS_SCSI, None), "SCSI Drive");
        assert_eq!(drive_kind(BUS_SCM, Some(false)), "SSD");
        assert!(is_internal_bus(BUS_NVME));
        assert!(is_internal_bus(BUS_SATA));
        assert!(!is_internal_bus(7)); // USB
        assert!(!is_internal_bus(12)); // SD card
        assert!(!is_internal_bus(14)); // virtual
    }

    #[test]
    fn battery_health_rules() {
        assert_eq!(battery_health_percent(50_000, 42_000, 0), Some(84));
        assert_eq!(battery_health_percent(50_000, 52_000, 0), Some(100));
        assert_eq!(battery_health_percent(0, 42_000, 0), None);
        assert_eq!(battery_health_percent(50_000, 0, 0), None);
        assert_eq!(battery_health_percent(u32::MAX, 42_000, 0), None);
        assert_eq!(
            battery_health_percent(100, 90, BATTERY_CAPACITY_RELATIVE),
            None
        );
        assert_eq!(battery_label(84), "Good");
        assert_eq!(battery_label(80), "Good");
        assert_eq!(battery_label(79), "Fair");
        assert_eq!(battery_label(60), "Fair");
        assert_eq!(battery_label(59), "Worn");
    }

    #[test]
    fn uptime_text() {
        assert_eq!(format_uptime(0), "0 min");
        assert_eq!(format_uptime(90), "1 min");
        assert_eq!(format_uptime(5 * 3_600 + 120), "5 h 2 min");
        assert_eq!(format_uptime(3 * 86_400 + 4 * 3_600 + 5), "3 d 4 h");
    }

    #[test]
    fn windows_eleven_is_detected_by_build_number() {
        assert_eq!(
            windows_product_name("Windows 10 Pro", 26_100),
            "Windows 11 Pro"
        );
        assert_eq!(
            windows_product_name("Windows 10 Pro", 19_045),
            "Windows 10 Pro"
        );
        assert_eq!(
            windows_product_name("Windows 11 Home", 22_631),
            "Windows 11 Home"
        );
        assert_eq!(
            windows_product_name("Windows Server 2022 Standard", 20_348),
            "Windows Server 2022 Standard"
        );
    }

    #[test]
    fn small_labels() {
        assert_eq!(arch_label(0x8664), "64-bit (x64)");
        assert_eq!(arch_label(0xAA64), "64-bit (ARM64)");
        assert_eq!(arch_label(0x014C), "32-bit");
        assert_eq!(arch_label(0), "");
        assert_eq!(gpu_detail(8_585_740_288), "8 GB");
        assert_eq!(gpu_detail(4_294_967_296), "4 GB");
        assert_eq!(gpu_detail(134_217_728), "Integrated graphics");
        assert_eq!(gpu_detail(0), "Integrated graphics");
    }

    fn core_record(relationship: u32, size: u32) -> Vec<u8> {
        let mut record = vec![0u8; size as usize];
        record[0..4].copy_from_slice(&relationship.to_le_bytes());
        record[4..8].copy_from_slice(&size.to_le_bytes());
        record
    }

    #[test]
    fn core_records_are_counted() {
        let mut buffer = Vec::new();
        buffer.extend(core_record(0, 48));
        buffer.extend(core_record(0, 48));
        buffer.extend(core_record(1, 32));
        buffer.extend(core_record(0, 48));
        assert_eq!(count_core_records(&buffer), 3);
        assert_eq!(count_core_records(&[]), 0);
        assert_eq!(count_core_records(&[0, 0, 0, 0, 0, 0, 0, 0]), 0);
        // A truncated or hostile buffer must not hang or panic.
        assert_eq!(count_core_records(&[0, 0, 0, 0, 255, 255, 255, 255]), 0);
    }

    #[test]
    fn data_serializes_with_camel_case_names() {
        let json = serde_json::to_string(&SystemInfo::default()).unwrap();
        assert_eq!(
            json,
            r#"{"os":null,"cpu":null,"gpus":[],"memory":{"total":null,"modules":[]},"drives":[],"battery":null,"uptime":null}"#
        );
        let battery = BatteryInfo {
            health_percent: Some(84),
            detail: "x".to_string(),
        };
        assert_eq!(
            serde_json::to_string(&battery).unwrap(),
            r#"{"healthPercent":84,"detail":"x"}"#
        );
    }
}
