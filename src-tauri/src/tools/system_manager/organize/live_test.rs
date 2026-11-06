//! Live Organize smoke test — READ-ONLY (run on purpose: cargo test -- --ignored).
//! Resolves the REAL Desktop and Downloads folders and scans them, but
//! never writes, moves or deletes anything (rule G9: the real folders are
//! only ever touched by the owner, by hand).

use super::core::OrganizeTarget;
use super::scan;

#[test]
#[ignore]
fn live_read_only_scan_of_the_real_desktop_and_downloads() {
    for target in [OrganizeTarget::Desktop, OrganizeTarget::Downloads] {
        let root = scan::known_folder(target).expect("known folder resolves");
        assert!(root.is_dir(), "{:?} exists", target);
        let plan = scan::scan_dir(&root, target, &super::modes::Mode::Format, &mut |_: u32| {})
            .expect("read-only scan works");
        println!(
            "{:?} at {}: {} file(s) would move into {} folder(s), {} skipped, {} left in place, OneDrive: {}",
            target,
            plan.root.display(),
            plan.categories.iter().map(|c| c.files.len()).sum::<usize>(),
            plan.categories.len(),
            plan.skipped.len(),
            plan.left_in_place,
            plan.on_onedrive,
        );
    }
}
