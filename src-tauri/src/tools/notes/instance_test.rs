//! One-instance guard tests. Every test uses its own mutex name.

use super::error::NotesError;
use super::instance::*;
use super::webprofile::random_hex;

fn unique_name() -> String {
    format!("Local\\ERXTools.Test.{}", random_hex(8).unwrap())
}

#[test]
fn i1_first_acquire() {
    let name = unique_name();
    assert!(acquire_instance(&name).unwrap().is_some());
}

#[test]
fn i2_second_acquire_is_none() {
    let name = unique_name();
    let first = acquire_instance(&name).unwrap();
    assert!(first.is_some());
    assert!(acquire_instance(&name).unwrap().is_none());
    drop(first);
}

#[test]
fn i3_acquire_after_drop() {
    let name = unique_name();
    let first = acquire_instance(&name).unwrap();
    assert!(first.is_some());
    drop(first);
    assert!(acquire_instance(&name).unwrap().is_some());
}

#[test]
fn i4_different_names_independent() {
    let first = acquire_instance(&unique_name()).unwrap();
    let second = acquire_instance(&unique_name()).unwrap();
    assert!(first.is_some());
    assert!(second.is_some());
}

#[test]
fn i5_bad_names() {
    assert!(matches!(acquire_instance(""), Err(NotesError::Internal)));
    assert!(matches!(
        acquire_instance("Local\\ERXTools.Test.bad\0name"),
        Err(NotesError::Internal)
    ));
}

#[test]
fn i7_duplicate_handle_is_released() {
    let name = unique_name();
    let first = acquire_instance(&name).unwrap();
    assert!(first.is_some());
    assert!(acquire_instance(&name).unwrap().is_none());
    drop(first);
    assert!(acquire_instance(&name).unwrap().is_some());
}

#[test]
fn i6_constant_and_debug() {
    assert_eq!(
        NOTES_INSTANCE_NAME,
        "Local\\ERXTools.SecureNotes.Instance.v1"
    );
    assert!(NOTES_INSTANCE_NAME.starts_with("Local\\"));
    let guard = acquire_instance(&unique_name()).unwrap().unwrap();
    assert_eq!(format!("{:?}", guard), "InstanceGuard { .. }");
}
