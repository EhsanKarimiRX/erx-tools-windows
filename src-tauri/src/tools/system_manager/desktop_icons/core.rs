//! Pure Desktop Icons logic: the five classic system icons, their CLSIDs and
//! the visibility rule. No registry or Windows API here — that lives in
//! `registry.rs` behind a trait, so everything in this file is unit-testable.
//!
//! The registry key (HKCU only, no admin):
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\HideDesktopIcons\NewStartPanel`
//! One REG_DWORD per icon CLSID: 1 = hidden, 0 = shown. A MISSING value does
//! NOT mean "shown" — it means "the Windows default for that icon" (the
//! common bug this module exists to avoid). Writing always sets an explicit
//! 0 or 1 and never deletes a value.

/// The five classic desktop icons the section can show or hide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DesktopIcon {
    ThisPc,
    RecycleBin,
    ControlPanel,
    Network,
    UserFiles,
}

impl DesktopIcon {
    pub const ALL: [DesktopIcon; 5] = [
        DesktopIcon::ThisPc,
        DesktopIcon::RecycleBin,
        DesktopIcon::ControlPanel,
        DesktopIcon::Network,
        DesktopIcon::UserFiles,
    ];

    /// Stable id the frontend sends (never a path or a raw CLSID).
    pub fn id(self) -> &'static str {
        match self {
            DesktopIcon::ThisPc => "this_pc",
            DesktopIcon::RecycleBin => "recycle_bin",
            DesktopIcon::ControlPanel => "control_panel",
            DesktopIcon::Network => "network",
            DesktopIcon::UserFiles => "user_files",
        }
    }

    pub fn from_id(id: &str) -> Option<DesktopIcon> {
        DesktopIcon::ALL
            .iter()
            .copied()
            .find(|icon| icon.id() == id)
    }

    /// The registry value name (the icon's CLSID, canonical casing as used
    /// by Windows). Verified live — see docs/PROMPT_5_PROGRESS.md.
    pub fn clsid(self) -> &'static str {
        match self {
            DesktopIcon::ThisPc => "{20D04FE0-3AEA-1069-A2D8-08002B30309D}",
            DesktopIcon::RecycleBin => "{645FF040-5081-101B-9F08-00AA002F954E}",
            DesktopIcon::ControlPanel => "{5399E694-6CE5-4D6D-8792-F8F34E1E2E6F}",
            DesktopIcon::Network => "{F02C1A0D-BE21-4350-88B0-7367FC96EF3C}",
            DesktopIcon::UserFiles => "{59031a47-3f72-44a7-89c5-5595fe6b30ee}",
        }
    }

    /// Row label for the UI.
    pub fn label(self) -> &'static str {
        match self {
            DesktopIcon::ThisPc => "This PC",
            DesktopIcon::RecycleBin => "Recycle Bin",
            DesktopIcon::ControlPanel => "Control Panel",
            DesktopIcon::Network => "Network",
            DesktopIcon::UserFiles => "User's Files (your user folder)",
        }
    }

    /// What a MISSING registry value means: the Windows default per icon.
    /// Only the Recycle Bin is shown on a fresh account; the rest are hidden.
    pub fn default_visible(self) -> bool {
        matches!(self, DesktopIcon::RecycleBin)
    }
}

/// Visibility from a raw registry value (spec 5c): Some(0) = shown,
/// Some(_) = hidden, None = the Windows default for that icon.
pub fn is_visible(value: Option<u32>, icon: DesktopIcon) -> bool {
    match value {
        Some(0) => true,
        Some(_) => false,
        None => icon.default_visible(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clsid_table_has_five_unique_entries() {
        let mut clsids: Vec<&str> = DesktopIcon::ALL.iter().map(|icon| icon.clsid()).collect();
        let count = clsids.len();
        clsids.sort_unstable();
        clsids.dedup();
        assert_eq!(clsids.len(), count, "five distinct CLSIDs");
        for clsid in &clsids {
            assert!(clsid.starts_with('{') && clsid.ends_with('}'), "{clsid}");
        }
    }

    #[test]
    fn ids_round_trip_and_are_unique() {
        let mut ids: Vec<&str> = DesktopIcon::ALL.iter().map(|icon| icon.id()).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count);
        for icon in DesktopIcon::ALL {
            assert_eq!(DesktopIcon::from_id(icon.id()), Some(icon));
        }
        assert_eq!(DesktopIcon::from_id("bogus"), None);
    }

    #[test]
    fn visibility_rules_all_icons_all_values() {
        for icon in DesktopIcon::ALL {
            assert!(is_visible(Some(0), icon), "{icon:?} value 0 is shown");
            assert!(!is_visible(Some(1), icon), "{icon:?} value 1 is hidden");
            assert!(
                !is_visible(Some(42), icon),
                "{icon:?} any nonzero value is hidden"
            );
            assert_eq!(
                is_visible(None, icon),
                icon.default_visible(),
                "{icon:?} absent value means the Windows default"
            );
        }
        // Exactly one icon defaults to visible.
        assert_eq!(
            DesktopIcon::ALL
                .iter()
                .filter(|icon| icon.default_visible())
                .count(),
            1
        );
        assert!(DesktopIcon::RecycleBin.default_visible());
    }
}
