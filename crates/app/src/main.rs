//! Wisp: portable HTTP client.

mod assets;

mod format;

mod actions;
mod cookies;
mod draft;
mod highlight;
mod pager;
mod palette;
mod pretty;
mod settings;
mod tab;
mod theme;
mod ui;
mod url_sync;
mod workspace;

use gpui::{AppContext as _, WindowOptions, px, size};
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

/// Smallest window the layout is designed for (logical px).
const MIN_WINDOW_WIDTH: f32 = 720.;
const MIN_WINDOW_HEIGHT: f32 = 480.;

fn window_options() -> WindowOptions {
    WindowOptions {
        window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
        ..WindowOptions::default()
    }
}

fn main() {
    trace("app run start");
    let app = gpui_platform::application().with_assets(assets::Icons);
    app.run(|cx| {
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
        let opened = cx.open_window(window_options(), |window, cx| {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_has_a_720_by_480_minimum() {
        let min = window_options().window_min_size.expect("min size set");
        assert_eq!(min, size(px(720.), px(480.)));
    }
}
