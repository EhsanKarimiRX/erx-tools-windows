//! Tests of the clipboard module. The pure logic runs against a fake
//! clipboard; the one test that touches the real Windows clipboard is
//! `#[ignore]` (c14).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW,
};
use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

use super::clipboard::{
    CLEAR_AFTER, ClearDecision, ClearPlan, ClipboardApi, ClipboardCore, ClipboardError,
    MARKER_FORMAT_NAMES, MAX_TEXT_BYTES, OPEN_ATTEMPTS, OPEN_DELAY, POLL_INTERVAL, PollOutcome,
    WinClipboard, decide, encode_text, error_to_command_error,
};
use super::commands::NotesCommandError;
use super::webprofile::random_hex;

/// One call the fake clipboard saw.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Write(Vec<u16>),
    Sequence,
    Clear,
}

/// What the fake clipboard returns and what it saw.
#[derive(Default)]
struct FakeState {
    calls: Vec<Call>,
    sequence: u32,
    write_error: Option<ClipboardError>,
    clear_error: Option<ClipboardError>,
}

/// A clipboard that records calls instead of touching Windows.
struct FakeApi {
    state: Arc<Mutex<FakeState>>,
}

impl ClipboardApi for FakeApi {
    fn write_text(&mut self, utf16: &[u16]) -> Result<u32, ClipboardError> {
        let mut state = self.state.lock().unwrap();
        state.calls.push(Call::Write(utf16.to_vec()));
        match state.write_error {
            Some(error) => Err(error),
            None => Ok(state.sequence),
        }
    }

    fn current_sequence(&mut self) -> u32 {
        let mut state = self.state.lock().unwrap();
        state.calls.push(Call::Sequence);
        state.sequence
    }

    fn clear(&mut self) -> Result<(), ClipboardError> {
        let mut state = self.state.lock().unwrap();
        state.calls.push(Call::Clear);
        match state.clear_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

/// Builds a core with a fake clipboard that reports `sequence`.
fn fake(sequence: u32) -> (Arc<Mutex<FakeState>>, ClipboardCore) {
    let state = Arc::new(Mutex::new(FakeState {
        sequence,
        ..FakeState::default()
    }));
    let api = FakeApi {
        state: Arc::clone(&state),
    };
    (state, ClipboardCore::new(Box::new(api)))
}

/// Counts the writes the fake saw.
fn writes(state: &Arc<Mutex<FakeState>>) -> Vec<Vec<u16>> {
    state
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter_map(|call| match call {
            Call::Write(units) => Some(units.clone()),
            _ => None,
        })
        .collect()
}

/// Counts the clears the fake saw.
fn clears(state: &Arc<Mutex<FakeState>>) -> usize {
    state
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter(|call| matches!(call, Call::Clear))
        .count()
}

/// Sets the sequence number the fake reports next.
fn set_sequence(state: &Arc<Mutex<FakeState>>, sequence: u32) {
    state.lock().unwrap().sequence = sequence;
}

#[test]
fn c1_encode_basic() {
    assert_eq!(&*encode_text("abc").unwrap(), &[97, 98, 99, 0]);

    let persian = "می‌خواهم";
    let mut expected: Vec<u16> = persian.encode_utf16().collect();
    expected.push(0);
    assert_eq!(&*encode_text(persian).unwrap(), expected.as_slice());

    assert_eq!(&*encode_text("😀").unwrap(), &[0xD83D, 0xDE00, 0]);
}

#[test]
fn c2_encode_rejects() {
    assert_eq!(encode_text(""), Err(ClipboardError::InvalidText));
    assert_eq!(encode_text("a\0b"), Err(ClipboardError::InvalidText));

    let biggest = "a".repeat(MAX_TEXT_BYTES);
    let encoded = encode_text(&biggest).unwrap();
    assert_eq!(encoded.len(), MAX_TEXT_BYTES + 1);
    assert_eq!(*encoded.last().unwrap(), 0);

    let too_big = "a".repeat(MAX_TEXT_BYTES + 1);
    assert_eq!(encode_text(&too_big), Err(ClipboardError::InvalidText));
}

#[test]
fn c3_decide_table() {
    let t0 = Instant::now();
    let plan = ClearPlan {
        sequence: 7,
        deadline: t0 + CLEAR_AFTER,
    };

    assert_eq!(
        decide(&plan, t0 + Duration::from_secs(29), 7),
        ClearDecision::Wait
    );
    assert_eq!(decide(&plan, t0 + CLEAR_AFTER, 7), ClearDecision::Clear);
    assert_eq!(
        decide(&plan, t0 + Duration::from_secs(31), 7),
        ClearDecision::Clear
    );
    assert_eq!(
        decide(&plan, t0 + Duration::from_secs(29), 8),
        ClearDecision::Forget
    );
    assert_eq!(
        decide(&plan, t0 + Duration::from_secs(31), 8),
        ClearDecision::Forget
    );
}

#[test]
fn c4_copy_sets_plan() {
    let t0 = Instant::now();
    let (state, core) = fake(7);

    assert_eq!(core.copy("abc", t0), Ok(()));
    assert_eq!(writes(&state), vec![vec![97, 98, 99, 0]]);

    assert_eq!(
        core.poll(t0 + Duration::from_secs(29)),
        PollOutcome::Waiting
    );
    assert_eq!(clears(&state), 0);

    assert_eq!(core.poll(t0 + CLEAR_AFTER), PollOutcome::Cleared);
    assert_eq!(clears(&state), 1);
}

#[test]
fn c5_poll_flow() {
    let t0 = Instant::now();

    let (state, core) = fake(7);
    assert_eq!(core.poll(t0), PollOutcome::Idle);
    assert!(state.lock().unwrap().calls.is_empty());

    assert_eq!(core.copy("abc", t0), Ok(()));
    assert_eq!(core.poll(t0 + CLEAR_AFTER), PollOutcome::Cleared);
    let calls_after_clear = state.lock().unwrap().calls.len();
    assert_eq!(
        core.poll(t0 + CLEAR_AFTER + POLL_INTERVAL),
        PollOutcome::Idle
    );
    assert_eq!(state.lock().unwrap().calls.len(), calls_after_clear);
    assert_eq!(clears(&state), 1);

    let (state, core) = fake(7);
    assert_eq!(core.copy("abc", t0), Ok(()));
    set_sequence(&state, 8);
    assert_eq!(core.poll(t0 + Duration::from_secs(5)), PollOutcome::Forgot);
    assert_eq!(clears(&state), 0);
    assert_eq!(core.poll(t0 + Duration::from_secs(6)), PollOutcome::Idle);
}

#[test]
fn c6_second_copy_replaces_plan() {
    let t0 = Instant::now();
    let (state, core) = fake(7);

    assert_eq!(core.copy("abc", t0), Ok(()));
    set_sequence(&state, 9);
    assert_eq!(core.copy("xyz", t0 + Duration::from_secs(20)), Ok(()));

    assert_eq!(
        core.poll(t0 + Duration::from_secs(30)),
        PollOutcome::Waiting
    );
    assert_eq!(clears(&state), 0);
    assert_eq!(
        core.poll(t0 + Duration::from_secs(50)),
        PollOutcome::Cleared
    );
}

#[test]
fn c7_failed_write_keeps_old_plan() {
    let t0 = Instant::now();
    let (state, core) = fake(7);

    assert_eq!(core.copy("abc", t0), Ok(()));
    state.lock().unwrap().write_error = Some(ClipboardError::Busy);
    assert_eq!(
        core.copy("xyz", t0 + Duration::from_secs(1)),
        Err(ClipboardError::Busy)
    );
    assert_eq!(core.poll(t0 + CLEAR_AFTER), PollOutcome::Cleared);

    let (state, core) = fake(7);
    assert_eq!(core.copy("", t0), Err(ClipboardError::InvalidText));
    assert!(state.lock().unwrap().calls.is_empty());
}

#[test]
fn c8_clear_failure_keeps_plan() {
    let t0 = Instant::now();
    let (state, core) = fake(7);

    assert_eq!(core.copy("abc", t0), Ok(()));
    state.lock().unwrap().clear_error = Some(ClipboardError::Failed);
    assert_eq!(core.poll(t0 + CLEAR_AFTER), PollOutcome::ClearFailed);
    assert_eq!(clears(&state), 1);

    state.lock().unwrap().clear_error = None;
    assert_eq!(
        core.poll(t0 + CLEAR_AFTER + POLL_INTERVAL),
        PollOutcome::Cleared
    );
    assert_eq!(clears(&state), 2);
}

#[test]
fn c9_clear_now() {
    let t0 = Instant::now();

    let (state, core) = fake(7);
    assert!(!core.clear_now());
    assert!(state.lock().unwrap().calls.is_empty());

    let (state, core) = fake(7);
    assert_eq!(core.copy("abc", t0), Ok(()));
    assert!(core.clear_now());
    assert_eq!(clears(&state), 1);
    assert!(!core.clear_now());

    let (state, core) = fake(7);
    assert_eq!(core.copy("abc", t0), Ok(()));
    set_sequence(&state, 8);
    assert!(!core.clear_now());
    assert_eq!(clears(&state), 0);
    assert!(!core.clear_now());

    let (state, core) = fake(7);
    assert_eq!(core.copy("abc", t0), Ok(()));
    state.lock().unwrap().clear_error = Some(ClipboardError::Failed);
    assert!(!core.clear_now());
    state.lock().unwrap().clear_error = None;
    let calls = state.lock().unwrap().calls.len();
    assert_eq!(core.poll(t0), PollOutcome::Idle);
    assert_eq!(state.lock().unwrap().calls.len(), calls);
    assert!(!core.clear_now());
}

#[test]
fn c10_constants() {
    assert_eq!(CLEAR_AFTER, Duration::from_secs(30));
    assert_eq!(OPEN_ATTEMPTS, 10);
    assert_eq!(OPEN_DELAY, Duration::from_millis(20));
    assert_eq!(POLL_INTERVAL, Duration::from_secs(1));
    assert_eq!(MAX_TEXT_BYTES, 4_194_304);
    assert_eq!(
        MARKER_FORMAT_NAMES,
        [
            "CanIncludeInClipboardHistory",
            "CanUploadToCloudClipboard",
            "ExcludeClipboardContentFromMonitorProcessing",
        ]
    );
}

#[test]
fn c11_error_mapping() {
    let cases = [
        (
            ClipboardError::InvalidText,
            "clipboard_invalid_text",
            "There is no valid text to copy.",
        ),
        (
            ClipboardError::Busy,
            "clipboard_busy",
            "The clipboard is in use by another program. Try again.",
        ),
        (
            ClipboardError::Failed,
            "clipboard_failed",
            "Could not copy to the clipboard.",
        ),
    ];

    for (error, code, message) in cases {
        let mapped = error_to_command_error(error);
        assert_eq!(
            mapped,
            NotesCommandError {
                code: code.to_string(),
                message: message.to_string(),
            }
        );
        assert!(mapped.code.starts_with("clipboard_"));
        assert!(!mapped.code.is_empty());
        assert!(
            mapped
                .code
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '_')
        );
    }

    assert_ne!(
        error_to_command_error(ClipboardError::InvalidText).code,
        error_to_command_error(ClipboardError::Busy).code
    );
    assert_ne!(
        error_to_command_error(ClipboardError::Busy).code,
        error_to_command_error(ClipboardError::Failed).code
    );
    assert_ne!(
        error_to_command_error(ClipboardError::InvalidText).code,
        error_to_command_error(ClipboardError::Failed).code
    );
}

#[test]
fn c12_source_scan_clipboard() {
    let source = include_str!("clipboard.rs");
    assert_eq!(source.matches("#[tauri::command").count(), 2);
    assert_eq!(source.matches("pub async fn notes_copy_text(").count(), 1);
    assert_eq!(source.matches("pub async fn notes_copy_words(").count(), 1);
    assert_eq!(
        source.matches("check_window_label(window.label())").count(),
        2
    );
    assert_eq!(source.matches("status().unlocked").count(), 1);
    assert_eq!(source.matches("words_text()").count(), 1);
    assert!(source.matches("spawn_blocking").count() >= 2);
    assert!(source.matches("EmptyClipboard()").count() >= 3);
    for name in MARKER_FORMAT_NAMES {
        assert_eq!(source.matches(name).count(), 1);
    }
    assert!(source.matches("CF_UNICODETEXT").count() >= 1);
    assert!(source.matches("CloseClipboard()").count() >= 3);
}

#[test]
fn c13_source_scan_lib_and_commands() {
    let lib = include_str!("../../lib.rs");
    assert_eq!(
        lib.matches("tools::notes::clipboard::notes_copy_text,")
            .count(),
        1
    );
    assert_eq!(
        lib.matches("tools::notes::clipboard::notes_copy_words,")
            .count(),
        1
    );
    assert_eq!(
        lib.matches(".manage(tools::notes::clipboard::NotesClipboardState::default())")
            .count(),
        1
    );
    assert_eq!(lib.matches("tools::notes::clipboard::init(").count(), 1);
    assert_eq!(lib.matches("tools::notes::clipboard::on_exit(").count(), 1);

    let commands = include_str!("commands.rs");
    assert_eq!(
        commands
            .matches("pub(crate) fn shared(&self) -> Arc<NotesShared>")
            .count(),
        1
    );
    assert_eq!(commands.matches("#[tauri::command").count(), 20);
}

/// Opens the clipboard for reading, with the same retry as the module.
fn open_for_read() -> bool {
    for attempt in 0..OPEN_ATTEMPTS {
        // SAFETY: a null owner window asks for the clipboard without an owner.
        if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
            return true;
        }
        if attempt + 1 < OPEN_ATTEMPTS {
            std::thread::sleep(OPEN_DELAY);
        }
    }
    false
}

/// Reads the Unicode text that is on the clipboard right now.
fn read_clipboard_text() -> Vec<u16> {
    assert!(open_for_read());
    let mut units = Vec::new();
    // SAFETY: the clipboard is open; the handle is borrowed for this block.
    unsafe {
        let handle = GetClipboardData(CF_UNICODETEXT as u32);
        assert!(!handle.is_null());
        let pointer = GlobalLock(handle) as *const u16;
        assert!(!pointer.is_null());
        let mut index = 0usize;
        loop {
            let unit = *pointer.add(index);
            if unit == 0 {
                break;
            }
            units.push(unit);
            index += 1;
            assert!(index < 1_000_000);
        }
        GlobalUnlock(handle);
        CloseClipboard();
    }
    units
}

#[test]
#[ignore = "uses the real Windows clipboard"]
fn c14_real_clipboard_roundtrip() {
    let text = format!("ERX-CLIPBOARD-TEST-{}", random_hex(8).unwrap());
    let utf16 = encode_text(&text).unwrap();

    let mut clipboard = WinClipboard;
    let sequence = clipboard.write_text(&utf16).unwrap();
    assert_ne!(sequence, 0);
    assert_eq!(clipboard.current_sequence(), sequence);

    assert_eq!(
        read_clipboard_text(),
        text.encode_utf16().collect::<Vec<_>>()
    );

    for name in MARKER_FORMAT_NAMES {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `wide` is a valid null-terminated UTF-16 string.
        let id = unsafe { RegisterClipboardFormatW(wide.as_ptr()) };
        assert_ne!(id, 0);
        // SAFETY: no pointers, reads the clipboard state.
        assert_ne!(unsafe { IsClipboardFormatAvailable(id) }, 0);
    }

    assert_eq!(clipboard.clear(), Ok(()));
    // SAFETY: no pointers, reads the clipboard state.
    assert_eq!(
        unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT as u32) },
        0
    );
}
