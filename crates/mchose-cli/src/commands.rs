//! Command handlers.
//!
//! Each command opens the mouse, does one thing, and prints the result. Writes
//! go through [`apply`], which reads the configuration back afterwards so the
//! caller reports what the mouse actually holds rather than what was asked for.

use std::thread;
use std::time::Duration;

use mchose_core::device::{DeviceError, Mouse};
use mchose_core::protocol::{self, Battery, Config};

use crate::render;

/// Open the mouse, turning a permission failure into an actionable message.
pub fn open() -> Result<Mouse, String> {
    Mouse::open().map_err(|e| {
        if let DeviceError::Open { source, .. } = &e
            && source.kind() == std::io::ErrorKind::PermissionDenied
        {
            return format!(
                "{e}\n\nPermission denied. Install the udev rule from \
                 udev/99-mchose-g7.rules and reconnect the mouse, or run with sudo."
            );
        }
        e.to_string()
    })
}

pub fn status() -> Result<(), String> {
    let mouse = open()?;
    println!(
        "{}  ({:04x}:{:04x}, {})",
        mouse.product_name,
        mouse.vendor_id,
        mouse.product_id,
        render::link(&mouse)
    );
    if let Ok(fw) = mouse.firmware() {
        println!("Firmware    {fw}");
    }
    match mouse.battery() {
        Ok(b) => println!("Battery     {}", render::battery_line(&b)),
        Err(_) => println!("Battery     unavailable"),
    }
    match mouse.config() {
        Ok(c) => render::print_config(&c),
        Err(_) => println!("DPI         unavailable - is the mouse awake?"),
    }
    Ok(())
}

pub fn battery() -> Result<(), String> {
    let mouse = open()?;
    let b = mouse.battery().map_err(short)?;
    println!("Battery     {}", render::battery_line(&b));
    Ok(())
}

pub fn info() -> Result<(), String> {
    let mouse = open()?;
    println!(
        "Device      {}  ({:04x}:{:04x})",
        mouse.product_name, mouse.vendor_id, mouse.product_id
    );
    println!("Link        {}", render::link(&mouse));
    println!("Hidraw      {}", mouse.path.display());
    if let Ok(fw) = mouse.firmware() {
        println!("Firmware    {fw}");
    }
    match mouse.mouse_status() {
        Ok(code) if code != 0 => println!("Mouse       awake (status {code})"),
        Ok(_) => println!("Mouse       asleep / not reachable"),
        Err(_) => println!("Mouse       no reply"),
    }
    Ok(())
}

pub fn dpi(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        None | Some("show") => {
            let mouse = open()?;
            let c = mouse.config().map_err(short)?;
            render::print_config(&c);
            Ok(())
        }
        Some("set") => dpi_set(&args[1..]),
        Some("active") => dpi_active(&args[1..]),
        Some("list") => dpi_list(&args[1..]),
        Some(other) => Err(format!(
            "unknown 'dpi {other}' subcommand (expected show, set, active or list)"
        )),
    }
}

fn dpi_set(args: &[String]) -> Result<(), String> {
    if args.len() != 2 {
        return Err("usage: mchose dpi set <stage 1-6> <dpi>".into());
    }
    let stage = parse_stage(&args[0])?;
    let value: u16 = args[1]
        .parse()
        .map_err(|_| format!("'{}' is not a DPI value", args[1]))?;

    let mouse = open()?;
    let mut config = mouse.config().map_err(short)?;
    let max = protocol::dpi_max_for(&mouse.product_name);
    check_dpi(value, max)?;

    config.dpi[(stage - 1) as usize] = value;
    let after = apply(&mouse, &config)?;
    println!(
        "Stage {stage} is now {} DPI (active stage {}/{}).",
        after.dpi[(stage - 1) as usize],
        after.active_index + 1,
        after.dpi_count
    );
    Ok(())
}

fn dpi_active(args: &[String]) -> Result<(), String> {
    if args.len() != 1 {
        return Err("usage: mchose dpi active <stage 1-6>".into());
    }
    let stage = parse_stage(&args[0])?;

    let mouse = open()?;
    let mut config = mouse.config().map_err(short)?;
    let enabled = config.dpi_count.clamp(1, 6);
    if stage > enabled {
        return Err(format!(
            "stage {stage} is beyond the enabled stages ({enabled})"
        ));
    }
    config.active_index = stage - 1;
    let after = apply(&mouse, &config)?;
    println!(
        "Active DPI stage is {} ({} DPI).",
        after.active_index + 1,
        after.dpi[after.active_index as usize]
    );
    Ok(())
}

fn dpi_list(args: &[String]) -> Result<(), String> {
    if args.is_empty() || args.len() > 6 {
        return Err("usage: mchose dpi list <dpi> [dpi] ... (1 to 6 values)\n\
             e.g.   mchose dpi list 400 800 1600 3200 6400 12000"
            .into());
    }
    let mouse = open()?;
    let mut config = mouse.config().map_err(short)?;
    let max = protocol::dpi_max_for(&mouse.product_name);
    for (i, raw) in args.iter().enumerate() {
        let value: u16 = raw
            .parse()
            .map_err(|_| format!("'{raw}' is not a DPI value"))?;
        check_dpi(value, max)?;
        config.dpi[i] = value;
    }
    config.dpi_count = args.len() as u8;
    if config.active_index >= config.dpi_count {
        config.active_index = config.dpi_count - 1;
    }
    let after = apply(&mouse, &config)?;
    render::print_config(&after);
    Ok(())
}

pub fn rate(args: &[String]) -> Result<(), String> {
    if args.len() != 1 {
        return Err("usage: mchose rate <125|250|500|1000>".into());
    }
    let hz: u16 = args[0]
        .parse()
        .map_err(|_| format!("'{}' is not a report rate", args[0]))?;
    let index = protocol::rate_index_for(hz)
        .ok_or_else(|| format!("unsupported rate {hz} Hz (use 125, 250, 500 or 1000)"))?;

    let mouse = open()?;
    let mut config = mouse.config().map_err(short)?;
    config.rate_index = index;
    let after = apply(&mouse, &config)?;
    println!("Report rate is now {} Hz.", after.rate_hz());
    Ok(())
}

pub fn lod(args: &[String]) -> Result<(), String> {
    if args.len() != 1 {
        return Err("usage: mchose lod <1|2>".into());
    }
    let mm: u8 = args[0]
        .parse()
        .map_err(|_| format!("'{}' is not a lift-off distance", args[0]))?;
    let index = protocol::lod_index_for(mm)
        .ok_or_else(|| format!("unsupported lift-off {mm} mm (use 1 or 2)"))?;

    let mouse = open()?;
    let mut config = mouse.config().map_err(short)?;
    config.lod_index = index;
    let after = apply(&mouse, &config)?;
    println!("Lift-off distance is now {} mm.", after.lod_mm());
    Ok(())
}

pub fn watch(args: &[String]) -> Result<(), String> {
    let interval: u64 = match args.first() {
        None => 2,
        Some(raw) => raw
            .parse()
            .ok()
            .filter(|&s| s > 0)
            .ok_or_else(|| format!("'{raw}' is not a positive number of seconds"))?,
    };

    let mouse = open()?;
    println!(
        "Watching {} every {interval}s - press Ctrl-C to stop.",
        mouse.product_name
    );
    let mut last: Option<(Battery, Config)> = None;
    loop {
        match (mouse.battery(), mouse.config()) {
            (Ok(b), Ok(c)) => {
                let current = (b, c);
                if last.as_ref() != Some(&current) {
                    let (b, c) = &current;
                    println!(
                        "{}  battery {:>3}%{}   dpi stage {}/{} ({} dpi)   rate {} Hz",
                        render::stamp(),
                        b.level,
                        if b.charging { " charging" } else { "         " },
                        c.active_index + 1,
                        c.dpi_count,
                        c.dpi[c.active_index as usize],
                        c.rate_hz(),
                    );
                    last = Some(current);
                }
            }
            _ => {
                if last.is_some() {
                    println!("{}  mouse not responding", render::stamp());
                    last = None;
                }
            }
        }
        thread::sleep(Duration::from_secs(interval));
    }
}

/// Write a configuration and read it back so the caller reports reality.
fn apply(mouse: &Mouse, config: &Config) -> Result<Config, String> {
    mouse.write_config(config).map_err(short)?;
    thread::sleep(Duration::from_millis(200));
    mouse.config().map_err(short)
}

fn parse_stage(raw: &str) -> Result<u8, String> {
    let stage: u8 = raw
        .parse()
        .map_err(|_| format!("'{raw}' is not a stage number"))?;
    if (1..=6).contains(&stage) {
        Ok(stage)
    } else {
        Err(format!("stage must be between 1 and 6 (got {stage})"))
    }
}

fn check_dpi(value: u16, max: u16) -> Result<(), String> {
    if (50..=max).contains(&value) {
        Ok(())
    } else {
        Err(format!("DPI must be between 50 and {max} (got {value})"))
    }
}

fn short(e: std::io::Error) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_accepts_one_through_six() {
        for stage in 1..=6 {
            assert_eq!(parse_stage(&stage.to_string()), Ok(stage));
        }
    }

    #[test]
    fn stage_rejects_out_of_range_and_garbage() {
        assert!(parse_stage("0").is_err());
        assert!(parse_stage("7").is_err());
        assert!(parse_stage("two").is_err());
    }

    #[test]
    fn dpi_respects_sensor_ceiling() {
        assert!(check_dpi(50, 12_000).is_ok());
        assert!(check_dpi(12_000, 12_000).is_ok());
        assert!(check_dpi(49, 12_000).is_err());
        assert!(check_dpi(12_050, 12_000).is_err());
    }
}
