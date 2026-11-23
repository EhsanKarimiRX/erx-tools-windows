use std::path::{Path, PathBuf};

use super::error::NotesError;
use super::hardening::{EXPECTED_READBACK, HardenError, SettingsReadback};
use super::openflow::{OpenError, OpenOps, OpenOutcome, open_with};
use super::windowgate::WindowGate;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Create,
    Build(PathBuf),
    Harden,
    Show,
    Destroy,
    Delete(PathBuf),
    Focus,
    ReleasedByEvent,
}

struct FakeOps<'a> {
    calls: Vec<Call>,
    gate: &'a WindowGate,
    dir: PathBuf,
    create_err: Option<NotesError>,
    release_in_create: bool,
    build_err: bool,
    harden_err: Option<HardenError>,
    release_in_harden: bool,
    show_err: bool,
}

impl<'a> FakeOps<'a> {
    fn new(gate: &'a WindowGate) -> FakeOps<'a> {
        FakeOps {
            calls: Vec::new(),
            gate,
            dir: PathBuf::from("erx-notes-web-0123456789abcdef"),
            create_err: None,
            release_in_create: false,
            build_err: false,
            harden_err: None,
            release_in_harden: false,
            show_err: false,
        }
    }
}

impl OpenOps for FakeOps<'_> {
    fn create_profile_dir(&mut self) -> Result<PathBuf, NotesError> {
        self.calls.push(Call::Create);
        if self.release_in_create {
            self.gate.release();
            self.calls.push(Call::ReleasedByEvent);
        }
        match self.create_err.clone() {
            Some(error) => Err(error),
            None => Ok(self.dir.clone()),
        }
    }

    fn build_hidden_window(&mut self, dir: &Path) -> Result<(), ()> {
        self.calls.push(Call::Build(dir.to_path_buf()));
        if self.build_err { Err(()) } else { Ok(()) }
    }

    fn harden(&mut self) -> Result<SettingsReadback, HardenError> {
        self.calls.push(Call::Harden);
        if self.release_in_harden {
            self.gate.release();
            self.calls.push(Call::ReleasedByEvent);
        }
        match self.harden_err {
            Some(error) => Err(error),
            None => Ok(EXPECTED_READBACK),
        }
    }

    fn show_window(&mut self) -> Result<(), ()> {
        self.calls.push(Call::Show);
        if self.show_err { Err(()) } else { Ok(()) }
    }

    fn destroy_window(&mut self) {
        self.calls.push(Call::Destroy);
    }

    fn focus_existing(&mut self) {
        self.calls.push(Call::Focus);
    }

    fn schedule_profile_delete(&mut self, dir: PathBuf) {
        self.calls.push(Call::Delete(dir));
    }
}

#[test]
fn f1_success() {
    let gate = WindowGate::new();
    let mut ops = FakeOps::new(&gate);
    let dir = ops.dir.clone();
    assert_eq!(open_with(&gate, &mut ops), Ok(OpenOutcome::Opened));
    assert_eq!(
        ops.calls,
        vec![Call::Create, Call::Build(dir), Call::Harden, Call::Show]
    );
    assert!(gate.is_open());
    assert!(!ops.calls.contains(&Call::Destroy));
    assert!(!ops.calls.iter().any(|call| matches!(call, Call::Delete(_))));
}

#[test]
fn f2_already_open() {
    let gate = WindowGate::new();
    let dir = PathBuf::from("erx-notes-web-0123456789abcdef");
    assert_eq!(gate.begin_open(), Ok(()));
    assert!(gate.attach_dir(dir));
    assert!(gate.finish_open());

    let mut ops = FakeOps::new(&gate);
    assert_eq!(open_with(&gate, &mut ops), Ok(OpenOutcome::AlreadyOpen));
    assert_eq!(ops.calls, vec![Call::Focus]);
    assert!(gate.is_open());
}

#[test]
fn f3_busy_opening() {
    let gate = WindowGate::new();
    assert_eq!(gate.begin_open(), Ok(()));

    let mut ops = FakeOps::new(&gate);
    assert_eq!(open_with(&gate, &mut ops), Ok(OpenOutcome::AlreadyOpen));
    assert_eq!(ops.calls, vec![Call::Focus]);
    assert!(gate.is_active());
    assert!(!gate.is_open());
}

#[test]
fn f4_profile_failure() {
    let gate = WindowGate::new();
    {
        let mut ops = FakeOps::new(&gate);
        ops.create_err = Some(NotesError::Io);
        assert_eq!(open_with(&gate, &mut ops), Err(OpenError::ProfileFailed));
        assert_eq!(ops.calls, vec![Call::Create]);
    }
    assert!(!gate.is_active());

    let mut ops = FakeOps::new(&gate);
    assert_eq!(open_with(&gate, &mut ops), Ok(OpenOutcome::Opened));
}

#[test]
fn f5_build_failure() {
    let gate = WindowGate::new();
    let mut ops = FakeOps::new(&gate);
    let dir = ops.dir.clone();
    ops.build_err = true;
    assert_eq!(open_with(&gate, &mut ops), Err(OpenError::BuildFailed));
    assert_eq!(
        ops.calls,
        vec![Call::Create, Call::Build(dir.clone()), Call::Delete(dir)]
    );
    assert!(!gate.is_active());
}

#[test]
fn f6_harden_failure_each() {
    for error in [
        HardenError::Unavailable,
        HardenError::NotApplied,
        HardenError::TimedOut,
    ] {
        let gate = WindowGate::new();
        let mut ops = FakeOps::new(&gate);
        let dir = ops.dir.clone();
        ops.harden_err = Some(error);
        assert_eq!(
            open_with(&gate, &mut ops),
            Err(OpenError::NotHardened(error))
        );
        assert_eq!(
            ops.calls,
            vec![
                Call::Create,
                Call::Build(dir.clone()),
                Call::Harden,
                Call::Destroy,
                Call::Delete(dir),
            ]
        );
        assert!(!ops.calls.contains(&Call::Show));
        assert!(!gate.is_active());
    }
}

#[test]
fn f7_show_failure() {
    let gate = WindowGate::new();
    let mut ops = FakeOps::new(&gate);
    let dir = ops.dir.clone();
    ops.show_err = true;
    assert_eq!(open_with(&gate, &mut ops), Err(OpenError::ShowFailed));
    assert_eq!(
        ops.calls,
        vec![
            Call::Create,
            Call::Build(dir.clone()),
            Call::Harden,
            Call::Show,
            Call::Destroy,
            Call::Delete(dir),
        ]
    );
    assert!(!gate.is_active());
}

#[test]
fn f8_interrupted_at_commit() {
    let gate = WindowGate::new();
    let mut ops = FakeOps::new(&gate);
    let dir = ops.dir.clone();
    ops.release_in_harden = true;
    assert_eq!(open_with(&gate, &mut ops), Err(OpenError::Interrupted));
    assert_eq!(
        ops.calls,
        vec![
            Call::Create,
            Call::Build(dir),
            Call::Harden,
            Call::ReleasedByEvent,
            Call::Destroy,
        ]
    );
    assert!(!ops.calls.contains(&Call::Show));
    assert!(!ops.calls.iter().any(|call| matches!(call, Call::Delete(_))));
}

#[test]
fn f9_interrupted_at_attach() {
    let gate = WindowGate::new();
    let mut ops = FakeOps::new(&gate);
    let dir = ops.dir.clone();
    ops.release_in_create = true;
    assert_eq!(open_with(&gate, &mut ops), Err(OpenError::Interrupted));
    assert_eq!(
        ops.calls,
        vec![Call::Create, Call::ReleasedByEvent, Call::Delete(dir)]
    );
    assert!(!ops.calls.iter().any(|call| matches!(call, Call::Build(_))));
    assert!(!ops.calls.contains(&Call::Harden));
    assert!(!ops.calls.contains(&Call::Show));
}
