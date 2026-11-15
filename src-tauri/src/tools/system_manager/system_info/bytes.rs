//! Bounds-checked little-endian readers for raw byte buffers. They never
//! panic: a read outside the buffer simply returns None.

pub fn u8_at(data: &[u8], offset: usize) -> Option<u8> {
    data.get(offset).copied()
}

pub fn u16_at(data: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let bytes: [u8; 2] = data.get(offset..end)?.try_into().ok()?;
    Some(u16::from_le_bytes(bytes))
}

pub fn u32_at(data: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let bytes: [u8; 4] = data.get(offset..end)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

pub fn u64_at(data: &[u8], offset: usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    let bytes: [u8; 8] = data.get(offset..end)?.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

/// Reads a NUL-terminated ASCII text that starts at `offset` (at most 128
/// bytes). Offset 0 means "no text" in the Windows structures, so it gives an
/// empty string. The result is trimmed.
pub fn ascii_at(data: &[u8], offset: usize) -> String {
    if offset == 0 {
        return String::new();
    }
    let Some(tail) = data.get(offset..) else {
        return String::new();
    };
    let bytes: Vec<u8> = tail
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .take(128)
        .collect();
    String::from_utf8_lossy(&bytes).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readers_are_bounds_checked() {
        let data = [1u8, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(u8_at(&data, 0), Some(1));
        assert_eq!(u8_at(&data, 8), None);
        assert_eq!(u16_at(&data, 0), Some(0x0201));
        assert_eq!(u16_at(&data, 7), None);
        assert_eq!(u32_at(&data, 0), Some(0x0403_0201));
        assert_eq!(u32_at(&data, 5), None);
        assert_eq!(u64_at(&data, 0), Some(0x0807_0605_0403_0201));
        assert_eq!(u64_at(&data, 1), None);
        assert_eq!(u32_at(&data, usize::MAX), None);
    }

    #[test]
    fn ascii_reads_until_nul() {
        let mut data = vec![0u8; 4];
        data.extend_from_slice(b"  Samsung SSD \0junk");
        assert_eq!(ascii_at(&data, 4), "Samsung SSD");
        assert_eq!(ascii_at(&data, 0), "");
        assert_eq!(ascii_at(&data, 999), "");
    }
}
