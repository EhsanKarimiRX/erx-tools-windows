//! Reads the installed RAM modules out of the raw SMBIOS table (the BIOS
//! description of the PC). The input comes from the Windows firmware table
//! API: an 8-byte header (calling method, major, minor, revision, then the
//! table length as u32) followed by the SMBIOS structures. The parser never
//! panics: anything unexpected simply ends the walk.

use super::bytes::{u8_at, u16_at, u32_at};

/// One RAM module that is really installed (empty slots are skipped).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryModule {
    pub size_mb: u32,
    /// SMBIOS memory type code (0x22 = DDR5, 0x1A = DDR4, ...).
    pub type_code: u8,
    /// Configured speed, or the rated speed when the configured one is missing.
    pub speed_mts: Option<u32>,
}

const MAX_STRUCTURES: u32 = 10_000;
const TYPE_MEMORY_DEVICE: u8 = 17;
const TYPE_END_OF_TABLE: u8 = 127;

/// Walks the structure list and collects every installed memory device.
pub fn parse_memory_modules(raw: &[u8]) -> Vec<MemoryModule> {
    let mut modules = Vec::new();
    let Some(declared) = u32_at(raw, 4) else {
        return modules;
    };
    let Some(body) = raw.get(8..) else {
        return modules;
    };
    let end = body
        .len()
        .min(usize::try_from(declared).unwrap_or(usize::MAX));
    let Some(table) = body.get(..end) else {
        return modules;
    };
    let mut position = 0usize;
    for _ in 0..MAX_STRUCTURES {
        let Some(kind) = u8_at(table, position) else {
            break;
        };
        let Some(length) = u8_at(table, position + 1) else {
            break;
        };
        let length = usize::from(length);
        if length < 4 {
            break;
        }
        let Some(structure_end) = position.checked_add(length) else {
            break;
        };
        let Some(formatted) = table.get(position..structure_end) else {
            break;
        };
        if kind == TYPE_END_OF_TABLE {
            break;
        }
        if kind == TYPE_MEMORY_DEVICE
            && let Some(module) = parse_memory_device(formatted)
        {
            modules.push(module);
        }
        // The text strings of the structure follow it and end with two NUL bytes.
        let mut cursor = structure_end;
        loop {
            match (u8_at(table, cursor), u8_at(table, cursor + 1)) {
                (Some(0), Some(0)) => {
                    cursor += 2;
                    break;
                }
                (Some(_), Some(_)) => cursor += 1,
                _ => return modules,
            }
        }
        position = cursor;
    }
    modules
}

/// Reads one "memory device" structure (type 17). None = empty slot or
/// unusable data.
fn parse_memory_device(formatted: &[u8]) -> Option<MemoryModule> {
    let size_field = u16_at(formatted, 0x0C)?;
    let size_mb = match size_field {
        // 0 = no module installed, 0xFFFF = size unknown.
        0 | 0xFFFF => return None,
        // 0x7FFF = the real size is in the extended size field (megabytes).
        0x7FFF => u32_at(formatted, 0x1C)? & 0x7FFF_FFFF,
        // Bit 15 set = the size is given in kilobytes.
        value if value & 0x8000 != 0 => u32::from(value & 0x7FFF).div_ceil(1024),
        value => u32::from(value),
    };
    if size_mb == 0 {
        return None;
    }
    let type_code = u8_at(formatted, 0x12).unwrap_or(0);
    let usable = |speed: &u16| *speed != 0 && *speed != 0xFFFF;
    let configured = u16_at(formatted, 0x20).filter(usable);
    let rated = u16_at(formatted, 0x15).filter(usable);
    Some(MemoryModule {
        size_mb,
        type_code,
        speed_mts: configured.or(rated).map(u32::from),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A type 17 structure (34 bytes) followed by one text and the NUL end.
    fn memory_device(size_field: u16, type_code: u8, speed: u16, extended: u32) -> Vec<u8> {
        let mut data = vec![0u8; 0x22];
        data[0] = 17;
        data[1] = 0x22;
        data[0x0C..0x0E].copy_from_slice(&size_field.to_le_bytes());
        data[0x12] = type_code;
        data[0x15..0x17].copy_from_slice(&speed.to_le_bytes());
        data[0x1C..0x20].copy_from_slice(&extended.to_le_bytes());
        data[0x20..0x22].copy_from_slice(&speed.to_le_bytes());
        data.extend_from_slice(b"DIMM A\0\0");
        data
    }

    /// Another structure type (1) with two texts, to be skipped correctly.
    fn other_structure() -> Vec<u8> {
        let mut data = vec![0u8; 8];
        data[0] = 1;
        data[1] = 8;
        data.extend_from_slice(b"ACME\0Model X\0\0");
        data
    }

    fn wrap(structures: &[Vec<u8>]) -> Vec<u8> {
        let mut body: Vec<u8> = Vec::new();
        for structure in structures {
            body.extend_from_slice(structure);
        }
        body.extend_from_slice(&[127, 4, 0, 0, 0, 0]);
        let mut raw = vec![0u8, 3, 3, 0];
        raw.extend_from_slice(&(body.len() as u32).to_le_bytes());
        raw.extend_from_slice(&body);
        raw
    }

    #[test]
    fn installed_modules_are_listed_and_empty_slots_skipped() {
        let raw = wrap(&[
            other_structure(),
            memory_device(16_384, 0x22, 5_600, 0),
            memory_device(0, 0x22, 0, 0),
            other_structure(),
            memory_device(8_192, 0x1A, 3_200, 0),
        ]);
        let modules = parse_memory_modules(&raw);
        assert_eq!(
            modules,
            vec![
                MemoryModule {
                    size_mb: 16_384,
                    type_code: 0x22,
                    speed_mts: Some(5_600)
                },
                MemoryModule {
                    size_mb: 8_192,
                    type_code: 0x1A,
                    speed_mts: Some(3_200)
                },
            ]
        );
    }

    #[test]
    fn extended_size_and_kilobyte_size_are_understood() {
        let raw = wrap(&[
            memory_device(0x7FFF, 0x22, 4_800, 32_768),
            memory_device(0x8000 | 2_048, 0x1A, 0, 0),
        ]);
        let modules = parse_memory_modules(&raw);
        assert_eq!(modules.len(), 2);
        assert_eq!(modules[0].size_mb, 32_768);
        assert_eq!(modules[1].size_mb, 2);
    }

    #[test]
    fn unknown_speed_is_none() {
        let raw = wrap(&[memory_device(4_096, 0x18, 0xFFFF, 0)]);
        let modules = parse_memory_modules(&raw);
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].speed_mts, None);
        let raw = wrap(&[memory_device(4_096, 0x18, 0, 0)]);
        assert_eq!(parse_memory_modules(&raw)[0].speed_mts, None);
    }

    #[test]
    fn broken_input_never_panics() {
        assert!(parse_memory_modules(&[]).is_empty());
        assert!(parse_memory_modules(&[0, 3, 3]).is_empty());
        let good = wrap(&[memory_device(16_384, 0x22, 5_600, 0)]);
        for cut in 0..good.len() {
            let _ = parse_memory_modules(&good[..cut]);
        }
        // Declared length larger than the data.
        let mut long_claim = good.clone();
        long_claim[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(parse_memory_modules(&long_claim).len(), 1);
        // A structure that claims a length of 0 or 3 ends the walk.
        let bad = vec![0u8, 3, 3, 0, 8, 0, 0, 0, 17, 0, 0, 0, 0, 0, 0, 0];
        assert!(parse_memory_modules(&bad).is_empty());
    }

    #[test]
    fn random_bytes_never_panic() {
        let mut state: u32 = 12345;
        for round in 0..2_000u32 {
            let length = (round % 300) as usize;
            let mut data = Vec::with_capacity(length);
            for _ in 0..length {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                data.push((state >> 24) as u8);
            }
            let _ = parse_memory_modules(&data);
        }
    }
}
