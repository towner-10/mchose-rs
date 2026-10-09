//! `mchose-gui` - a GPUI desktop front-end for the MCHOSE G7.
//!
//! The visual language follows Zed's default **One Dark** theme: the same
//! surfaces, borders, text and accent colours, a title-bar strip, flat panels
//! and small corner radii.
//!
//! The device lives behind a worker thread so the UI never blocks on HID I/O:
//! the view sends [`Request`]s and polls the worker's [`Update`]s on a GPUI
//! timer. Battery, DPI, report rate and lift-off are all read back from the
//! mouse, so what you see is what the mouse holds.
//!
//! Build with: `cargo run -p mchose-gui --release`

use std::io;
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use gpui::{
    App, Application, Bounds, ClickEvent, Context, ElementId, FontWeight, Render, SharedString,
    Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, rgba, size,
};
use mchose_core::device::{DeviceError, Mouse};
use mchose_core::protocol::{self, Config, LOD_MM, RATES_HZ};

// ---------------------------------------------------------------------------
// Zed "One Dark" tokens (assets/themes/one/one.json)
// ---------------------------------------------------------------------------

const FONT: &str = "Noto Sans";

const EDITOR_BG: u32 = 0x282c33; // editor.background / window
const SURFACE: u32 = 0x2f343e; // panel.background
const ELEMENT: u32 = 0x2e343e; // element.background
const ELEMENT_HOVER: u32 = 0x363c46; // element.hover / border.variant
const ELEMENT_ACTIVE: u32 = 0x454a56; // element.active / element.selected
const TITLEBAR: u32 = 0x3b414d; // title_bar.background / status_bar.background
const BORDER: u32 = 0x464b57; // border
const BORDER_VARIANT: u32 = 0x363c46; // border.variant
const TEXT: u32 = 0xdce0e5;
const TEXT_MUTED: u32 = 0xa9afbc;
const TEXT_FAINT: u32 = 0x878a98;
const ACCENT: u32 = 0x74ade8; // icon.accent / link_text.hover
const ACCENT_DIM: u32 = 0x46618a;
const SUCCESS: u32 = 0xa1c181;
const WARNING: u32 = 0xdec184;
const ERROR: u32 = 0xd07277;
const WHITE: u32 = 0xffffff;

// ---------------------------------------------------------------------------
// Worker: owns the mouse, answers requests, streams updates
// ---------------------------------------------------------------------------

const POLL: Duration = Duration::from_secs(2);
const SETTLE: Duration = Duration::from_millis(180);

#[derive(Clone)]
struct Snapshot {
    name: String,
    wired: bool,
    path: String,
    firmware: Option<String>,
    battery: Option<(u8, bool)>,
    config: Option<Config>,
    asleep: bool,
    max_dpi: u16,
}

enum Request {
    Refresh,
    SetDpi(usize, u16),
    NudgeDpi(usize, i32),
    SetActive(usize),
    SetRate(u8),
    SetLod(u8),
}

enum Update {
    State(Snapshot),
    Error(String),
}

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

fn apply(mouse: &Mouse, edit: impl FnOnce(&mut Config)) -> Result<(), io::Error> {
    let mut config = mouse.config()?;
    edit(&mut config);
    mouse.write_config(&config)?;
    thread::sleep(SETTLE);
    Ok(())
}

fn spawn_worker(req_rx: Receiver<Request>, upd_tx: Sender<Update>) {
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

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

struct MouseApp {
    snapshot: Option<Snapshot>,
    error: Option<String>,
    req_tx: Sender<Request>,
}

impl MouseApp {
    fn new(cx: &mut Context<Self>) -> Self {
        let (req_tx, req_rx) = mpsc::channel();
        let (upd_tx, upd_rx) = mpsc::channel();
        spawn_worker(req_rx, upd_tx);

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(150))
                    .await;
                loop {
                    match upd_rx.try_recv() {
                        Ok(update) => {
                            if this
                                .update(cx, |this, cx| {
                                    this.on_update(update);
                                    cx.notify();
                                })
                                .is_err()
                            {
                                return;
                            }
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => return,
                    }
                }
            }
        })
        .detach();

        Self {
            snapshot: None,
            error: None,
            req_tx,
        }
    }

    fn send(&self, request: Request) {
        let _ = self.req_tx.send(request);
    }

    fn on_update(&mut self, update: Update) {
        match update {
            Update::State(snapshot) => {
                self.snapshot = Some(snapshot);
                self.error = None;
            }
            Update::Error(message) => self.error = Some(message),
        }
    }
}

impl Render for MouseApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.snapshot.clone();
        let error = self.error.clone();

        let mut content = div()
            .id("content")
            .flex()
            .flex_col()
            .gap_4()
            .p_4()
            .flex_1()
            .overflow_y_scroll();

        if let Some(message) = &error {
            content = content.child(banner(message));
        }

        match &snapshot {
            None => content = content.child(placeholder("Connecting to the mouse…")),
            Some(s) => {
                content = content.child(battery_card(s));
                if s.asleep {
                    content = content.child(placeholder(
                        "The mouse is asleep or out of range. Move it to wake it up.",
                    ));
                } else if let Some(config) = &s.config {
                    content = content.child(dpi_card(s, config, cx));
                    content = content.child(rate_card(config, cx));
                    content = content.child(lod_card(config, cx));
                }
            }
        }

        content = content.child(footer(cx));

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(EDITOR_BG))
            .text_color(rgb(TEXT))
            .font_family(FONT)
            .child(title_bar(snapshot.as_ref()))
            .child(content)
    }
}

// ---------------------------------------------------------------------------
// Elements
// ---------------------------------------------------------------------------

fn card() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .rounded_md()
        .bg(rgb(SURFACE))
        .border_1()
        .border_color(rgb(BORDER_VARIANT))
}

fn section_label(text: &str) -> impl IntoElement {
    div()
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(TEXT_MUTED))
        .child(text.to_string().to_uppercase())
}

/// A Zed-style title-bar strip: flat, `title_bar.background`, with a bottom
/// border and the connection state on the right.
fn title_bar(snapshot: Option<&Snapshot>) -> impl IntoElement {
    let (name, dot) = match snapshot {
        Some(s) if s.asleep => (s.name.clone(), WARNING),
        Some(s) => (s.name.clone(), SUCCESS),
        None => ("MCHOSE G7".to_string(), TEXT_FAINT),
    };

    let detail = match snapshot {
        Some(s) => format!(
            "{}  ·  firmware {}  ·  {}",
            if s.wired { "USB wired" } else { "2.4 GHz" },
            s.firmware.clone().unwrap_or_else(|| "—".to_string()),
            s.path
        ),
        None => "looking for the vendor HID interface".to_string(),
    };

    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .h(px(40.))
        .px_3()
        .bg(rgb(TITLEBAR))
        .border_b_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(div().size(px(8.)).rounded_full().bg(rgb(dot)))
                .child(div().text_sm().font_weight(FontWeight::BOLD).child(name))
                .child(div().text_xs().text_color(rgb(TEXT_MUTED)).child(detail)),
        )
        .child(
            div()
                .px_2()
                .py(px(2.))
                .rounded_sm()
                .bg(rgb(ELEMENT))
                .text_xs()
                .text_color(rgb(TEXT_FAINT))
                .child("unofficial"),
        )
}

fn banner(message: &str) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .rounded_md()
        .bg(rgba(0xd072771a))
        .border_1()
        .border_color(rgb(ERROR))
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(ERROR))
                .child("Not connected"),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(TEXT))
                .child(message.to_string()),
        )
}

fn placeholder(message: &str) -> impl IntoElement {
    card().items_center().justify_center().child(
        div()
            .text_sm()
            .text_color(rgb(TEXT_MUTED))
            .child(message.to_string()),
    )
}

fn battery_card(s: &Snapshot) -> impl IntoElement {
    let (level, charging) = s.battery.unwrap_or((0, false));
    let have = s.battery.is_some();
    let color = if !have {
        TEXT_FAINT
    } else if charging {
        SUCCESS
    } else if level <= 15 {
        ERROR
    } else if level <= 30 {
        WARNING
    } else {
        TEXT
    };
    let ratio = (level.min(100) as f32) / 100.0;

    let status = if !have {
        "No reading"
    } else if charging {
        "Charging"
    } else {
        "Discharging"
    };

    card()
        .child(section_label("Battery"))
        .child(
            div()
                .flex()
                .items_end()
                .gap_3()
                .child(
                    div()
                        .text_3xl()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(color))
                        .child(if have {
                            format!("{level}%")
                        } else {
                            "—".to_string()
                        }),
                )
                .child(
                    div()
                        .pb_1()
                        .text_sm()
                        .text_color(rgb(if charging { SUCCESS } else { TEXT_MUTED }))
                        .child(status),
                ),
        )
        .child(
            div()
                .h(px(8.))
                .w_full()
                .rounded_full()
                .bg(rgb(ELEMENT))
                .child(
                    div()
                        .h_full()
                        .w(px(380.0 * ratio))
                        .rounded_full()
                        .bg(rgb(if charging { SUCCESS } else { ACCENT })),
                ),
        )
}

fn dpi_card(s: &Snapshot, config: &Config, cx: &mut Context<MouseApp>) -> impl IntoElement {
    let mut card = card().child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(section_label("DPI stages"))
            .child(div().text_xs().text_color(rgb(TEXT_MUTED)).child(format!(
                "active {} of {}   ·   max {}",
                config.active_index + 1,
                config.dpi_count,
                s.max_dpi
            ))),
    );

    for stage in 0..6usize {
        card = card.child(dpi_row(s, config, stage, cx));
    }
    card
}

fn dpi_row(
    s: &Snapshot,
    config: &Config,
    stage: usize,
    cx: &mut Context<MouseApp>,
) -> impl IntoElement {
    let value = config.dpi[stage];
    let active = stage as u8 == config.active_index;
    let enabled = stage < config.dpi_count.clamp(1, 6) as usize;

    let mut row = div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded_sm()
        .bg(rgb(if active { ELEMENT_ACTIVE } else { ELEMENT }))
        .border_1()
        .border_color(rgb(if active { ACCENT } else { BORDER_VARIANT }))
        .opacity(if enabled { 1.0 } else { 0.55 })
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(px(56.))
                        .text_sm()
                        .text_color(rgb(TEXT_MUTED))
                        .child(format!("Stage {}", stage + 1)),
                )
                .child(
                    div()
                        .w(px(84.))
                        .text_lg()
                        .font_weight(FontWeight::BOLD)
                        .child(format!("{value}")),
                )
                .child(div().text_xs().text_color(rgb(TEXT_MUTED)).child("DPI"))
                .child(div().flex_1())
                .when(active, |d| d.child(pill("ACTIVE", ACCENT)))
                .child(button(
                    &format!("minus-{stage}"),
                    "−",
                    cx.listener(move |this, _: &ClickEvent, _, _| {
                        this.send(Request::NudgeDpi(stage, -50))
                    }),
                ))
                .child(button(
                    &format!("plus-{stage}"),
                    "+",
                    cx.listener(move |this, _: &ClickEvent, _, _| {
                        this.send(Request::NudgeDpi(stage, 50))
                    }),
                ))
                .child(button(
                    &format!("use-{stage}"),
                    "Use",
                    cx.listener(move |this, _: &ClickEvent, _, _| {
                        this.send(Request::SetActive(stage))
                    }),
                )),
        );

    if enabled {
        row = row.child(dpi_bar(s.max_dpi, value, active, stage, cx));
    }
    row
}

/// A clickable segmented bar. Clicking a segment sets the DPI to that
/// fraction of the sensor maximum, snapped to a 50 DPI step.
fn dpi_bar(
    max: u16,
    value: u16,
    active: bool,
    stage: usize,
    cx: &mut Context<MouseApp>,
) -> impl IntoElement {
    const SEGMENTS: usize = 24;
    let filled = ((value as f32 / max as f32) * SEGMENTS as f32).round() as usize;

    let mut bar = div()
        .flex()
        .gap(px(1.))
        .h(px(20.))
        .w_full()
        .p(px(1.))
        .rounded_sm()
        .bg(rgb(ELEMENT));

    for i in 0..SEGMENTS {
        let on = i < filled;
        let target = snap_to_50((((i + 1) as f32 / SEGMENTS as f32) * max as f32) as u16);
        let fill = if on {
            if active { ACCENT } else { ACCENT_DIM }
        } else {
            BORDER_VARIANT
        };
        bar = bar.child(
            div()
                .id(ElementId::Name(SharedString::from(format!(
                    "seg-{stage}-{i}"
                ))))
                .flex_1()
                .h_full()
                .rounded_sm()
                .bg(rgb(fill))
                .cursor_pointer()
                .hover(|s| s.opacity(0.75))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, _| {
                    this.send(Request::SetDpi(stage, target));
                })),
        );
    }
    bar
}

fn rate_card(config: &Config, cx: &mut Context<MouseApp>) -> impl IntoElement {
    let mut row = div().flex().gap_2();
    for (index, hz) in RATES_HZ.iter().enumerate() {
        let selected = config.rate_index as usize == index;
        row = row.child(choice(
            &format!("rate-{hz}"),
            &format!("{hz} Hz"),
            selected,
            cx.listener(move |this, _: &ClickEvent, _, _| this.send(Request::SetRate(index as u8))),
        ));
    }
    card().child(section_label("Report rate")).child(row)
}

fn lod_card(config: &Config, cx: &mut Context<MouseApp>) -> impl IntoElement {
    let mut row = div().flex().gap_2();
    for (index, mm) in LOD_MM.iter().enumerate() {
        let selected = config.lod_index as usize == index;
        row = row.child(choice(
            &format!("lod-{mm}"),
            &format!("{mm} mm"),
            selected,
            cx.listener(move |this, _: &ClickEvent, _, _| this.send(Request::SetLod(index as u8))),
        ));
    }
    card().child(section_label("Lift-off distance")).child(row)
}

fn footer(cx: &mut Context<MouseApp>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_xs()
                .text_color(rgb(TEXT_FAINT))
                .child("Reads back from the mouse after every change."),
        )
        .child(
            div()
                .flex()
                .gap_2()
                .child(button(
                    "refresh",
                    "Refresh",
                    cx.listener(|this, _: &ClickEvent, _, _| this.send(Request::Refresh)),
                ))
                .child(button(
                    "quit",
                    "Quit",
                    cx.listener(|_this, _: &ClickEvent, window, _| window.remove_window()),
                )),
        )
}

// -- small building blocks ---------------------------------------------------

fn button(
    id: &str,
    label: &str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(ElementId::Name(SharedString::from(id.to_string())))
        .flex()
        .items_center()
        .justify_center()
        .min_w(px(30.))
        .h(px(26.))
        .px_2()
        .rounded_sm()
        .bg(rgb(ELEMENT))
        .border_1()
        .border_color(rgb(BORDER_VARIANT))
        .text_color(rgb(TEXT))
        .text_sm()
        .cursor_pointer()
        .hover(|s| s.bg(rgb(ELEMENT_HOVER)).border_color(rgb(BORDER)))
        .active(|s| s.bg(rgb(ELEMENT_ACTIVE)))
        .child(label.to_string())
        .on_click(on_click)
}

fn choice(
    id: &str,
    label: &str,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(ElementId::Name(SharedString::from(id.to_string())))
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .h(px(30.))
        .rounded_sm()
        .border_1()
        .text_sm()
        .cursor_pointer()
        .when(selected, |d| {
            d.bg(rgb(ACCENT))
                .border_color(rgb(ACCENT))
                .text_color(rgb(WHITE))
                .font_weight(FontWeight::BOLD)
        })
        .when(!selected, |d| {
            d.bg(rgb(ELEMENT))
                .border_color(rgb(BORDER_VARIANT))
                .text_color(rgb(TEXT_MUTED))
                .hover(|s| s.bg(rgb(ELEMENT_HOVER)).text_color(rgb(TEXT)))
        })
        .child(label.to_string())
        .on_click(on_click)
}

fn pill(label: &str, color: u32) -> impl IntoElement {
    div()
        .px_2()
        .py(px(2.))
        .rounded_sm()
        .bg(rgb(color))
        .text_color(rgb(0x1b1f27))
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .child(label.to_string())
}

fn snap_to_50(value: u16) -> u16 {
    ((value.max(50) + 25) / 50 * 50).clamp(50, 50_000)
}

// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    Application::new().run(|cx: &mut App| {
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(520.), px(860.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(460.), px(560.))),
                app_id: Some("mchose-gui".to_string()),
                ..Default::default()
            },
            |_window, cx| cx.new(MouseApp::new),
        )
        .expect("failed to open the mchose window");

        cx.activate(true);
    });

    ExitCode::SUCCESS
}
