//! Remembers whether the Secure Notes window is closed, opening or open, and
//! which profile folder belongs to it. It holds no window: the window itself
//! belongs to the Tauri layer, which asks this gate before it shows anything.

use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

/// Returned when another open is already in progress or the window is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateBusy;

pub struct WindowGate {
    state: Mutex<GateState>,
}

enum GateState {
    Closed,
    Opening(Option<PathBuf>),
    Open(PathBuf),
}

impl Default for WindowGate {
    fn default() -> WindowGate {
        WindowGate::new()
    }
}

impl WindowGate {
    pub fn new() -> WindowGate {
        WindowGate {
            state: Mutex::new(GateState::Closed),
        }
    }

    /// Closed -> Opening(None). Opening or Open -> Err(GateBusy).
    pub fn begin_open(&self) -> Result<(), GateBusy> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        match *state {
            GateState::Closed => {
                *state = GateState::Opening(None);
                Ok(())
            }
            GateState::Opening(_) | GateState::Open(_) => Err(GateBusy),
        }
    }

    /// Only in Opening(None): stores the folder, returns true. Any other state:
    /// false and nothing changes.
    pub fn attach_dir(&self, dir: PathBuf) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        match &*state {
            GateState::Opening(None) => {
                *state = GateState::Opening(Some(dir));
                true
            }
            GateState::Closed | GateState::Opening(Some(_)) | GateState::Open(_) => false,
        }
    }

    /// Only in Opening(Some(dir)): becomes Open(dir), returns true. Any other
    /// state: false and nothing changes.
    pub fn finish_open(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        match std::mem::replace(&mut *state, GateState::Closed) {
            GateState::Opening(Some(dir)) => {
                *state = GateState::Open(dir);
                true
            }
            other => {
                *state = other;
                false
            }
        }
    }

    /// From ANY state to Closed. Returns the folder if the state held one. The
    /// caller that receives Some(dir) is responsible for deleting it.
    pub fn release(&self) -> Option<PathBuf> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        match std::mem::replace(&mut *state, GateState::Closed) {
            GateState::Closed => None,
            GateState::Opening(dir) => dir,
            GateState::Open(dir) => Some(dir),
        }
    }

    /// True in Opening and Open.
    #[allow(dead_code)] // used by tests only
    pub fn is_active(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        !matches!(&*state, GateState::Closed)
    }

    /// True only in Open.
    pub fn is_open(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        matches!(&*state, GateState::Open(_))
    }

    #[cfg(test)]
    pub(crate) fn poison_for_test(&self) {
        std::thread::scope(|scope| {
            let handle = scope.spawn(|| {
                let _guard = self.state.lock().expect("the gate lock must be free");
                panic!("poison the gate lock on purpose");
            });
            let _ = handle.join();
        });
    }
}
