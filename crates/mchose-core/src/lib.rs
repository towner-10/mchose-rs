//! Unofficial Linux library for the MCHOSE G7 gaming mouse.
//!
//! `protocol` describes the vendor HID frame format; `device` discovers the
//! mouse's vendor collection and performs the blocking hidraw I/O. Both are
//! used by the `mchose` CLI and the `mchose-gui` desktop app.

pub mod device;
pub mod protocol;
