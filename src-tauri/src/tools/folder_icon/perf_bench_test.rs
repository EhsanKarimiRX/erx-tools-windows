//! PERF-TRACE (temporary): micro-benchmarks for the Folder Icon delay
//! investigation. Every test is `#[ignore]`; run on demand with:
//!   cargo test --manifest-path src-tauri/Cargo.toml perf_ -- --ignored --nocapture --test-threads=1
//! Prints `[PERF] <name>: <ms> ms` lines. Touches only temporary folders
//! under the system temp dir (the eligibility rules allow those, same as
//! the existing apply.rs unit tests) and cleans up after itself.
//! NOTE: runs real Explorer notifications once (notify tests) — the
//! desktop/Explorer windows may flicker briefly while this runs.

#[cfg(test)]
mod perf_bench {
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_NORMAL, SetFileAttributesW,
    };
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_Contacts, FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads,
        FOLDERID_Favorites, FOLDERID_Links, FOLDERID_LocalAppData, FOLDERID_Music,
        FOLDERID_Pictures, FOLDERID_Profile, FOLDERID_ProgramData, FOLDERID_ProgramFiles,
        FOLDERID_ProgramFilesX86, FOLDERID_RoamingAppData, FOLDERID_SavedGames,
        FOLDERID_SearchHome, FOLDERID_SkyDrive, FOLDERID_UserProfiles, FOLDERID_Videos,
        FOLDERID_Windows, SHGetKnownFolderPath,
    };
    use windows_sys::core::{GUID, PWSTR};

    use super::super::core;
    use super::super::settings::IconSettings;

    // ----- helpers -----------------------------------------------------

    /// `#RRGGBB`/pattern/size/opacity values come straight from
    /// settings.rs's accepted tokens (validated there, so `expect` is fine).
    fn default_settings() -> IconSettings {
        IconSettings::from_parts("#0078D4", "none", "#FFFFFF", 3, 70).expect("valid default")
    }

    fn none_settings() -> IconSettings {
        // (b) "pattern none" design — note: identical to the default design,
        // which also has no pattern; measured separately as requested.
        IconSettings::from_parts("#0078D4", "none", "#FFFFFF", 3, 70).expect("valid none")
    }

    fn checker_settings() -> IconSettings {
        // (c) heaviest pattern at its smallest step: checker, size 1
        // (4 px canvas cell → 930 pattern shapes per drawing).
        IconSettings::from_parts("#0078D4", "checker", "#FFFFFF", 1, 100)
            .expect("valid checker")
    }

    fn ms(start: &Instant) -> f64 {
        start.elapsed().as_secs_f64() * 1000.0
    }

    /// One cold call + 20 warm calls, printed as `[PERF]` lines.
    fn bench(cold_name: &str, warm_name: &str, mut f: impl FnMut()) {
        let start = Instant::now();
        f();
        println!("[PERF] {cold_name}: {:.3} ms", ms(&start));
        let mut total = 0.0f64;
        for _ in 0..20 {
            let start = Instant::now();
            f();
            total += ms(&start);
        }
        println!("[PERF] {warm_name}: {:.3} ms", total / 20.0);
    }

    fn make_perf_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "erx-perf-{}-{}-{}",
            tag,
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(&path).expect("temp dir");
        path
    }

    /// Removes the temp tree (clears attributes first, like the apply.rs
    /// test cleanup, because applied folders become Read-only and their
    /// files Hidden+System).
    fn cleanup_perf(path: &Path) {
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
                    cleanup_perf(&entry.path());
                } else {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        let _ = std::fs::remove_dir(path);
    }

    // PERF-TRACE (temporary): replica of platform.rs's private
    // `known_folder` (same body) so the known-folder calls can be timed
    // WITHOUT touching platform.rs.
    fn known_folder_call(guid: &GUID) -> Option<PathBuf> {
        unsafe {
            let mut path: PWSTR = std::ptr::null_mut();
            let result = SHGetKnownFolderPath(guid, 0, std::ptr::null_mut(), &mut path);
            if result != 0 || path.is_null() {
                return None;
            }
            let mut len = 0usize;
            while *path.add(len) != 0 {
                len += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(path, len));
            CoTaskMemFree(path.cast());
            Some(PathBuf::from(text))
        }
    }

    // PERF-TRACE (temporary): one GetDriveTypeW call, timed.
    fn drive_type_call_timed(letter: u8) -> f64 {
        let root = format!(r"{}:\", letter as char);
        let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
        let start = Instant::now();
        unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(wide.as_ptr()) };
        ms(&start)
    }

    // ----- 1. gather_environment ----------------------------------------

    #[test]
    #[ignore]
    fn perf_gather_environment() {
        bench(
            "gather_environment cold",
            "gather_environment warm avg(20)",
            || {
                let _ = super::super::platform::gather_environment();
            },
        );

        // PERF-TRACE (temporary): the known-folder half, replicated with the
        // SAME 22 calls gather_environment makes (15 + Roaming parent +
        // 3 system + 2 root + SkyDrive again).
        bench("known_folder x22 cold", "known_folder x22 warm avg(20)", || {
            let known: [&GUID; 15] = [
                &FOLDERID_Profile,
                &FOLDERID_Desktop,
                &FOLDERID_Documents,
                &FOLDERID_Downloads,
                &FOLDERID_Music,
                &FOLDERID_Pictures,
                &FOLDERID_Videos,
                &FOLDERID_Favorites,
                &FOLDERID_Links,
                &FOLDERID_SavedGames,
                &FOLDERID_SearchHome,
                &FOLDERID_Contacts,
                &FOLDERID_RoamingAppData,
                &FOLDERID_LocalAppData,
                &FOLDERID_SkyDrive,
            ];
            for guid in known {
                let _ = known_folder_call(guid);
            }
            if let Some(appdata) = known_folder_call(&FOLDERID_RoamingAppData) {
                let _ = appdata.parent().map(Path::to_path_buf);
            }
            for guid in [&FOLDERID_Windows, &FOLDERID_ProgramFiles, &FOLDERID_ProgramFilesX86] {
                let _ = known_folder_call(guid);
            }
            for guid in [&FOLDERID_ProgramData, &FOLDERID_UserProfiles] {
                let _ = known_folder_call(guid);
            }
            let _ = known_folder_call(&FOLDERID_SkyDrive);
        });

        // PERF-TRACE (temporary): the drive-type half, 26 letters.
        bench("drive_type x26 cold", "drive_type x26 warm avg(20)", || {
            for letter in b'A'..=b'Z' {
                let _ = drive_type_call_timed(letter);
            }
        });

        // Per-letter warm averages; print any letter over 5 ms.
        let mut sums = [0.0f64; 26];
        for _ in 0..20 {
            for (i, letter) in (b'A'..=b'Z').enumerate() {
                sums[i] += drive_type_call_timed(letter);
            }
        }
        for (i, letter) in (b'A'..=b'Z').enumerate() {
            let avg = sums[i] / 20.0;
            if avg > 5.0 {
                println!(
                    "[PERF] drive {}: {:.3} ms warm avg  (OVER 5 ms)",
                    letter as char, avg
                );
            }
        }
    }

    // ----- 2. resolve (canonicalize) ------------------------------------

    #[test]
    #[ignore]
    fn perf_resolve_canonicalize() {
        let dir = make_perf_dir("resolve");
        bench("resolve cold", "resolve warm avg(20)", || {
            let _ = super::super::eligibility::resolve(&dir);
        });
        cleanup_perf(&dir);
    }

    // ----- 3. core::build ------------------------------------------------

    #[test]
    #[ignore]
    fn perf_core_build() {
        bench("core_build default cold", "core_build default warm avg(20)", || {
            let _ = core::build(&default_settings());
        });
        bench(
            "core_build pattern-none cold",
            "core_build pattern-none warm avg(20)",
            || {
                let _ = core::build(&none_settings());
            },
        );
        bench(
            "core_build checker-step1 cold",
            "core_build checker-step1 warm avg(20)",
            || {
                let _ = core::build(&checker_settings());
            },
        );
    }

    // ----- 4. build_ico ---------------------------------------------------

    #[test]
    #[ignore]
    fn perf_build_ico() {
        let default_params = core::build(&default_settings()).params;
        let none_params = core::build(&none_settings()).params;
        let checker_params = core::build(&checker_settings()).params;
        bench("build_ico default cold", "build_ico default warm avg(20)", || {
            let _ = super::super::ico::build_ico(&default_params);
        });
        bench(
            "build_ico pattern-none cold",
            "build_ico pattern-none warm avg(20)",
            || {
                let _ = super::super::ico::build_ico(&none_params);
            },
        );
        bench(
            "build_ico checker-step1 cold",
            "build_ico checker-step1 warm avg(20)",
            || {
                let _ = super::super::ico::build_ico(&checker_params);
            },
        );
    }

    // ----- 5. render_frame_png + base64 -----------------------------------

    #[test]
    #[ignore]
    fn perf_render_frame_png() {
        let default_params = core::build(&default_settings()).params;
        let none_params = core::build(&none_settings()).params;
        let checker_params = core::build(&checker_settings()).params;
        let designs: [(&str, core::IconParams); 3] = [
            ("default", default_params),
            ("pattern-none", none_params),
            ("checker-step1", checker_params),
        ];
        for (design, params) in designs {
            for size in [16u32, 32, 48] {
                bench(
                    &format!("render_frame_png {size}px {design} cold"),
                    &format!("render_frame_png {size}px {design} warm avg(20)"),
                    || {
                        let _ = super::super::raster::render_frame_png(&params, size);
                    },
                );
            }
        }
        // The base64 data-URL step the preview command does per frame.
        let png = super::super::raster::render_frame_png(&default_params, 48).expect("png");
        bench(
            "base64 data_url (48px png) cold",
            "base64 data_url (48px png) warm avg(20)",
            || {
                let _ = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png);
            },
        );
    }

    // ----- 6. apply_icon over 1 / 10 / 50 temp folders ---------------------

    #[test]
    #[ignore]
    fn perf_apply_icon_folders() {
        // PERF-TRACE (temporary): enable the existing dev trace for the
        // 1-folder per-step timeline (debug builds only). Edition 2024 makes
        // set_var unsafe; the bench runs with --test-threads=1.
        unsafe {
            std::env::set_var("ERX_FOLDER_ICON_TRACE", "1");
        }
        let base = make_perf_dir("apply50");
        let params = core::build(&default_settings()).params;
        let mut total = 0.0f64;
        let mut total10 = 0.0f64;
        for i in 0..50usize {
            let dir = base.join(format!("f{i:03}"));
            std::fs::create_dir_all(&dir).expect("folder");
            let start = Instant::now();
            super::super::apply::apply_icon(&dir, &params).expect("apply ok");
            let elapsed = ms(&start);
            total += elapsed;
            if i < 10 {
                total10 += elapsed;
            }
            if i == 0 {
                println!("[PERF] apply_icon folder 1 (cold): {elapsed:.3} ms");
            }
        }
        println!("[PERF] apply_icon avg over 10 folders: {:.3} ms", total10 / 10.0);
        println!("[PERF] apply_icon avg over 50 folders: {:.3} ms", total / 50.0);
        unsafe {
            std::env::remove_var("ERX_FOLDER_ICON_TRACE");
        }
        cleanup_perf(&base);
    }

    // ----- 7. restore_icon over 1 / 10 temp folders ------------------------

    #[test]
    #[ignore]
    fn perf_restore_icon() {
        let base = make_perf_dir("restore10");
        let params = core::build(&default_settings()).params;
        let mut dirs = Vec::new();
        for i in 0..10usize {
            let dir = base.join(format!("f{i:03}"));
            std::fs::create_dir_all(&dir).expect("folder");
            super::super::apply::apply_icon(&dir, &params).expect("apply ok");
            dirs.push(dir);
        }
        let mut total = 0.0f64;
        for (i, dir) in dirs.iter().enumerate() {
            let start = Instant::now();
            super::super::apply::restore_icon(dir).expect("restore ok");
            let elapsed = ms(&start);
            total += elapsed;
            if i == 0 {
                println!("[PERF] restore_icon folder 1 (cold): {elapsed:.3} ms");
            }
        }
        println!("[PERF] restore_icon avg over 10 folders: {:.3} ms", total / 10.0);
        cleanup_perf(&base);
    }

    // ----- 8. eligibility pre-check path (batch_step / apply_icon do this) --

    #[test]
    #[ignore]
    fn perf_check_eligibility_path() {
        let dir = make_perf_dir("eligibility");
        bench(
            "eligibility path (resolve+env+facts+evaluate) cold",
            "eligibility path (resolve+env+facts+evaluate) warm avg(20)",
            || {
                let resolved = super::super::eligibility::resolve(&dir);
                let env = super::super::platform::gather_environment();
                let facts = super::super::platform::path_facts(&resolved);
                let _ = super::super::eligibility::evaluate(&resolved, facts, &env);
            },
        );
        cleanup_perf(&dir);
    }

    // ----- 9. notifications (real Explorer calls, once each) ----------------

    #[test]
    #[ignore]
    fn perf_notifications() {
        let dir = make_perf_dir("notify");
        let start = Instant::now();
        super::super::notification::notify_folder_now(&dir);
        println!("[PERF] notify_folder_now (call time): {:.3} ms", ms(&start));

        let start = Instant::now();
        super::super::notification::notify_all_associations();
        println!(
            "[PERF] notify_all_associations (call time): {:.3} ms",
            ms(&start)
        );

        let start = Instant::now();
        let posted = super::super::notification::refresh_shell_views();
        println!(
            "[PERF] refresh_shell_views (call time, posted to {posted} views): {:.3} ms",
            ms(&start)
        );
        cleanup_perf(&dir);
    }

    // ----- 10. read_dir scan cost with 10,000 files --------------------------

    #[test]
    #[ignore]
    fn perf_read_dir_10k_files() {
        let dir = make_perf_dir("readdir10k");
        for i in 0..10_000usize {
            std::fs::write(dir.join(format!("erx-perf-{i:05}.tmp")), b"").expect("file");
        }
        // PERF-TRACE (temporary): same loop shape as apply.rs's private
        // delete_our_icon_files (read_dir + file_name string + our-name
        // pattern check), without deleting anything.
        let scan = || {
            let mut count = 0usize;
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if super::super::apply::is_our_icon_file_name(&name) {
                        count += 1;
                    }
                }
            }
            count
        };
        let start = Instant::now();
        let found = scan();
        println!("[PERF] read_dir 10k files cold: {:.3} ms", ms(&start));
        let mut total = 0.0f64;
        for _ in 0..5 {
            let start = Instant::now();
            scan();
            total += ms(&start);
        }
        println!(
            "[PERF] read_dir 10k files warm avg(5): {:.3} ms (matched {found} names)",
            total / 5.0
        );
        cleanup_perf(&dir);
    }
}
