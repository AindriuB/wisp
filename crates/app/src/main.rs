//! Wisp: portable HTTP client.

// kept for the response pane (task 31+), allow removed by 35
#[allow(dead_code)]
mod format;

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

use crate::ui::root::WorkspaceView;
use crate::workspace::Workspace;

/// Startup checkpoint on stderr, only when the measurement hook is active, so a
/// hang can be located from the captured stderr.
pub fn trace(stage: &str) {
    if std::env::var_os("WISP_AUTOSEND_URL").is_some() {
        eprintln!("WISP_TRACE {stage}");
    }
}

fn main() {
    trace("app run start");
    gpui_platform::application().run(|cx| {
        gpui_component::init(cx);
        actions::bind_keys(cx);
        let settings_path = settings::settings_path();
        let loaded = match &settings_path {
            Some(path) => {
                let (loaded, warning) = settings::load(path);
                if let Some(w) = warning {
                    eprintln!("wisp: settings: {w}");
                }
                loaded
            }
            None => settings::Settings::default(),
        };
        theme::apply(loaded.theme, None, cx);
        let open = std::env::args_os().nth(1).map(std::path::PathBuf::from);
        trace("before open_window");
        let opened = cx.open_window(WindowOptions::default(), |window, cx| {
            let workspace = cx.new(|cx| Workspace::new(loaded, settings_path, open, cx));
            let view = cx.new(|cx| WorkspaceView::new(workspace, window, cx));
            trace("window opened");
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
