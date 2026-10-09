//! The root view: owns the latest [`Snapshot`] and talks to the [`worker`].
//!
//! [`worker`]: crate::worker

use std::sync::mpsc::{self, Sender, TryRecvError};
use std::time::Duration;

use gpui::{Context, Render, Window, div, prelude::*, rgb};

use crate::theme::{EDITOR_BG, FONT, TEXT};
use crate::ui;
use crate::worker::{self, Request, Snapshot, Update};

/// How often the view drains the worker's update channel.
const TICK: Duration = Duration::from_millis(150);

pub struct MouseApp {
    snapshot: Option<Snapshot>,
    error: Option<String>,
    req_tx: Sender<Request>,
}

impl MouseApp {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let (req_tx, req_rx) = mpsc::channel();
        let (upd_tx, upd_rx) = mpsc::channel();
        worker::spawn(req_rx, upd_tx);

        // Poll the worker on GPUI's executor: never block the UI on HID I/O.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
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

    /// Queue a request for the worker.
    pub fn send(&self, request: Request) {
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
            content = content.child(ui::banner(message));
        }

        match &snapshot {
            None => content = content.child(ui::placeholder("Connecting to the mouse…")),
            Some(s) => {
                content = content.child(ui::battery_card(s));
                if s.asleep {
                    content = content.child(ui::placeholder(
                        "The mouse is asleep or out of range. Move it to wake it up.",
                    ));
                } else if let Some(config) = &s.config {
                    content = content.child(ui::dpi_card(s, config, cx));
                    content = content.child(ui::rate_card(config, cx));
                    content = content.child(ui::lod_card(config, cx));
                }
            }
        }

        content = content.child(ui::footer(cx));

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(EDITOR_BG))
            .text_color(rgb(TEXT))
            .font_family(FONT)
            .child(ui::title_bar(snapshot.as_ref()))
            .child(content)
    }
}
