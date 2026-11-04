//! TEMPORARY live-flow experiment (not committed): reproduces the REAL UI
//! command sequence — inspect each folder, then run the real batch command
//! function — and probes for leftover handles afterwards, including the
//! exact exclusive-open/rename checks from probe.rs. If this passes, the
//! holder (if any) is outside these code paths and the diagnostic must run
//! in the real UI process.

#[cfg(test)]
mod live_flow_probe {
    use crate::tools::folder_icon::core::IconParams;
    use crate::tools::folder_icon::probe::{try_exclusive_open, try_folder_rename};
    use std::path::Path;

    fn params() -> crate::tools::folder_icon::core::IconParams {
        use crate::tools::folder_icon::colors::Rgb;
        use crate::tools::folder_icon::settings::PatternKind;
        IconParams {
            base_color: Rgb {
                r: 0,
                g: 0x78,
                b: 0xd4,
            },
            back_color: Rgb {
                r: 0,
                g: 0x40,
                b: 0x80,
            },
            pattern: PatternKind::None,
            pattern_color: Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
            pattern_size: 3,
            pattern_opacity: 1.0,
        }
    }

    #[test]
    fn real_command_flow_leaves_no_handles() {
        // 1. Simulate the UI flow on a fresh folder: inspect, then batch.
        let base = std::env::temp_dir().join(format!(
            "erx-live-probe-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        let folder = base.join("fresh");
        std::fs::create_dir_all(&folder).expect("folder");

        // folder_icon_inspect body (same calls as the command):
        let inspect = crate::tools::folder_icon::commands::folder_icon_inspect(
            folder.to_string_lossy().into_owned(),
        )
        .expect("inspect");
        assert_eq!(inspect.state, "default");

        // folder_icon_apply_batch body for ONE folder (same calls as the
        // command, including the Channel-less summary path):
        let dto: crate::tools::folder_icon::commands::FolderIconSettingsDto =
            serde_json::from_str(
                r##"{"baseColor":"#0078D4","pattern":"none","patternColor":"#FFFFFF","patternSize":3,"patternOpacity":70}"##,
            )
            .unwrap();
        let settings = crate::tools::folder_icon::settings::IconSettings::from_parts(
            &dto.base_color,
            &dto.pattern,
            &dto.pattern_color,
            dto.pattern_size,
            dto.pattern_opacity,
        )
        .unwrap();
        let built = crate::tools::folder_icon::core::build(&settings);
        crate::tools::folder_icon::apply::apply_icon(&folder, &built.params).expect("apply");

        // 2. Probe: exclusive opens + folder rename AFTER the real flow.
        let ico_name = crate::tools::folder_icon::apply::icon_file_name(
            &crate::tools::folder_icon::ico::build_ico(&params()).unwrap(),
        );
        let results = [
            (
                "desktop.ini",
                try_exclusive_open(&folder.join("desktop.ini")),
            ),
            ("the .ico", try_exclusive_open(&folder.join(&ico_name))),
            ("the folder", try_folder_rename(&folder)),
        ];
        for (label, result) in results {
            assert_eq!(result, Ok(true), "{label} must not be held");
        }

        // 3. Also probe the PARENT (Explorer watches parents) and a
        //    re-inspect afterwards (the UI would call inspect again on
        //    list changes).
        assert_eq!(
            try_folder_rename(&base),
            Ok(true),
            "parent must not be held"
        );
        let _ = crate::tools::folder_icon::commands::folder_icon_inspect(
            folder.to_string_lossy().into_owned(),
        );
        assert_eq!(
            try_exclusive_open(&folder.join("desktop.ini")),
            Ok(true),
            "desktop.ini must not be held after a second inspect"
        );

        // cleanup
        crate::tools::folder_icon::apply::restore_icon(&folder).expect("restore");
        clear_and_remove(&base);
    }

    fn clear_and_remove(path: &Path) {
        use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, SetFileAttributesW};
        let clear = |p: &Path| {
            let wide: Vec<u16> = p
                .as_os_str()
                .to_string_lossy()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            unsafe { SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_NORMAL) };
        };
        clear(path);
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                clear(&entry.path());
                if entry.path().is_dir() {
                    clear_and_remove(&entry.path());
                } else {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        let _ = std::fs::remove_dir(path);
    }
}
