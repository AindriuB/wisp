//! Wisp: portable HTTP client.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

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

/// The one close path: the window's close button, the last-window backstop and
/// the `WISP_CLOSE_WINDOW_AFTER_MS` hook all quit the app rather than tearing a
/// window down while the process lives on (that teardown crashes on macOS).
fn quit_app(cx: &mut gpui::App) {
    cx.quit();
}

/// Menus: macOS only. GPUI 0.3.10 has no About or Hide item (no `OsAction` or
/// selector for them), so the app menu holds Services and Quit.
#[cfg(target_os = "macos")]
fn install_menus(cx: &mut gpui::App) {
    use gpui::{Menu, MenuItem, SystemMenuType};
    cx.set_menus(vec![Menu::new("Wisp").items([
        MenuItem::os_submenu("Services", SystemMenuType::Services),
        MenuItem::separator(),
        MenuItem::action("Quit Wisp", actions::Quit),
    ])]);
}

fn main() {
    trace("app run start");
    let app = gpui_platform::application().with_assets(assets::Icons);
    app.run(|cx| {
        gpui_component::init(cx);
        actions::bind_keys(cx);
        cx.on_action(|_: &actions::Quit, cx| quit_app(cx));
        #[cfg(target_os = "macos")]
        install_menus(cx);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                quit_app(cx);
            }
        })
        .detach();
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
            window.on_window_should_close(cx, |_, cx| {
                quit_app(cx);
                false
            });
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
        if let Some(ms) = std::env::var("WISP_CLOSE_WINDOW_AFTER_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
        {
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(ms))
                    .await;
                cx.update(quit_app);
            })
            .detach();
        }
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
