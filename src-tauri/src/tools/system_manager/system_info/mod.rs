//! System information card (top of System Manager): a read-only overview of
//! the hardware and Windows version. Everything is read again every time the
//! card is opened, needs no administrator rights, uses no network and sends
//! nothing anywhere. Pure logic (text helpers, parsers) is separated from the
//! thin Windows readers so it can be unit tested.

pub mod bytes;
pub mod card;
pub mod commands;
pub mod core;
pub mod smbios;

#[cfg(windows)]
pub mod basic;
#[cfg(windows)]
pub mod battery;
#[cfg(windows)]
pub mod gpu;
#[cfg(windows)]
pub mod memory;
#[cfg(windows)]
pub mod storage;
#[cfg(windows)]
pub mod winreg;
