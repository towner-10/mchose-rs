# mchose-rs

Unofficial Linux control for the **MCHOSE G7** gaming mouse (and its
`a8a5:2255` / `a8a4:2255` dongle). Read the battery, and read/change the DPI
configuration, from a small Rust CLI or a GPUI desktop app.

No vendor driver, no browser, no daemon. The settings live on the mouse.

```
$ mchose status
MCHOSE G7  (a8a5:2255, 2.4 GHz)
Firmware    2.0.0
Battery     100%  (charging)
Report rate  1000 Hz
Lift-off     1 mm
DPI stages   active 2, 6 enabled
  stage 1     400 DPI
  stage 2     650 DPI   (active)
  stage 3    1600 DPI
  stage 4    3200 DPI
  stage 5    6400 DPI
  stage 6   12000 DPI
```

The GUI shows the same state and lets you adjust it live. It follows Zed's
default **One Dark** theme (same surfaces, borders, text and accent colours)
and is built with [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui).

## Layout

```
crates/
  mchose-core/   shared library: HID protocol + device transport
  mchose-cli/    the `mchose` command-line tool
  mchose-gui/    the `mchose-gui` GPUI desktop app
udev/            udev rule for unprivileged device access
```

Nothing but the standard library is used except for the GUI, which depends on
`gpui` (there are no `libusb`/`libudev`/`hidapi` requirements for the CLI).

## Build

```sh
cargo build --workspace --release       # everything
# binaries:
#   target/release/mchose
#   target/release/mchose-gui

cargo build -p mchose-cli --release     # CLI only (fast, no gpui)
cargo run   -p mchose-gui               # run the GUI
```

## Permissions

Linux restricts `/dev/hidraw*` to root. Install the supplied udev rule once:

```sh
sudo cp udev/99-mchose-g7.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
# now reconnect the mouse/dongle
```

Then `mchose` and `mchose-gui` work as your normal user. Running with `sudo`
also works but is not required.

## CLI commands

```
mchose status                  battery, link and DPI summary (default)
mchose battery                 battery percentage and charging state
mchose info                    firmware and link details
mchose dpi                     show the six DPI stages and the active stage
mchose dpi set <stage> <dpi>   set one DPI stage (stage 1-6)
mchose dpi active <stage>      make a DPI stage the active one
mchose dpi list <dpi>...       set stages in order (1 to 6 values)
mchose rate <hz>               report rate: 125, 250, 500 or 1000
mchose lod <mm>                lift-off distance: 1 or 2
mchose watch [seconds]         print battery/DPI whenever they change
```

Examples:

```sh
mchose dpi set 2 800           # stage 2 -> 800 DPI
mchose dpi active 2            # switch to stage 2
mchose dpi list 400 800 1600 3200 6400 12000
mchose rate 1000
mchose watch                   # live battery/DPI monitor
```

Every write is read back from the mouse afterwards, so what is printed is what
the mouse actually holds.

## How it works

The mouse's third USB interface carries a vendor HID collection, usage page
`0xFF01` / usage `0x10`, with 64-byte input and output reports and no report ID.
That is the same channel the official M HUB `gmouse` page uses over WebHID.

The tool discovers the interface by reading the kernel report descriptor
(`/sys/class/hidraw/*/device/report_descriptor`) for that usage page, so the
`/dev/hidrawN` number is never assumed.

Frames start with `0x55` (host → device) or `0xAA` (device → host); byte 1 is
the command. The relevant ones:

| Command | Direction | Meaning |
|---|---|---|
| `0x55 0x03` | → | read firmware version |
| `0x55 0x30 0xA5 0x0B 0x2E 01 01 01` | → | read battery → `0xAA 0x30`, level at byte 8, charging at byte 9 |
| `0x55 0x0E 0xA5 0x0B 0x30 01 01 01` | → | read config → `0xAA 0x0E` |
| `0x55 0x0F 0xAE 0x0A 0x30 ...` | → | write config (full block) |
| `0xAA 0xFA ...` | ← | unsolicited active-stage / rate change |

In the config block, byte 10 is the report-rate index + 1, byte 11 the number of
enabled stages, byte 12 the active stage + 1, bytes 13..24 the six DPI values as
little-endian `u16`, byte 49 the lift-off index. Report rates are
`0→125, 1→250, 2→500, 3→1000` Hz.

This was recovered from the public M HUB front-end and verified byte-for-byte
against a real G7 (`a8a5:2255`, firmware 2.0.0).

## Notes

* The workspace pins `libc` to `=0.2.180`: `libc` 0.2.181+ dropped `ENOATTR` on
  Linux, which breaks `xattr 0.2.3` (pulled in transitively by `gpui`).
* Tested on Linux with the 2.4 GHz dongle. The wired mouse (`a8a4:2255`) is
  matched too but has not been exercised here.
* Bluetooth mode does not expose this vendor collection, so it is not supported.
* Writes are applied by rewriting the whole config block, preserving every
  field the tool does not change.
* Not affiliated with or endorsed by MCHOSE.

## License

MIT - see [LICENSE](LICENSE).
