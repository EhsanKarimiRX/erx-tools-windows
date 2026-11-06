//! Engine tests for run + undo, in scratch directories under the system
//! temp folder (never the owner's real Desktop/Downloads — rule G9). The
//! real Desktop/Downloads resolution is exercised read-only by the owner's
//! manual test and the (read-only) app scan.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use super::core::OrganizeTarget;
use super::engine::{self};
use super::manifest;
use super::scan::{self, PlannedFile};

fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "erx-organize-run-{}-{}-{}",
        std::process::id(),
        tag,
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn planned(root: &Path, name: &str) -> PlannedFile {
    let metadata = fs::symlink_metadata(root.join(name)).expect("stat planned file");
    PlannedFile {
        name: name.to_string(),
        size: metadata.len(),
        modified_unix: scan::modified_seconds(&metadata),
    }
}

fn names(skipped: &[super::scan::SkippedFile]) -> Vec<String> {
    skipped.iter().map(|file| file.name.clone()).collect()
}

fn no_progress(_done: u32, _total: u32) {}

#[test]
fn run_moves_every_file_into_the_right_folder() {
    let dir = temp_dir("all-kinds");
    for name in [
        "a.pdf", "b.jpg", "c.mp4", "d.mp3", "e.zip", "f.exe", "g.xyz",
    ] {
        fs::write(dir.join(name), b"data").expect("write");
    }
    let files: Vec<PlannedFile> = [
        "a.pdf", "b.jpg", "c.mp4", "d.mp3", "e.zip", "f.exe", "g.xyz",
    ]
    .iter()
    .map(|name| planned(&dir, name))
    .collect();
    let manifest_path = dir.join("organize_test.jsonl");

    let report = engine::run(
        OrganizeTarget::Downloads,
        &dir,
        &files,
        &manifest_path,
        1000,
        &mut no_progress,
    )
    .expect("run");
    assert_eq!(report.moved, 7, "all seven moved");
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);

    assert!(dir.join("Documents").join("a.pdf").exists());
    assert!(dir.join("Images").join("b.jpg").exists());
    assert!(dir.join("Videos").join("c.mp4").exists());
    assert!(dir.join("Music").join("d.mp3").exists());
    assert!(dir.join("Archives").join("e.zip").exists());
    assert!(dir.join("Installers").join("f.exe").exists());
    assert!(dir.join("More").join("g.xyz").exists());
    assert!(
        !dir.join("a.pdf").exists(),
        "the original is gone (moved, not copied)"
    );

    let manifest = manifest::read(&manifest_path).expect("manifest");
    assert_eq!(manifest.moves.len(), 7);
    assert_eq!(manifest.mkdirs.len(), 7);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn run_never_overwrites_and_uses_two_suffixes() {
    let dir = temp_dir("collision");
    fs::write(dir.join("report.pdf"), b"new").expect("write");
    fs::create_dir(dir.join("Documents")).expect("mkdir");
    fs::write(dir.join("Documents").join("report.pdf"), b"PRECIOUS").expect("pre-existing");

    let files = vec![planned(&dir, "report.pdf")];
    let report = engine::run(
        OrganizeTarget::Downloads,
        &dir,
        &files,
        &dir.join("m.jsonl"),
        1,
        &mut no_progress,
    )
    .expect("run");
    assert_eq!(report.moved, 1);
    // The pre-existing file is UNCHANGED; the moved file sits next to it.
    assert_eq!(
        fs::read(dir.join("Documents").join("report.pdf")).expect("read"),
        b"PRECIOUS"
    );
    assert_eq!(
        fs::read(dir.join("Documents").join("report (2).pdf")).expect("read"),
        b"new"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn run_reuses_an_existing_category_folder() {
    let dir = temp_dir("reuse");
    fs::write(dir.join("a.txt"), b"x").expect("write");
    fs::create_dir(dir.join("Documents")).expect("mkdir");
    fs::write(dir.join("Documents").join("old.txt"), b"old").expect("write");

    let files = vec![planned(&dir, "a.txt")];
    let report = engine::run(
        OrganizeTarget::Desktop,
        &dir,
        &files,
        &dir.join("m.jsonl"),
        1,
        &mut no_progress,
    )
    .expect("run");
    assert_eq!(report.moved, 1);
    assert!(
        dir.join("Documents").join("old.txt").exists(),
        "existing content untouched"
    );
    assert!(dir.join("Documents").join("a.txt").exists());
    let manifest = manifest::read(&dir.join("m.jsonl")).expect("manifest");
    assert_eq!(manifest.mkdirs.len(), 0, "no folder was created");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn run_skips_category_whose_name_is_taken_by_a_file() {
    let dir = temp_dir("conflict");
    fs::write(dir.join("a.pdf"), b"x").expect("write");
    fs::write(dir.join("Documents"), b"a FILE named Documents").expect("write");

    let files = vec![planned(&dir, "a.pdf")];
    let report = engine::run(
        OrganizeTarget::Desktop,
        &dir,
        &files,
        &dir.join("m.jsonl"),
        1,
        &mut no_progress,
    )
    .expect("run");
    assert_eq!(report.moved, 0);
    assert_eq!(names(&report.skipped), vec!["a.pdf"]);
    assert_eq!(report.skipped[0].reason, "name conflict");
    assert!(dir.join("a.pdf").exists(), "the file was not touched");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn run_skips_a_locked_file_as_in_use() {
    use std::os::windows::fs::OpenOptionsExt;

    let dir = temp_dir("locked");
    fs::write(dir.join("busy.txt"), b"locked by another program").expect("write");
    fs::write(dir.join("free.txt"), b"free").expect("write");

    // Hold busy.txt open with NO sharing: every other open fails.
    let mut handle = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0) // exclusive
        .open(dir.join("busy.txt"))
        .expect("lock the file");

    let files = vec![planned(&dir, "busy.txt"), planned(&dir, "free.txt")];
    let report = engine::run(
        OrganizeTarget::Desktop,
        &dir,
        &files,
        &dir.join("m.jsonl"),
        1,
        &mut no_progress,
    )
    .expect("run");
    assert_eq!(report.moved, 1, "the free file moved");
    assert_eq!(names(&report.skipped), vec!["busy.txt"]);
    assert_eq!(report.skipped[0].reason, "in use or protected");
    assert!(dir.join("busy.txt").exists(), "locked file stayed");

    handle.flush().ok();
    drop(handle);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn run_skips_files_changed_since_preview() {
    let dir = temp_dir("changed");
    fs::write(dir.join("a.txt"), b"original").expect("write");
    let mut files = vec![planned(&dir, "a.txt")];

    // The file changes after the scan: different size.
    fs::write(dir.join("a.txt"), b"original, now longer").expect("rewrite");
    let report = engine::run(
        OrganizeTarget::Desktop,
        &dir,
        &files.clone(),
        &dir.join("m.jsonl"),
        1,
        &mut no_progress,
    )
    .expect("run");
    assert_eq!(report.moved, 0);
    assert_eq!(report.skipped[0].reason, "changed since preview");

    // A planned file that vanished is also skipped honestly.
    fs::remove_file(dir.join("a.txt")).expect("remove");
    files[0].modified_unix += 0;
    let report = engine::run(
        OrganizeTarget::Desktop,
        &dir,
        &files,
        &dir.join("m.jsonl"),
        2,
        &mut no_progress,
    )
    .expect("run");
    assert_eq!(report.moved, 0);
    assert_eq!(report.skipped[0].reason, "no longer there");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn manifest_records_line_by_line_and_end_line() {
    let dir = temp_dir("manifest");
    fs::write(dir.join("a.txt"), b"x").expect("write");
    let files = vec![planned(&dir, "a.txt")];
    let manifest_path = dir.join("m.jsonl");
    let _ = engine::run(
        OrganizeTarget::Desktop,
        &dir,
        &files,
        &manifest_path,
        5,
        &mut no_progress,
    )
    .expect("run");

    let text = fs::read_to_string(&manifest_path).expect("read manifest text");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4, "run + mkdir + move + end: {lines:?}");
    assert!(lines[0].contains("\"type\":\"run\""));
    assert!(lines[1].contains("\"type\":\"mkdir\""));
    assert!(lines[2].contains("\"type\":\"move\""));
    assert!(lines[3].contains("\"type\":\"end\""));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn undo_restores_everything_and_removes_only_empty_folders() {
    let dir = temp_dir("undo");
    fs::write(dir.join("a.pdf"), b"a").expect("write");
    fs::write(dir.join("b.jpg"), b"b").expect("write");
    fs::write(dir.join("c.xyz"), b"c").expect("write");
    let files = vec![
        planned(&dir, "a.pdf"),
        planned(&dir, "b.jpg"),
        planned(&dir, "c.xyz"),
    ];
    let manifest_path = dir.join("m.jsonl");
    let report = engine::run(
        OrganizeTarget::Desktop,
        &dir,
        &files,
        &manifest_path,
        1,
        &mut no_progress,
    )
    .expect("run");
    assert_eq!(report.moved, 3);

    let undo = engine::undo(&manifest_path).expect("undo");
    assert_eq!(undo.restored, 3);
    assert_eq!(undo.not_found, 0);
    assert!(undo.failed.is_empty(), "{:?}", undo.failed);
    // Desktop categories: a.pdf → Documents, b.jpg and c.xyz → More.
    assert_eq!(undo.folders_removed, 2, "both created folders were empty");
    assert!(dir.join("a.pdf").exists() && dir.join("b.jpg").exists() && dir.join("c.xyz").exists());
    assert!(
        !dir.join("Documents").exists(),
        "empty created folder removed"
    );
    assert!(!dir.join("More").exists());
    assert!(
        !manifest_path.exists(),
        "fully successful undo deletes the manifest"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn undo_keeps_folders_that_still_have_content() {
    let dir = temp_dir("undo-keep");
    fs::write(dir.join("a.pdf"), b"a").expect("write");
    let files = vec![planned(&dir, "a.pdf")];
    let manifest_path = dir.join("m.jsonl");
    engine::run(
        OrganizeTarget::Desktop,
        &dir,
        &files,
        &manifest_path,
        1,
        &mut no_progress,
    )
    .expect("run");

    // The user drops their own file into Documents before the undo.
    fs::write(dir.join("Documents").join("mine.txt"), b"user file").expect("write");

    let undo = engine::undo(&manifest_path).expect("undo");
    assert_eq!(undo.restored, 1);
    assert_eq!(undo.folders_removed, 0);
    assert_eq!(
        undo.folders_kept, 1,
        "a folder with user content is never deleted"
    );
    assert!(
        dir.join("Documents").join("mine.txt").exists(),
        "user content safe"
    );
    assert!(dir.join("a.pdf").exists());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn undo_when_the_user_deleted_the_moved_file() {
    let dir = temp_dir("undo-gone");
    fs::write(dir.join("a.pdf"), b"a").expect("write");
    let files = vec![planned(&dir, "a.pdf")];
    let manifest_path = dir.join("m.jsonl");
    engine::run(
        OrganizeTarget::Desktop,
        &dir,
        &files,
        &manifest_path,
        1,
        &mut no_progress,
    )
    .expect("run");
    fs::remove_file(dir.join("Documents").join("a.pdf")).expect("user deletes it");

    let undo = engine::undo(&manifest_path).expect("undo");
    assert_eq!(undo.not_found, 1);
    assert_eq!(undo.restored, 0);
    assert!(!manifest_path.exists(), "nothing left to undo");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn undo_when_the_original_name_is_occupied() {
    let dir = temp_dir("undo-occupied");
    fs::write(dir.join("a.pdf"), b"new one").expect("write");
    let files = vec![planned(&dir, "a.pdf")];
    let manifest_path = dir.join("m.jsonl");
    engine::run(
        OrganizeTarget::Desktop,
        &dir,
        &files,
        &manifest_path,
        1,
        &mut no_progress,
    )
    .expect("run");

    // A DIFFERENT file now sits at the original path.
    fs::write(dir.join("a.pdf"), b"a different file").expect("occupy");

    let undo = engine::undo(&manifest_path).expect("undo");
    assert_eq!(undo.restored_renamed, 1);
    // The occupant is never overwritten…
    assert_eq!(
        fs::read(dir.join("a.pdf")).expect("read"),
        b"a different file"
    );
    // …and the moved file comes back NEXT TO its original spot under (2)
    // (the manifest's "from" parent), while the Documents folder, now
    // empty, disappears.
    assert_eq!(fs::read(dir.join("a (2).pdf")).expect("read"), b"new one");
    assert!(!dir.join("Documents").exists());
    assert!(
        undo.notes.iter().any(|note| note.contains("(2)")),
        "{:?}",
        undo.notes
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn undo_works_after_a_crash_without_end_line() {
    let dir = temp_dir("undo-crash");
    fs::write(dir.join("a.pdf"), b"a").expect("write");
    let manifest_path = dir.join("m.jsonl");
    {
        // Simulate a run that died before the end line: hand-written manifest.
        let mut writer =
            manifest::start(&manifest_path, OrganizeTarget::Desktop, &dir, 1).expect("start");
        fs::create_dir(dir.join("Documents")).expect("mkdir");
        fs::rename(dir.join("a.pdf"), dir.join("Documents").join("a.pdf")).expect("move");
        writer
            .record_mkdir(&dir.join("Documents"))
            .expect("record mkdir");
        writer
            .record_move(&dir.join("a.pdf"), &dir.join("Documents").join("a.pdf"))
            .expect("record move");
        // no record_end: "crash"
    }
    let undo = engine::undo(&manifest_path).expect("undo after crash");
    assert_eq!(undo.restored, 1);
    assert_eq!(undo.folders_removed, 1);
    assert!(dir.join("a.pdf").exists());
    assert!(!manifest_path.exists());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn progress_counts_are_honest() {
    let dir = temp_dir("progress");
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(dir.join(name), b"x").expect("write");
    }
    let files: Vec<PlannedFile> = ["a.txt", "b.txt", "c.txt"]
        .iter()
        .map(|name| planned(&dir, name))
        .collect();
    let mut calls: Vec<(u32, u32)> = Vec::new();
    let _ = engine::run(
        OrganizeTarget::Desktop,
        &dir,
        &files,
        &dir.join("m.jsonl"),
        1,
        &mut |done, total| {
            calls.push((done, total));
        },
    )
    .expect("run");
    assert_eq!(calls.first(), Some(&(0, 3)));
    assert_eq!(
        calls.last(),
        Some(&(3, 3)),
        "final call reports completion, no fake percentages"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn second_run_on_same_target_replaces_the_manifest() {
    let dir = temp_dir("replace");
    fs::write(dir.join("a.txt"), b"a").expect("write");
    let manifest_path = dir.join("m.jsonl");
    let files = vec![planned(&dir, "a.txt")];
    engine::run(
        OrganizeTarget::Desktop,
        &dir,
        &files,
        &manifest_path,
        1,
        &mut no_progress,
    )
    .expect("first run");
    engine::undo(&manifest_path).expect("undo");
    fs::write(dir.join("a.txt"), b"a2").expect("rewrite");
    // Re-plan AFTER the rewrite: the run re-checks size/modified time.
    let files = vec![planned(&dir, "a.txt")];
    engine::run(
        OrganizeTarget::Desktop,
        &dir,
        &files,
        &manifest_path,
        2,
        &mut no_progress,
    )
    .expect("second run");
    let manifest = manifest::read(&manifest_path).expect("manifest");
    assert_eq!(manifest.moves.len(), 1, "only the latest run is undoable");
    assert_eq!(manifest.started_at, 2);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn custom_target_leaves_runtime_files_and_reports_them_in_the_plan() {
    let dir = temp_dir("custom");
    fs::write(dir.join("doc.pdf"), b"pdf").expect("write");
    fs::write(dir.join("prog.exe"), b"exe").expect("write");
    fs::write(dir.join("settings.ini"), b"ini").expect("write");

    let plan = scan::scan_dir(&dir, OrganizeTarget::Custom).expect("scan");
    assert_eq!(plan.total_would_move(), 1, "only Documents-type files move");
    assert_eq!(
        plan.left_in_place, 2,
        "exe and ini are left in place on purpose"
    );
    assert!(
        plan.skipped.is_empty(),
        "left-in-place is a decision, not an error"
    );

    // And the run moves only the one file.
    let files = vec![planned(&dir, "doc.pdf")];
    let report = engine::run(
        OrganizeTarget::Custom,
        &dir,
        &files,
        &dir.join("m.jsonl"),
        1,
        &mut no_progress,
    )
    .expect("run");
    assert_eq!(report.moved, 1);
    assert!(dir.join("prog.exe").exists() && dir.join("settings.ini").exists());
    assert!(dir.join("Documents").join("doc.pdf").exists());
    fs::remove_dir_all(&dir).ok();
}
