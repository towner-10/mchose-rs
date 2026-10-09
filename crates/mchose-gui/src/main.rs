//! `mchose-gui` - a GPUI desktop front-end for the MCHOSE G7.
//!
//! The visual language follows Zed's default **One Dark** theme: the same
//! surfaces, borders, text and accent colours, a title-bar strip, flat panels
//! and small corner radii.
//!
//! Structure:
//! - [`theme`] - the Zed One Dark colour tokens
//! - [`worker`] - owns the mouse on a background thread
//! - [`app`] - the root view and its link to the worker
//! - [`ui`] - the presentational building blocks
//!
//! Build with: `cargo run -p mchose-gui --release`

mod app;
mod theme;
mod ui;
mod worker;

use std::process::ExitCode;

use gpui::{App, AppContext as _, Application, Bounds, WindowBounds, WindowOptions, px, size};

use crate::app::MouseApp;

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
