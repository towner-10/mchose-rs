//! Presentational building blocks. Everything here is a pure function of a
//! [`Snapshot`] plus a context for event listeners; no state lives here.

use gpui::{
    App, ClickEvent, Context, ElementId, FontWeight, SharedString, Window, div, prelude::*, px,
    rgb, rgba,
};
use mchose_core::protocol::{Config, LOD_MM, RATES_HZ};

use crate::app::MouseApp;
use crate::theme::*;
use crate::worker::{Request, Snapshot};

/// A flat panel, the base surface for every group of controls.
pub fn card() -> gpui::Div {
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

pub fn section_label(text: &str) -> impl IntoElement {
    div()
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(TEXT_MUTED))
        .child(text.to_string().to_uppercase())
}

/// A Zed-style title-bar strip: flat, `title_bar.background`, with a bottom
/// border and the connection state on the right.
pub fn title_bar(snapshot: Option<&Snapshot>) -> impl IntoElement {
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

pub fn banner(message: &str) -> impl IntoElement {
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

pub fn placeholder(message: &str) -> impl IntoElement {
    card().items_center().justify_center().child(
        div()
            .text_sm()
            .text_color(rgb(TEXT_MUTED))
            .child(message.to_string()),
    )
}

pub fn battery_card(s: &Snapshot) -> impl IntoElement {
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

pub fn dpi_card(s: &Snapshot, config: &Config, cx: &mut Context<MouseApp>) -> impl IntoElement {
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

pub fn rate_card(config: &Config, cx: &mut Context<MouseApp>) -> impl IntoElement {
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

pub fn lod_card(config: &Config, cx: &mut Context<MouseApp>) -> impl IntoElement {
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

pub fn footer(cx: &mut Context<MouseApp>) -> impl IntoElement {
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

pub fn button(
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

pub fn choice(
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

pub fn pill(label: &str, color: u32) -> impl IntoElement {
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
