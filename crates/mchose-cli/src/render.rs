//! Output formatting.
//!
//! Separated from the command handlers so presentation can change (or be
//! tested) without touching anything that talks to the device.

use std::time::{SystemTime, UNIX_EPOCH};

use mchose_core::device::Mouse;
use mchose_core::protocol::{Battery, Config};

pub fn print_config(config: &Config) {
    println!("Report rate  {} Hz", config.rate_hz());
    println!("Lift-off     {} mm", config.lod_mm());
    println!(
        "DPI stages   active {}, {} enabled",
        config.active_index + 1,
        config.dpi_count
    );
    for i in 0..6u8 {
        let mut notes = Vec::new();
        if i == config.active_index {
            notes.push("active");
        }
        if i >= config.dpi_count {
            notes.push("unused");
        }
        let note = if notes.is_empty() {
            String::new()
        } else {
            format!("   ({})", notes.join(", "))
        };
        println!(
            "  stage {}  {:>6} DPI{}",
            i + 1,
            config.dpi[i as usize],
            note
        );
    }
}

pub fn battery_line(b: &Battery) -> String {
    if b.charging {
        format!("{}%  (charging)", b.level)
    } else {
        format!("{}%", b.level)
    }
}

pub fn link(mouse: &Mouse) -> &'static str {
    if mouse.is_wired() {
        "USB wired"
    } else {
        "2.4 GHz"
    }
}

/// UTC wall clock as `HH:MM:SS`, computed without extra dependencies.
pub fn stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let day = secs % 86_400;
    format!("{:02}:{:02}:{:02}", day / 3600, (day % 3600) / 60, day % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_line_marks_charging() {
        let idle = Battery {
            level: 42,
            charging: false,
        };
        let charging = Battery {
            level: 42,
            charging: true,
        };
        assert_eq!(battery_line(&idle), "42%");
        assert_eq!(battery_line(&charging), "42%  (charging)");
    }
}
