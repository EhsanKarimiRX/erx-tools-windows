//! Graphics cards, read through DXGI (the Windows graphics infrastructure).
//! Software adapters and Microsoft's "Basic Render Driver" are skipped. The
//! card with the most own memory comes first.

use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
};

use super::core::{NamedDetail, gpu_detail};

/// Vendor id of Microsoft's software renderer.
const VENDOR_MICROSOFT: u32 = 0x1414;
const MAX_ADAPTERS: u32 = 16;

pub fn gpu_list() -> Vec<NamedDetail> {
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return Vec::new();
    };
    let mut found: Vec<(u64, String)> = Vec::new();
    for index in 0..MAX_ADAPTERS {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(index) }) else {
            break;
        };
        let Ok(description) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };
        if description.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
            continue;
        }
        if description.VendorId == VENDOR_MICROSOFT {
            continue;
        }
        let length = description
            .Description
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(description.Description.len());
        let name = String::from_utf16_lossy(description.Description.get(..length).unwrap_or(&[]));
        let name = name.trim().to_string();
        if name.is_empty() {
            continue;
        }
        found.push((description.DedicatedVideoMemory as u64, name));
    }
    found.sort_by_key(|a| std::cmp::Reverse(a.0));
    found
        .into_iter()
        .map(|(memory, name)| NamedDetail {
            name,
            detail: gpu_detail(memory),
        })
        .collect()
}
