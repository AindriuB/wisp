//! Wisp walking-skeleton window: one request bar, one response pane.

mod format;
mod view;

use gpui::{AppContext as _, WindowOptions};
use gpui_component::Root;

fn main() {
    view::trace("app run start");
    gpui_platform::application().run(|cx| {
        gpui_component::init(cx);
        view::trace("before open_window");
        let opened = cx.open_window(WindowOptions::default(), |window, cx| {
            let view = cx.new(|cx| view::WispView::new(window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        });
        if let Err(err) = opened {
            eprintln!("wisp: failed to open window: {err:#}");
            cx.quit();
            // Quitting alone may leave the process alive; fail loudly instead.
            std::process::exit(1);
        }
        cx.activate(true);
    });
}
