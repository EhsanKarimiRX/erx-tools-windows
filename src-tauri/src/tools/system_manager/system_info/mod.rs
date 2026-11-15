//! System information card (top of System Manager): a read-only overview of
//! the hardware and Windows version. Everything is read again every time the
//! card is opened, needs no administrator rights, uses no network and sends
//! nothing anywhere. Pure logic (text helpers, parsers) is separated from the
//! thin Windows readers so it can be unit tested.

// Temporary: removed in the last step (SC7) when everything is wired up.
#![allow(dead_code)]

pub mod bytes;
pub mod core;
