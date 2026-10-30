//! Wisp walking-skeleton window: one request bar, one response pane.

mod format;
mod view;

// mounted by 28, allow removed by 35
#[allow(dead_code)]
mod actions;
// mounted by 28, allow removed by 35
#[allow(dead_code)]
mod cookies;
// mounted by 28, allow removed by 35
#[allow(dead_code)]
mod draft;
// mounted by 28, allow removed by 35
#[allow(dead_code)]
mod highlight;
// mounted by 28, allow removed by 35
#[allow(dead_code)]
mod pager;
// mounted by 28, allow removed by 35
#[allow(dead_code)]
mod palette;
// mounted by 28, allow removed by 35
#[allow(dead_code)]
mod pretty;
// mounted by 28, allow removed by 35
#[allow(dead_code)]
mod settings;
// mounted by 28, allow removed by 35
#[allow(dead_code)]
mod tab;
// mounted by 28, allow removed by 35
#[allow(dead_code)]
mod theme;
// mounted by 28, allow removed by 35
#[allow(dead_code)]
mod url_sync;
// mounted by 28, allow removed by 35
#[allow(dead_code)]
mod ui;
// mounted by 28, allow removed by 35
#[allow(dead_code)]
mod workspace;

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
