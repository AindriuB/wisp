//! Wisp walking-skeleton window: one request bar, one response pane.

mod format;
mod view;

use gpui::{AppContext as _, WindowOptions};
use gpui_component::Root;

fn main() {
    gpui_platform::application().run(|cx| {
        gpui_component::init(cx);
        let opened = cx.open_window(WindowOptions::default(), |window, cx| {
            let view = cx.new(|cx| view::WispView::new(window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        });
        if let Err(err) = opened {
            eprintln!("wisp: failed to open window: {err:#}");
            cx.quit();
            return;
        }
        cx.activate(true);
    });
}
