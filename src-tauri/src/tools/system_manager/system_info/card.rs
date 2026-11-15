//! Turns the raw SystemInfo into the exact texts of the six card cells and
//! the "More" list. Pure logic, covered by unit tests; nothing here touches
//! Windows.

use serde::Serialize;

use super::core::{BatteryInfo, MemoryInfo, ModuleInfo, NamedDetail, SEP, SystemInfo};

const INTEGRATED: &str = "Integrated graphics";

/// One cell of the card: the main line and the small line under it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardCell {
    pub value: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoreItem {
    pub name: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoreGroup {
    pub title: String,
    pub items: Vec<MoreItem>,
}

/// Everything the UI needs. None = "Not reported" (or: this PC has no
/// battery). `more` is empty when there is nothing extra to list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemCard {
    pub processor: Option<CardCell>,
    pub graphics: Option<CardCell>,
    pub memory: Option<CardCell>,
    pub storage: Option<CardCell>,
    pub os: Option<CardCell>,
    pub battery: Option<BatteryInfo>,
    pub uptime: Option<String>,
    pub more: Vec<MoreGroup>,
}

/// Removes the trademark signs Windows puts into graphics card names.
fn clean_gpu_name(raw: &str) -> String {
    let mut text = raw.to_string();
    for junk in ["(R)", "(r)", "(TM)", "(tm)", "(C)"] {
        text = text.replace(junk, "");
    }
    text.split_whitespace().collect::<Vec<&str>>().join(" ")
}

/// True for the graphics that sit inside the processor. A guess from the
/// name and the memory text: Intel without Arc, AMD "Radeon ... Graphics".
pub fn is_integrated_gpu(gpu: &NamedDetail) -> bool {
    if gpu.detail == INTEGRATED {
        return true;
    }
    let name = gpu.name.to_lowercase();
    if name.contains("intel") {
        return !name.contains("arc");
    }
    name.contains("radeon")
        && name.contains("graphics")
        && !name.contains(" rx ")
        && !name.contains("pro")
}

/// Cleans the names and puts discrete cards before processor graphics (the
/// order inside each group stays: most own memory first).
fn ordered_gpus(gpus: &[NamedDetail]) -> Vec<NamedDetail> {
    let mut tagged: Vec<(bool, NamedDetail)> = gpus
        .iter()
        .map(|gpu| {
            let integrated = is_integrated_gpu(gpu);
            let detail = if integrated {
                INTEGRATED.to_string()
            } else {
                gpu.detail.clone()
            };
            (
                integrated,
                NamedDetail {
                    name: clean_gpu_name(&gpu.name),
                    detail,
                },
            )
        })
        .collect();
    tagged.sort_by_key(|(integrated, _)| *integrated);
    tagged.into_iter().map(|(_, gpu)| gpu).collect()
}

/// The memory type ("DDR5") at the start of a module detail line, if any.
pub fn module_kind(detail: &str) -> Option<&str> {
    detail.split(SEP).next().filter(|part| part.contains("DDR"))
}

/// "16 GB", "2 x 16 GB" (all equal), "16 GB + 8 GB" (different).
pub fn layout_text(modules: &[ModuleInfo]) -> String {
    let Some(first) = modules.first() else {
        return String::new();
    };
    if modules.len() > 1 && modules.iter().all(|module| module.size == first.size) {
        return format!("{} \u{00D7} {}", modules.len(), first.size);
    }
    modules
        .iter()
        .map(|module| module.size.as_str())
        .collect::<Vec<&str>>()
        .join(" + ")
}

fn single(item: Option<&NamedDetail>) -> Option<CardCell> {
    item.map(|item| CardCell {
        value: item.name.clone(),
        detail: item.detail.clone(),
    })
}

/// First item of a list; "+N more" is added to the detail when there are more.
fn first_with_more(list: &[NamedDetail]) -> Option<CardCell> {
    let first = list.first()?;
    let mut detail = first.detail.clone();
    let extra = list.len().saturating_sub(1);
    if extra > 0 {
        if !detail.is_empty() {
            detail.push_str(SEP);
        }
        detail.push_str(&format!("+{extra} more"));
    }
    Some(CardCell {
        value: first.name.clone(),
        detail,
    })
}

fn memory_cell(memory: &MemoryInfo) -> Option<CardCell> {
    let total = memory.total.as_ref()?;
    let kind = memory
        .modules
        .first()
        .and_then(|module| module_kind(&module.detail));
    let value = match kind {
        Some(kind) => format!("{kind}{SEP}{total} Total"),
        None => format!("{total} Total"),
    };
    Some(CardCell {
        value,
        detail: layout_text(&memory.modules),
    })
}

fn item_from_named(item: &NamedDetail) -> MoreItem {
    MoreItem {
        name: item.name.clone(),
        detail: item.detail.clone(),
    }
}

fn push_group(groups: &mut Vec<MoreGroup>, title: &str, items: Vec<MoreItem>) {
    if !items.is_empty() {
        groups.push(MoreGroup {
            title: title.to_string(),
            items,
        });
    }
}

fn more_groups(
    gpus: &[NamedDetail],
    memory: &MemoryInfo,
    drives: &[NamedDetail],
) -> Vec<MoreGroup> {
    let mut groups = Vec::new();
    push_group(
        &mut groups,
        "Graphics",
        gpus.iter().map(item_from_named).collect(),
    );
    push_group(
        &mut groups,
        "Memory",
        memory
            .modules
            .iter()
            .enumerate()
            .map(|(index, module)| MoreItem {
                name: format!("Module {}: {}", index + 1, module.size),
                detail: module.detail.clone(),
            })
            .collect(),
    );
    push_group(
        &mut groups,
        "Storage",
        drives.iter().map(item_from_named).collect(),
    );
    groups
}

pub fn build_card(info: &SystemInfo) -> SystemCard {
    let gpus = ordered_gpus(&info.gpus);
    let has_extra = gpus.len() > 1 || info.drives.len() > 1 || info.memory.modules.len() > 2;
    SystemCard {
        processor: single(info.cpu.as_ref()),
        graphics: first_with_more(&gpus),
        memory: memory_cell(&info.memory),
        storage: first_with_more(&info.drives),
        os: single(info.os.as_ref()),
        battery: info.battery.clone(),
        uptime: info.uptime.clone(),
        more: if has_extra {
            more_groups(&gpus, &info.memory, &info.drives)
        } else {
            Vec::new()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str, detail: &str) -> NamedDetail {
        NamedDetail {
            name: name.to_string(),
            detail: detail.to_string(),
        }
    }

    fn module(size: &str, detail: &str) -> ModuleInfo {
        ModuleInfo {
            size: size.to_string(),
            detail: detail.to_string(),
        }
    }

    fn laptop() -> SystemInfo {
        SystemInfo {
            os: Some(named(
                "Windows 11 Pro",
                "64-bit (x64) \u{00B7} 24H2 \u{00B7} build 26100",
            )),
            cpu: Some(named(
                "13th Gen Intel Core i7-13620H",
                "10 cores \u{00B7} 16 threads",
            )),
            gpus: vec![
                named("Intel(R) UHD Graphics", "Integrated graphics"),
                named("NVIDIA GeForce RTX 4060 Laptop GPU", "8 GB"),
            ],
            memory: MemoryInfo {
                total: Some("32 GB".to_string()),
                modules: vec![
                    module("16 GB", "DDR5 \u{00B7} 5600 MT/s"),
                    module("16 GB", "DDR5 \u{00B7} 5600 MT/s"),
                ],
            },
            drives: vec![named("Samsung SSD 990 PRO 1TB", "NVMe SSD \u{00B7} 1 TB")],
            battery: Some(BatteryInfo {
                health_percent: Some(84),
                detail: "312 charge cycles \u{00B7} Good".to_string(),
            }),
            uptime: Some("3 d 4 h".to_string()),
        }
    }

    #[test]
    fn laptop_card_matches_the_design() {
        let card = build_card(&laptop());
        assert_eq!(
            card.processor,
            Some(CardCell {
                value: "13th Gen Intel Core i7-13620H".to_string(),
                detail: "10 cores \u{00B7} 16 threads".to_string(),
            })
        );
        assert_eq!(
            card.graphics,
            Some(CardCell {
                value: "NVIDIA GeForce RTX 4060 Laptop GPU".to_string(),
                detail: format!("8 GB{SEP}+1 more"),
            })
        );
        assert_eq!(
            card.memory,
            Some(CardCell {
                value: format!("DDR5{SEP}32 GB Total"),
                detail: "2 \u{00D7} 16 GB".to_string(),
            })
        );
        assert_eq!(
            card.storage,
            Some(CardCell {
                value: "Samsung SSD 990 PRO 1TB".to_string(),
                detail: "NVMe SSD \u{00B7} 1 TB".to_string(),
            })
        );
        assert_eq!(
            card.os.as_ref().map(|cell| cell.value.as_str()),
            Some("Windows 11 Pro")
        );
        assert_eq!(
            card.battery
                .as_ref()
                .and_then(|battery| battery.health_percent),
            Some(84)
        );
        let titles: Vec<&str> = card.more.iter().map(|group| group.title.as_str()).collect();
        assert_eq!(titles, vec!["Graphics", "Memory", "Storage"]);
        assert_eq!(
            card.more[0].items[0].name,
            "NVIDIA GeForce RTX 4060 Laptop GPU"
        );
        assert_eq!(card.more[0].items[1].name, "Intel UHD Graphics");
        assert_eq!(card.more[1].items[0].name, "Module 1: 16 GB");
    }

    #[test]
    fn processor_graphics_alone_is_shown_with_its_own_name() {
        let info = SystemInfo {
            gpus: vec![named("Intel(R) UHD Graphics", "Integrated graphics")],
            ..SystemInfo::default()
        };
        let card = build_card(&info);
        assert_eq!(
            card.graphics,
            Some(CardCell {
                value: "Intel UHD Graphics".to_string(),
                detail: "Integrated graphics".to_string(),
            })
        );
        assert!(card.more.is_empty());
    }

    #[test]
    fn amd_discrete_card_comes_before_amd_processor_graphics() {
        let info = SystemInfo {
            gpus: vec![
                named("AMD Radeon(TM) Graphics", "2 GB"),
                named("AMD Radeon RX 7600M XT", "8 GB"),
            ],
            ..SystemInfo::default()
        };
        let card = build_card(&info);
        assert_eq!(
            card.graphics,
            Some(CardCell {
                value: "AMD Radeon RX 7600M XT".to_string(),
                detail: format!("8 GB{SEP}+1 more"),
            })
        );
        assert_eq!(card.more[0].items[1].name, "AMD Radeon Graphics");
        assert_eq!(card.more[0].items[1].detail, "Integrated graphics");
        assert!(!is_integrated_gpu(&named(
            "Intel(R) Arc(TM) A370M Graphics",
            "4 GB"
        )));
    }

    #[test]
    fn desktop_without_battery_has_uptime() {
        let mut info = laptop();
        info.battery = None;
        let card = build_card(&info);
        assert!(card.battery.is_none());
        assert_eq!(card.uptime, Some("3 d 4 h".to_string()));
    }

    #[test]
    fn nothing_reported_gives_empty_cells() {
        let card = build_card(&SystemInfo::default());
        assert_eq!(card, SystemCard::default());
        assert!(card.more.is_empty());
    }

    #[test]
    fn ram_texts() {
        assert_eq!(layout_text(&[]), "");
        assert_eq!(layout_text(&[module("16 GB", "")]), "16 GB");
        assert_eq!(
            layout_text(&[module("16 GB", ""), module("16 GB", "")]),
            "2 \u{00D7} 16 GB"
        );
        assert_eq!(
            layout_text(&[
                module("8 GB", ""),
                module("8 GB", ""),
                module("8 GB", ""),
                module("8 GB", "")
            ]),
            "4 \u{00D7} 8 GB"
        );
        assert_eq!(
            layout_text(&[module("16 GB", ""), module("8 GB", "")]),
            "16 GB + 8 GB"
        );
        assert_eq!(module_kind("DDR5 \u{00B7} 5600 MT/s"), Some("DDR5"));
        assert_eq!(module_kind("5600 MT/s"), None);
        assert_eq!(module_kind(""), None);
        let info = SystemInfo {
            memory: MemoryInfo {
                total: Some("8 GB".to_string()),
                modules: Vec::new(),
            },
            ..SystemInfo::default()
        };
        assert_eq!(
            build_card(&info).memory,
            Some(CardCell {
                value: "8 GB Total".to_string(),
                detail: String::new(),
            })
        );
    }

    #[test]
    fn more_list_appears_only_with_extra_items() {
        let mut info = laptop();
        info.gpus = vec![named("NVIDIA GeForce RTX 4060", "8 GB")];
        assert!(build_card(&info).more.is_empty());
        info.memory.modules.push(module("8 GB", "DDR5"));
        assert_eq!(build_card(&info).more.len(), 3);
        let mut info = laptop();
        info.gpus = vec![named("NVIDIA GeForce RTX 4060", "8 GB")];
        info.drives
            .push(named("WD Blue 1TB", "SATA HDD \u{00B7} 1 TB"));
        assert_eq!(build_card(&info).more.len(), 3);
    }

    #[test]
    fn card_serializes_with_camel_case_names() {
        let json = serde_json::to_string(&SystemCard::default()).unwrap();
        assert_eq!(
            json,
            r#"{"processor":null,"graphics":null,"memory":null,"storage":null,"os":null,"battery":null,"uptime":null,"more":[]}"#
        );
    }
}
