//! Device discovery and the raw hidraw transport.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use crate::protocol::{self, Battery, Config, Frame};

/// Vendors used by the YJX `0x2255` dongle/mouse. `0xA8A5` is the 2.4 GHz
/// receiver, `0xA8A4` the mouse when it is plugged in over USB.
pub const VENDOR_IDS: [u16; 2] = [0xA8A5, 0xA8A4];
pub const PRODUCT_ID: u16 = 0x2255;

/// How long to wait for a command reply before giving up.
const RESPONSE_TIMEOUT: Duration = Duration::from_millis(900);

/// The vendor collection descriptor begins with usage page `0xFF01`, usage
/// `0x10`: `06 01 FF 09 10`. Match that exact prefix.
const VENDOR_USAGE_PREFIX: [u8; 5] = [0x06, 0x01, 0xFF, 0x09, 0x10];

#[derive(Debug)]
pub enum DeviceError {
    NotFound,
    Open { path: PathBuf, source: io::Error },
    Io(io::Error),
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceError::NotFound => write!(
                f,
                "no MCHOSE G7 vendor interface found (looked for {:04x}:{:04x} on usage page 0xFF01)",
                VENDOR_IDS[0], PRODUCT_ID
            ),
            DeviceError::Open { path, source } => {
                write!(f, "could not open {}: {source}", path.display())
            }
            DeviceError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for DeviceError {}

impl From<io::Error> for DeviceError {
    fn from(e: io::Error) -> Self {
        DeviceError::Io(e)
    }
}

struct Candidate {
    path: PathBuf,
    vendor_id: u16,
    product_id: u16,
    hid_name: String,
}

fn has_vendor_collection(descriptor: &[u8]) -> bool {
    descriptor
        .windows(VENDOR_USAGE_PREFIX.len())
        .any(|w| w == VENDOR_USAGE_PREFIX)
}

/// Parse `HID_ID=0003:0000A8A5:00002255` / `HID_NAME=YJX-CHIP MCHOSE G7`.
fn parse_uevent(uevent: &str) -> Option<(u16, u16, String)> {
    let mut vendor = None;
    let mut product = None;
    let mut name = String::new();
    for line in uevent.lines() {
        if let Some(v) = line.strip_prefix("HID_ID=") {
            let mut parts = v.split(':');
            let _bus = parts.next()?;
            vendor = parts
                .next()
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .map(|v| (v & 0xFFFF) as u16);
            product = parts
                .next()
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .map(|v| (v & 0xFFFF) as u16);
        } else if let Some(v) = line.strip_prefix("HID_NAME=") {
            name = v.trim().to_string();
        }
    }
    Some((vendor?, product?, name))
}

fn candidates() -> Vec<Candidate> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/hidraw") else {
        return out;
    };
    for entry in entries.flatten() {
        let sys = entry.path();
        let dev = Path::new("/dev").join(entry.file_name());
        let Ok(descriptor) = fs::read(sys.join("device/report_descriptor")) else {
            continue;
        };
        if !has_vendor_collection(&descriptor) {
            continue;
        }
        let uevent = fs::read_to_string(sys.join("device/uevent")).unwrap_or_default();
        let Some((vendor, product, name)) = parse_uevent(&uevent) else {
            continue;
        };
        if VENDOR_IDS.contains(&vendor) && product == PRODUCT_ID {
            out.push(Candidate {
                path: dev,
                vendor_id: vendor,
                product_id: product,
                hid_name: name,
            });
        }
    }
    out
}

/// Strip the OEM prefix from the HID product string.
fn clean_name(name: &str) -> String {
    let name = name
        .strip_prefix("YJX-CHIP ")
        .or_else(|| name.strip_prefix("RealTek "))
        .unwrap_or(name);
    name.trim().to_string()
}

/// An open connection to the mouse's vendor collection.
pub struct Mouse {
    file: File,
    rx: Receiver<Vec<u8>>,
    pub vendor_id: u16,
    pub product_id: u16,
    pub product_name: String,
    pub path: PathBuf,
}

impl Mouse {
    pub fn open() -> Result<Mouse, DeviceError> {
        let candidates = candidates();
        let Some(candidate) = candidates.into_iter().next() else {
            return Err(DeviceError::NotFound);
        };
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&candidate.path)
            .map_err(|source| DeviceError::Open {
                path: candidate.path.clone(),
                source,
            })?;

        // hidraw read() blocks until a report arrives, so pump reports on a
        // background thread and hand them to the command layer over a channel.
        let mut reader = file.try_clone()?;
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = [0u8; protocol::REPORT_LEN];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        });

        Ok(Mouse {
            file,
            rx,
            vendor_id: candidate.vendor_id,
            product_id: candidate.product_id,
            product_name: clean_name(&candidate.hid_name),
            path: candidate.path,
        })
    }

    /// True when the device is connected over USB rather than the 2.4 GHz dongle.
    pub fn is_wired(&self) -> bool {
        self.vendor_id == 0xA8A4
    }

    fn send(&self, frame: &Frame) -> io::Result<()> {
        (&self.file).write_all(frame)
    }

    fn drain(&self) {
        while self.rx.try_recv().is_ok() {}
    }

    /// Send a command and wait for the matching `0xAA <expect>` reply,
    /// ignoring unrelated push frames.
    fn request(&self, frame: &Frame, expect: u8) -> io::Result<Vec<u8>> {
        self.drain();
        self.send(frame)?;
        let deadline = Instant::now() + RESPONSE_TIMEOUT;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("timed out waiting for 0x{expect:02X} reply"),
                ));
            }
            match self.rx.recv_timeout(deadline - now) {
                Ok(r) if r.len() >= 2 && r[0] == protocol::HEAD_RESP && r[1] == expect => {
                    return Ok(r);
                }
                Ok(_) => continue,
                Err(RecvTimeoutError::Timeout) => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("timed out waiting for 0x{expect:02X} reply"),
                    ));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "device read thread stopped",
                    ));
                }
            }
        }
    }

    pub fn battery(&self) -> io::Result<Battery> {
        let reply = self.request(&protocol::battery_request(), protocol::CMD_BATTERY)?;
        Battery::from_frame(&reply)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "malformed battery reply"))
    }

    pub fn firmware(&self) -> io::Result<String> {
        let reply = self.request(&protocol::version_request(), protocol::CMD_VERSION)?;
        protocol::parse_version(&reply)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "malformed version reply"))
    }

    pub fn config(&self) -> io::Result<Config> {
        let reply = self.request(&protocol::config_read_request(), protocol::CMD_CONFIG_READ)?;
        Config::from_frame(&reply)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "malformed config reply"))
    }

    /// Send a full configuration write and wait for the device to acknowledge.
    pub fn write_config(&self, config: &Config) -> io::Result<()> {
        self.request(&config.to_frame(), protocol::CMD_CONFIG_WRITE)?;
        Ok(())
    }

    /// Mouse presence code; non-zero means the mouse is reachable.
    pub fn mouse_status(&self) -> io::Result<u8> {
        let reply = self.request(
            &protocol::mouse_status_request(),
            protocol::CMD_MOUSE_STATUS,
        )?;
        protocol::parse_mouse_status(&reply)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "malformed status reply"))
    }
}
