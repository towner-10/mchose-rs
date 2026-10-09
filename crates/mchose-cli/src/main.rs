//! `mchose` - unofficial Linux control for the MCHOSE G7 gaming mouse.
//!
//! Talks to the mouse's vendor HID collection to read the battery and to read
//! and write the DPI configuration. See `README.md`.
//!
//! [`commands`] holds one function per subcommand; [`render`] holds the output
//! formatting.

mod commands;
mod render;

use std::process::ExitCode;

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
        None | Some("status") => commands::status(),
        Some("battery") => commands::battery(),
        Some("info") => commands::info(),
        Some("dpi") => commands::dpi(&args[1..]),
        Some("rate") => commands::rate(&args[1..]),
        Some("lod") => commands::lod(&args[1..]),
        Some("watch") => commands::watch(&args[1..]),
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
