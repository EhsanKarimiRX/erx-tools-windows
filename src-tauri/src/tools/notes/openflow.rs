//! The order that opens the hardened Secure Notes window. Every step that can
//! fail runs before the window is shown, and every failure removes the window
//! and its profile folder, so the window is never shown unless the WebView2
//! settings were verified.

use std::path::{Path, PathBuf};

use super::error::NotesError;
use super::hardening::{HardenError, SettingsReadback};
use super::windowgate::WindowGate;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenOutcome {
    Opened,
    AlreadyOpen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    ProfileFailed,
    BuildFailed,
    NotHardened(HardenError),
    ShowFailed,
    Interrupted,
}

/// Everything the flow needs from the outside world. The real implementation
/// (N6-4) uses Tauri; tests use a fake.
pub trait OpenOps {
    fn create_profile_dir(&mut self) -> Result<PathBuf, NotesError>;
    fn build_hidden_window(&mut self, dir: &Path) -> Result<(), ()>;
    fn harden(&mut self) -> Result<SettingsReadback, HardenError>;
    fn show_window(&mut self) -> Result<(), ()>;
    fn destroy_window(&mut self);
    fn focus_existing(&mut self);
    fn schedule_profile_delete(&mut self, dir: PathBuf);
}

pub fn open_with(gate: &WindowGate, ops: &mut dyn OpenOps) -> Result<OpenOutcome, OpenError> {
    if gate.begin_open().is_err() {
        ops.focus_existing();
        return Ok(OpenOutcome::AlreadyOpen);
    }

    let dir = match ops.create_profile_dir() {
        Ok(dir) => dir,
        Err(_) => {
            gate.release();
            return Err(OpenError::ProfileFailed);
        }
    };

    if !gate.attach_dir(dir.clone()) {
        ops.schedule_profile_delete(dir);
        return Err(OpenError::Interrupted);
    }

    if ops.build_hidden_window(&dir).is_err() {
        abort(gate, ops, false);
        return Err(OpenError::BuildFailed);
    }

    if let Err(error) = ops.harden() {
        abort(gate, ops, true);
        return Err(OpenError::NotHardened(error));
    }

    if !gate.finish_open() {
        ops.destroy_window();
        return Err(OpenError::Interrupted);
    }

    if ops.show_window().is_err() {
        abort(gate, ops, true);
        return Err(OpenError::ShowFailed);
    }

    Ok(OpenOutcome::Opened)
}

fn abort(gate: &WindowGate, ops: &mut dyn OpenOps, destroy: bool) {
    if destroy {
        ops.destroy_window();
    }
    if let Some(dir) = gate.release() {
        ops.schedule_profile_delete(dir);
    }
}
