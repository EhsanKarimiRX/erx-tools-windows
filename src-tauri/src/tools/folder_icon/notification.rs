//! All Shell notification strategies for the Folder Icon tool in ONE place
//! (spec 4.1-A6): what is sent, when, and why.
//!
//! Strategies, in the order they are used:
//!
//! 1. **Immediate per-folder notification** (`notify_folder_now`, sent
//!    right after the last file/attribute operation, all handles closed):
//!    SHCNE_UPDATEITEM + SHCNE_ATTRIBUTES + SHCNE_UPDATEDIR for the
//!    folder, SHCNE_UPDATEDIR for its parent — each event in BOTH forms,
//!    path (`SHCNF_PATHW`) and PIDL (`SHCNF_IDLIST`, PIDL from
//!    `SHParseDisplayName`), with `SHCNF_FLUSH` so the notification is
//!    delivered before we continue. UPDATEITEM/ATTRIBUTES tell the Shell
//!    the item's icon-relevant state changed; UPDATEDIR refreshes the
//!    views showing it.
//! 2. **Deferred per-folder notification** (`schedule_folder_notify`,
//!    same events after `DEFERRED_NOTIFY_DELAY_MS`): Explorer views
//!    sometimes miss the first notification (the folder's Read-only
//!    attribute changes at the same time as its content, which is a race
//!    the owner observed on Windows 11 — the next Apply "pushed" the
//!    previous folder's refresh). The retry runs on a dedicated worker
//!    thread: pushing a job never blocks the UI or the batch loop, the
//!    queue is bounded (`DEFERRED_QUEUE_CAP`, oldest dropped, duplicates
//!    coalesced), and the thread is a daemon — the process exiting (app
//!    close) stops it.
//! 3. **Global association-changed broadcast** (`notify_all_associations`):
//!    SHCNE_ASSOCCHANGED with `SHCNF_IDLIST | SHCNF_FLUSH`. Heavy — every
//!    Explorer window rebuilds its icon associations and views may flicker
//!    briefly — so it is sent ONCE per finished batch (never per folder)
//!    and on demand from the "Refresh icons" button.
//!
//! The `SHCNF_*` values are verified against windows-sys 0.61 and the
//! SHChangeNotify documentation: the PIDL form is `SHCNF_IDLIST` (0) —
//! there is no `SHCNF_PIDW` in the current flag set.

use std::collections::VecDeque;
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use windows_sys::Win32::UI::Shell::Common::ITEMIDLIST;
use windows_sys::Win32::UI::Shell::{
    ILFree, SHCNE_ASSOCCHANGED, SHCNE_ATTRIBUTES, SHCNE_UPDATEDIR, SHCNE_UPDATEITEM, SHCNF_FLUSH,
    SHCNF_IDLIST, SHCNF_PATHW, SHChangeNotify, SHParseDisplayName,
};

/// Delay before the deferred second notification (spec 4.1-A3: start at
/// about 300 ms).
pub const DEFERRED_NOTIFY_DELAY_MS: u64 = 300;

/// Bound for the deferred queue; beyond this the oldest entries are
/// dropped (the batch's global broadcast covers them anyway).
pub const DEFERRED_QUEUE_CAP: usize = 64;

/// Sends the full immediate per-folder notification set (blocking, with
/// flush). Called after every file operation for the folder is complete.
pub fn notify_folder_now(folder: &Path) {
    send_folder_events(folder);
}

/// Schedules the same notification set again after the delay, on the
/// worker thread. Never blocks the caller.
pub fn schedule_folder_notify(folder: std::path::PathBuf) {
    worker().schedule(folder);
}

/// Sends the global association-changed broadcast once (batch end or the
/// "Refresh icons" button). May cause a brief Explorer flicker.
pub fn notify_all_associations() {
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED as i32,
            SHCNF_IDLIST,
            ptr::null(),
            ptr::null(),
        );
    }
}

/// The full per-folder event set, in path and PIDL form, with flush.
fn send_folder_events(folder: &Path) {
    // Path form: PCWSTR items.
    let wide_folder = wide(folder);
    unsafe {
        for event in [SHCNE_UPDATEITEM, SHCNE_ATTRIBUTES] {
            SHChangeNotify(
                event as i32,
                SHCNF_PATHW | SHCNF_FLUSH,
                wide_folder.as_ptr().cast(),
                ptr::null(),
            );
        }
        SHChangeNotify(
            SHCNE_UPDATEDIR as i32,
            SHCNF_PATHW | SHCNF_FLUSH,
            wide_folder.as_ptr().cast(),
            ptr::null(),
        );
    }
    if let Some(parent) = folder.parent() {
        let wide_parent = wide(parent);
        unsafe {
            SHChangeNotify(
                SHCNE_UPDATEDIR as i32,
                SHCNF_PATHW | SHCNF_FLUSH,
                wide_parent.as_ptr().cast(),
                ptr::null(),
            );
        }
    }

    // PIDL form: the same events with PIDL items (some views only react
    // to IDLIST-form notifications).
    if let Some(pidl) = pidl_of(folder) {
        unsafe {
            for event in [SHCNE_UPDATEITEM, SHCNE_ATTRIBUTES] {
                SHChangeNotify(
                    event as i32,
                    SHCNF_IDLIST | SHCNF_FLUSH,
                    pidl.cast(),
                    ptr::null(),
                );
            }
            SHChangeNotify(
                SHCNE_UPDATEDIR as i32,
                SHCNF_IDLIST | SHCNF_FLUSH,
                pidl.cast(),
                ptr::null(),
            );
            ILFree(pidl);
        }
    }
    if let Some(parent) = folder.parent()
        && let Some(pidl) = pidl_of(parent)
    {
        unsafe {
            SHChangeNotify(
                SHCNE_UPDATEDIR as i32,
                SHCNF_IDLIST | SHCNF_FLUSH,
                pidl.cast(),
                ptr::null(),
            );
            ILFree(pidl);
        }
    }
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

/// The absolute PIDL for a path (freed with `ILFree` by the caller).
fn pidl_of(path: &Path) -> Option<*mut ITEMIDLIST> {
    let name = wide(path);
    let mut pidl: *mut ITEMIDLIST = ptr::null_mut();
    let result = unsafe {
        SHParseDisplayName(
            name.as_ptr(),
            ptr::null_mut(),
            &mut pidl,
            0,
            ptr::null_mut(),
        )
    };
    if result == 0 && !pidl.is_null() {
        Some(pidl)
    } else {
        None
    }
}

// ----- deferred worker (spec 4.1-A3) -----

struct DeferredQueue {
    jobs: Mutex<VecDeque<std::path::PathBuf>>,
    signal: Condvar,
}

fn worker() -> &'static DeferredQueue {
    static QUEUE: OnceLock<DeferredQueue> = OnceLock::new();
    static SPAWNED: AtomicUsize = AtomicUsize::new(0);
    let queue = QUEUE.get_or_init(DeferredQueue::new);
    // Spawn the worker thread exactly once, lazily, on first use.
    if SPAWNED.fetch_add(1, Ordering::SeqCst) == 0 {
        let spawned = queue;
        let started = thread::Builder::new()
            .name("erx-folder-icon-deferred-notify".into())
            .spawn(move || spawned.run());
        if started.is_err() {
            // No worker thread: reset so a later call retries the spawn.
            SPAWNED.store(0, Ordering::SeqCst);
        }
    }
    queue
}

impl DeferredQueue {
    fn new() -> Self {
        Self {
            jobs: Mutex::new(VecDeque::new()),
            signal: Condvar::new(),
        }
    }

    /// Pushes a job (dedup by path, bounded queue). Never blocks.
    fn schedule(&self, folder: std::path::PathBuf) {
        let mut jobs = self.jobs.lock().expect("deferred queue lock");
        // A newer entry for the same folder supersedes an older one.
        if let Some(existing) = jobs.iter().position(|job| same_path(job, &folder)) {
            jobs.remove(existing);
        }
        if jobs.len() >= DEFERRED_QUEUE_CAP {
            jobs.pop_front();
        }
        jobs.push_back(folder);
        drop(jobs);
        self.signal.notify_one();
    }

    /// The worker loop: take a job, wait out the delay, send the same
    /// notification set again. Exits when the process exits (daemon).
    fn run(&'static self) {
        loop {
            let folder = {
                let mut jobs = self.jobs.lock().expect("deferred queue lock");
                loop {
                    match jobs.pop_front() {
                        Some(job) => break job,
                        None => jobs = self.signal.wait(jobs).expect("deferred queue wait"),
                    }
                }
            };
            thread::sleep(Duration::from_millis(DEFERRED_NOTIFY_DELAY_MS));
            send_folder_events(&folder);
        }
    }
}

/// Case-insensitive path equality (Windows file systems are
/// case-insensitive; matching the rest of the tool's comparisons).
fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}

/// Number of queued deferred jobs (tests and diagnostics).
#[cfg(test)]
fn pending_count() -> usize {
    worker().jobs.lock().expect("deferred queue lock").len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> std::path::PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("erx-notify-{tag}-{}-{n}", std::process::id()))
    }

    #[test]
    fn scheduling_is_non_blocking_even_for_many_jobs() {
        let start = std::time::Instant::now();
        for _ in 0..200 {
            schedule_folder_notify(temp_path("nonblocking"));
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(1),
            "scheduling must never block: {elapsed:?}"
        );
    }

    #[test]
    fn queue_is_bounded_and_deduplicates() {
        // Fill far beyond the cap.
        for _ in 0..(DEFERRED_QUEUE_CAP + 40) {
            schedule_folder_notify(temp_path("cap"));
        }
        assert!(
            pending_count() <= DEFERRED_QUEUE_CAP,
            "queue must stay bounded"
        );
        // Duplicate paths coalesce into one entry.
        let folder = temp_path("dedupe");
        schedule_folder_notify(folder.clone());
        let before = pending_count();
        schedule_folder_notify(folder.clone());
        schedule_folder_notify(folder);
        assert_eq!(pending_count(), before, "duplicates must coalesce");
    }

    #[test]
    fn pidl_resolution_works_for_real_paths() {
        let folder = std::env::temp_dir();
        let pidl = pidl_of(&folder);
        assert!(pidl.is_some(), "temp dir must resolve to a PIDL");
        if let Some(pidl) = pidl {
            unsafe { ILFree(pidl) };
        }
        // A nonexistent path simply yields None (no panic).
        assert!(pidl_of(Path::new(r"C:\erx-nonexistent-folder-xyz")).is_none());
    }

    #[test]
    fn immediate_notification_set_is_safe_for_arbitrary_paths() {
        // Real notifications for a temp path — harmless, must not panic.
        let folder = temp_path("send");
        std::fs::create_dir_all(&folder).expect("temp dir");
        send_folder_events(&folder);
        std::fs::remove_dir(&folder).ok();
    }
}
