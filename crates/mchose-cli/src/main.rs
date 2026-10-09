//! `mchose` - unofficial Linux control for the MCHOSE G7 gaming mouse.
//!
//! Talks to the mouse's vendor HID collection to read the battery and to read
//! and write the DPI configuration. See `README.md`.

use std::process::ExitCode;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mchose_core::device::{DeviceError, Mouse};
use mchose_core::protocol::{self, Battery, Config};

const HELP: &str = "\
mchose - control a MCHOSE G7 gaming mouse on Linux

USAGE:
    mchose [COMMAND]

COMMANDS:
    status                  Battery, link and DPI summary (default)
    battery                 Battery percentage and charging state
    info                    Firmware and link details
    dpi                     Show the six DPI stages and the active stage
    dpi set <stage> <dpi>   Set one DPI stage (stage 1-6)
    dpi active <stage>      Make a DPI stage the active one
    dpi list <dpi>...       Set stages in order (1 to 6 values)
    rate <hz>               Set report rate: 125, 250, 500 or 1000
    lod <mm>                Set lift-off distance: 1 or 2
    watch [seconds]         Print battery/DPI whenever they change (default 2s)

    -h, --help              Show this help
    -V, --version           Show version
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("error: {msg}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        None | Some("status") => status(),
        Some("battery") => battery(),
        Some("info") => info(),
        Some("dpi") => dpi(&args[1..]),
        Some("rate") => rate(&args[1..]),
        Some("lod") => lod(&args[1..]),
        Some("watch") => watch(&args[1..]),
        Some("-h" | "--help" | "help") => {
            print!("{HELP}");
            Ok(())
        }
        Some("-V" | "--version") => {
            println!("mchose {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(other) => Err(format!("unknown command '{other}'\n\n{HELP}")),
    }
}

/// Open the mouse, turning a permission failure into an actionable message.
fn open() -> Result<Mouse, String> {
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

fn status() -> Result<(), String> {
    let mouse = open()?;
    println!(
        "{}  ({:04x}:{:04x}, {})",
        mouse.product_name,
        mouse.vendor_id,
        mouse.product_id,
        link(&mouse)
    );
    if let Ok(fw) = mouse.firmware() {
        println!("Firmware    {fw}");
    }
    match mouse.battery() {
        Ok(b) => println!("Battery     {}", battery_line(&b)),
        Err(_) => println!("Battery     unavailable"),
    }
    match mouse.config() {
        Ok(c) => print_config(&c),
        Err(_) => println!("DPI         unavailable - is the mouse awake?"),
    }
    Ok(())
}

fn battery() -> Result<(), String> {
    let mouse = open()?;
    let b = mouse.battery().map_err(short)?;
    println!("Battery     {}", battery_line(&b));
    Ok(())
}

fn info() -> Result<(), String> {
    let mouse = open()?;
    println!(
        "Device      {}  ({:04x}:{:04x})",
        mouse.product_name, mouse.vendor_id, mouse.product_id
    );
    println!("Link        {}", link(&mouse));
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

fn dpi(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        None | Some("show") => {
            let mouse = open()?;
            let c = mouse.config().map_err(short)?;
            print_config(&c);
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
    print_config(&after);
    Ok(())
}

fn rate(args: &[String]) -> Result<(), String> {
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

fn lod(args: &[String]) -> Result<(), String> {
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

fn watch(args: &[String]) -> Result<(), String> {
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
        let reading = (mouse.battery(), mouse.config());
        match reading {
            (Ok(b), Ok(c)) => {
                let current = (b, c);
                if last.as_ref() != Some(&current) {
                    let (b, c) = &current;
                    println!(
                        "{}  battery {:>3}%{}   dpi stage {}/{} ({} dpi)   rate {} Hz",
                        stamp(),
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
                    println!("{}  mouse not responding", stamp());
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

fn print_config(config: &Config) {
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

fn battery_line(b: &Battery) -> String {
    if b.charging {
        format!("{}%  (charging)", b.level)
    } else {
        format!("{}%", b.level)
    }
}

fn link(mouse: &Mouse) -> &'static str {
    if mouse.is_wired() {
        "USB wired"
    } else {
        "2.4 GHz"
    }
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

/// UTC wall clock as `HH:MM:SS`, computed without extra dependencies.
fn stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let day = secs % 86_400;
    format!("{:02}:{:02}:{:02}", day / 3600, (day % 3600) / 60, day % 60)
}
