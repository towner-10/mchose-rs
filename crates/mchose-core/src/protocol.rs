//! Frame format for the MCHOSE G7 vendor HID collection.
//!
//! Recovered from the M HUB `gmouse` web app and verified against a real
//! `a8a5:2255` dongle. The device exposes one vendor collection on usage page
//! `0xFF01` / usage `0x10` with 64-byte input and output reports and no report
//! ID. Every command is a 64-byte output report and every reply is a 64-byte
//! input report.
//!
//! Request frames start with `0x55`, replies with `0xAA`. Byte 1 is the
//! command. The remaining bytes are command specific.

pub const REPORT_LEN: usize = 64;

/// First byte of every host -> device frame.
pub const HEAD_REQ: u8 = 0x55;
/// First byte of every device -> host frame.
pub const HEAD_RESP: u8 = 0xAA;

// Commands (byte 1).
pub const CMD_VERSION: u8 = 0x03;
pub const CMD_CONFIG_READ: u8 = 0x0E;
pub const CMD_CONFIG_WRITE: u8 = 0x0F;
pub const CMD_BATTERY: u8 = 0x30;
pub const CMD_MOUSE_STATUS: u8 = 0xED;

/// Report rate menu, index -> Hz.
pub const RATES_HZ: [u16; 4] = [125, 250, 500, 1000];
/// Lift-off distance menu, index -> millimetres.
pub const LOD_MM: [u8; 2] = [1, 2];

pub type Frame = [u8; REPORT_LEN];

fn frame(prefix: &[u8]) -> Frame {
    let mut f = [0u8; REPORT_LEN];
    f[..prefix.len()].copy_from_slice(prefix);
    f
}

pub fn version_request() -> Frame {
    frame(&[HEAD_REQ, CMD_VERSION])
}

pub fn mouse_status_request() -> Frame {
    frame(&[HEAD_REQ, CMD_MOUSE_STATUS])
}

/// Ask the device to report its battery. The reply is `0xAA 0x30`.
pub fn battery_request() -> Frame {
    frame(&[HEAD_REQ, CMD_BATTERY, 0xA5, 0x0B, 0x2E, 1, 1, 1])
}

/// Read the full mouse configuration. The reply is `0xAA 0x0E`.
pub fn config_read_request() -> Frame {
    frame(&[HEAD_REQ, CMD_CONFIG_READ, 0xA5, 0x0B, 0x30, 1, 1, 1])
}

/// One battery reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    /// Charge level, 0..=100 (percent).
    pub level: u8,
    /// True while the mouse reports it is charging.
    pub charging: bool,
}

impl Battery {
    /// Parse a `0xAA 0x30` reply.
    pub fn from_frame(f: &[u8]) -> Option<Self> {
        if f.len() < 10 || f[0] != HEAD_RESP || f[1] != CMD_BATTERY {
            return None;
        }
        Some(Battery {
            level: f[8].min(100),
            charging: f[9] == 1,
        })
    }
}

/// The device's full configuration, as far as this tool understands it.
///
/// The write path rewrites the whole block, so every field read here is stored
/// and sent back unchanged unless the user asked to modify it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Report-rate index into [`RATES_HZ`].
    pub rate_index: u8,
    /// Number of enabled DPI stages (usually 6).
    pub dpi_count: u8,
    /// Active stage, zero based.
    pub active_index: u8,
    /// Six DPI stage values.
    pub dpi: [u16; 6],
    pub scroll_flag: u8,
    /// Lift-off distance index into [`LOD_MM`].
    pub lod_index: u8,
    pub sensor_flag: u8,
    pub key_respond: u8,
    pub sleep_light: u8,
    pub highspeed_mode: u8,
    pub sensor_angle: i8,
}

impl Config {
    /// Parse a `0xAA 0x0E` reply.
    ///
    /// The app treats an all-zero or all-0xFF DPI block as "device did not
    /// answer yet"; we do the same and return `None`.
    pub fn from_frame(f: &[u8]) -> Option<Self> {
        if f.len() < 56 || f[0] != HEAD_RESP || f[1] != CMD_CONFIG_READ {
            return None;
        }
        let zeros = f[13] == 0 && f[14] == 0 && f[15] == 0;
        let ff = f[13] == 0xFF && f[14] == 0xFF && f[15] == 0xFF;
        if zeros || ff {
            return None;
        }
        let le = |i: usize| u16::from_le_bytes([f[i], f[i + 1]]);
        let mut dpi = [0u16; 6];
        for (i, slot) in dpi.iter_mut().enumerate() {
            *slot = le(13 + i * 2).clamp(50, 50_000);
        }
        Some(Config {
            rate_index: f[10].saturating_sub(1),
            dpi_count: f[11],
            active_index: f[12].saturating_sub(1),
            dpi,
            scroll_flag: f[48],
            lod_index: f[49],
            sensor_flag: f[50],
            key_respond: f[51],
            sleep_light: f[52],
            highspeed_mode: f[53],
            sensor_angle: f[55] as i8,
        })
    }

    /// Serialise back into a `0x55 0x0F` write frame.
    pub fn to_frame(&self) -> Frame {
        let mut f = [0u8; REPORT_LEN];
        f[0] = HEAD_REQ;
        f[1] = CMD_CONFIG_WRITE;
        f[2] = 0xAE;
        f[3] = 0x0A;
        f[4] = 0x30;
        f[5] = 1;
        f[6] = 1;
        f[7] = 1;
        f[10] = self.rate_index.saturating_add(1);
        f[11] = self.dpi_count;
        f[12] = self.active_index.saturating_add(1);
        for (i, v) in self.dpi.iter().enumerate() {
            let [lo, hi] = v.to_le_bytes();
            f[13 + i * 2] = lo;
            f[14 + i * 2] = hi;
        }
        f[48] = self.scroll_flag;
        f[49] = self.lod_index;
        f[50] = self.sensor_flag;
        f[51] = self.key_respond;
        f[52] = self.sleep_light;
        f[53] = self.highspeed_mode;
        f[55] = self.sensor_angle as u8;
        f
    }

    pub fn rate_hz(&self) -> u16 {
        RATES_HZ.get(self.rate_index as usize).copied().unwrap_or(0)
    }

    pub fn lod_mm(&self) -> u8 {
        LOD_MM.get(self.lod_index as usize).copied().unwrap_or(0)
    }
}

/// Firmware version from a `0xAA 0x03` reply, e.g. `2.0.0`.
pub fn parse_version(f: &[u8]) -> Option<String> {
    if f.len() < 26 || f[0] != HEAD_RESP || f[1] != CMD_VERSION {
        return None;
    }
    let digit = |b: u8| b.is_ascii_digit().then_some(b as char);
    Some(format!(
        "{}.{}.{}",
        digit(f[23]).unwrap_or('0'),
        digit(f[24]).unwrap_or('0'),
        digit(f[25]).unwrap_or('0'),
    ))
}

/// Mouse presence code from a `0xAA 0xED` reply. Non-zero means reachable.
pub fn parse_mouse_status(f: &[u8]) -> Option<u8> {
    if f.len() < 9 || f[0] != HEAD_RESP || f[1] != CMD_MOUSE_STATUS {
        return None;
    }
    Some(f[8])
}

/// Map a user supplied Hz value to a rate index.
pub fn rate_index_for(hz: u16) -> Option<u8> {
    RATES_HZ.iter().position(|&r| r == hz).map(|i| i as u8)
}

/// Map a user supplied LOD in millimetres to a LOD index.
pub fn lod_index_for(mm: u8) -> Option<u8> {
    LOD_MM.iter().position(|&m| m == mm).map(|i| i as u8)
}

/// Best-effort DPI ceiling for a product name. The base G7 ships the PAW3311
/// sensor (12k); the Pro/Max variants use a 26k sensor.
pub fn dpi_max_for(product_name: &str) -> u16 {
    let base_g7 = product_name.contains("G7")
        && !product_name.contains("Pro")
        && !product_name.contains("Max")
        && !product_name.contains("MAX");
    if base_g7 { 12_000 } else { 26_000 }
}
