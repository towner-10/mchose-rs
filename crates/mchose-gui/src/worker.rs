//! The device worker.
//!
//! `Mouse` I/O blocks (a command can take up to ~1s), so it never runs on the
//! UI thread. A dedicated thread owns the connection, drains [`Request`]s from
//! the view and pushes [`Update`]s back. The view polls those updates on a GPUI
//! timer.
//!
//! The worker also reconnects on its own: if the device disappears it drops the
//! connection, reports the error once, and keeps trying every [`POLL`].

use std::io;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use mchose_core::device::{DeviceError, Mouse};
use mchose_core::protocol::{self, Config};

/// Idle refresh interval, and the rate at which we retry a missing device.
const POLL: Duration = Duration::from_secs(2);
/// Settle time after a write before reading the configuration back.
const SETTLE: Duration = Duration::from_millis(180);

/// A immutable reading of the mouse, as shown by the view.
#[derive(Clone)]
pub struct Snapshot {
    pub name: String,
    pub wired: bool,
    pub path: String,
    pub firmware: Option<String>,
    pub battery: Option<(u8, bool)>,
    pub config: Option<Config>,
    pub asleep: bool,
    pub max_dpi: u16,
}

/// What the view asks the worker to do.
pub enum Request {
    Refresh,
    SetDpi(usize, u16),
    NudgeDpi(usize, i32),
    SetActive(usize),
    SetRate(u8),
    SetLod(u8),
}

/// What the worker reports back.
pub enum Update {
    State(Snapshot),
    Error(String),
}

/// Start the worker. It exits when the view drops the request channel.
pub fn spawn(req_rx: Receiver<Request>, upd_tx: Sender<Update>) {
    thread::spawn(move || {
        let mut mouse: Option<Mouse> = None;
        let mut firmware: Option<String> = None;
        let mut last_error: Option<String> = None;

        loop {
            let request = req_rx.recv_timeout(POLL);
            if matches!(request, Err(RecvTimeoutError::Disconnected)) {
                break;
            }

            if mouse.is_none() {
                try_connect(&mut mouse, &mut firmware, &mut last_error, &upd_tx);
            }
            let Some(device) = mouse.as_ref() else {
                continue;
            };

            let outcome = match &request {
                Ok(Request::Refresh) => Ok(()),
                Ok(Request::SetDpi(stage, value)) => apply(device, |c| {
                    if *stage < 6 {
                        c.dpi[*stage] = *value;
                    }
                }),
                Ok(Request::NudgeDpi(stage, delta)) => apply(device, |c| {
                    if *stage < 6 {
                        let next = c.dpi[*stage] as i32 + delta;
                        c.dpi[*stage] = next.clamp(50, 50_000) as u16;
                    }
                }),
                Ok(Request::SetActive(stage)) => apply(device, |c| {
                    let last = c.dpi_count.clamp(1, 6).saturating_sub(1) as usize;
                    c.active_index = (*stage).min(last) as u8;
                }),
                Ok(Request::SetRate(index)) => apply(device, |c| c.rate_index = *index),
                Ok(Request::SetLod(index)) => apply(device, |c| c.lod_index = *index),
                Err(RecvTimeoutError::Timeout) => Ok(()),
                Err(RecvTimeoutError::Disconnected) => unreachable!(),
            };

            match outcome.and_then(|()| read_snapshot(device, firmware.clone())) {
                Ok(snapshot) => {
                    last_error = None;
                    let _ = upd_tx.send(Update::State(snapshot));
                }
                Err(e) => {
                    mouse = None;
                    report_error(&mut last_error, e.to_string(), &upd_tx);
                }
            }
        }
    });
}

/// Turn a device error into a user-facing message.
fn describe(error: &DeviceError) -> String {
    if let DeviceError::Open { source, .. } = error
        && source.kind() == io::ErrorKind::PermissionDenied
    {
        return "Permission denied opening the device.\n\
                Install udev/99-mchose-g7.rules and reconnect the mouse, or run with sudo."
            .to_string();
    }
    error.to_string()
}

/// Read battery and configuration. A timed-out config read means the mouse is
/// asleep (the dongle answered, the mouse did not), not that the link is gone.
fn read_snapshot(mouse: &Mouse, firmware: Option<String>) -> Result<Snapshot, io::Error> {
    let battery = mouse.battery().ok().map(|b| (b.level, b.charging));
    let base = |config, asleep| Snapshot {
        name: mouse.product_name.clone(),
        wired: mouse.is_wired(),
        path: mouse.path.display().to_string(),
        firmware: firmware.clone(),
        battery,
        config,
        asleep,
        max_dpi: protocol::dpi_max_for(&mouse.product_name),
    };
    match mouse.config() {
        Ok(config) => Ok(base(Some(config), false)),
        Err(e) if e.kind() == io::ErrorKind::TimedOut => Ok(base(None, true)),
        Err(e) => Err(e),
    }
}

/// Send an error only when it differs from the last one, so a missing device
/// does not spam the view every poll.
fn report_error(last: &mut Option<String>, message: String, tx: &Sender<Update>) {
    if last.as_deref() != Some(message.as_str()) {
        *last = Some(message.clone());
        let _ = tx.send(Update::Error(message));
    }
}

fn try_connect(
    mouse: &mut Option<Mouse>,
    firmware: &mut Option<String>,
    last_error: &mut Option<String>,
    tx: &Sender<Update>,
) {
    match Mouse::open() {
        Ok(m) => {
            *firmware = m.firmware().ok();
            match read_snapshot(&m, firmware.clone()) {
                Ok(snapshot) => {
                    *last_error = None;
                    let _ = tx.send(Update::State(snapshot));
                }
                Err(e) => report_error(last_error, e.to_string(), tx),
            }
            *mouse = Some(m);
        }
        Err(e) => report_error(last_error, describe(&e), tx),
    }
}

/// Read the configuration, apply an edit, write the whole block back.
fn apply(mouse: &Mouse, edit: impl FnOnce(&mut Config)) -> Result<(), io::Error> {
    let mut config = mouse.config()?;
    edit(&mut config);
    mouse.write_config(&config)?;
    thread::sleep(SETTLE);
    Ok(())
}
