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

use gpui::{AppContext as _, Entity, PromptLevel, Window, WindowOptions, px, size};
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

/// What a quit request does.
#[derive(Debug, PartialEq, Eq)]
enum QuitStep {
    Quit,
    Prompt,
    Nothing,
}

/// Decide a quit request from (any tab dirty, a prompt already up).
fn quit_step(any_dirty: bool, prompt_active: bool) -> QuitStep {
    match (any_dirty, prompt_active) {
        (false, _) => QuitStep::Quit,
        (true, false) => QuitStep::Prompt,
        (true, true) => QuitStep::Nothing,
    }
}

/// The single quit request: the `Quit` action and the window's close button.
/// Asks before discarding unsaved drafts.
fn request_quit(ws: &Entity<Workspace>, window: &mut Window, cx: &mut gpui::App) {
    let active = window.has_active_prompt() || ws.read(cx).prompt_up;
    match quit_step(ws.read(cx).any_dirty(cx), active) {
        QuitStep::Nothing => {}
        QuitStep::Quit => quit_app(cx),
        QuitStep::Prompt => {
            ws.update(cx, |w, _| w.prompt_up = true);
            let answer = window.prompt(
                PromptLevel::Warning,
                "Discard unsaved changes?",
                Some("Quitting closes all tabs."),
                &["Discard", "Cancel"],
                cx,
            );
            let ws = ws.clone();
            cx.spawn(async move |cx| {
                let discard = matches!(answer.await, Ok(0));
                cx.update(|cx| {
                    ws.update(cx, |w, _| w.prompt_up = false);
                    if discard {
                        quit_app(cx);
                    }
                });
            })
            .detach();
        }
    }
}

/// Menus: macOS only. GPUI 0.3.10 has no About item (no `OsAction` or
/// selector for it), so the app menu holds Services, Hide, Show All and Quit.
#[cfg(target_os = "macos")]
fn install_menus(cx: &mut gpui::App) {
    use gpui::{Menu, MenuItem, SystemMenuType};
    cx.set_menus(vec![Menu::new("Wisp").items([
        MenuItem::os_submenu("Services", SystemMenuType::Services),
        MenuItem::separator(),
        MenuItem::action("Hide Wisp", actions::HideApp),
        MenuItem::action("Hide Others", actions::HideOthers),
        MenuItem::action("Show All", actions::ShowAll),
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
        #[cfg(target_os = "macos")]
        {
            cx.on_action(|_: &actions::HideApp, cx| cx.hide());
            cx.on_action(|_: &actions::HideOthers, cx| cx.hide_other_apps());
            cx.on_action(|_: &actions::ShowAll, cx| cx.unhide_other_apps());
            install_menus(cx);
        }
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
        let workspace = cx.new(|cx| Workspace::new(loaded, settings_path, open, cx));
        let ws_for_window = workspace.clone();
        let opened = cx.open_window(window_options(), |window, cx| {
            let workspace = ws_for_window;
            let view = cx.new(|cx| WorkspaceView::new(workspace.clone(), window, cx));
            window.on_window_should_close(cx, move |window, cx| {
                request_quit(&workspace, window, cx);
                false
            });
            trace("window opened");
            cx.new(|cx| Root::new(view, window, cx))
        });
        if let Ok(handle) = &opened {
            let handle = *handle;
            cx.on_action(move |_: &actions::Quit, cx| {
                let ws = workspace.clone();
                let _ = handle.update(cx, |_, window, cx| request_quit(&ws, window, cx));
            });
        }
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
    fn quit_step_no_dirty_quits() {
        assert_eq!(quit_step(false, false), QuitStep::Quit);
        assert_eq!(quit_step(false, true), QuitStep::Quit);
    }

    #[test]
    fn quit_step_dirty_prompts() {
        assert_eq!(quit_step(true, false), QuitStep::Prompt);
    }

    #[test]
    fn quit_step_dirty_with_prompt_up_does_nothing() {
        assert_eq!(quit_step(true, true), QuitStep::Nothing);
    }

    #[test]
    fn window_has_a_720_by_480_minimum() {
        let min = window_options().window_min_size.expect("min size set");
        assert_eq!(min, size(px(720.), px(480.)));
    }
}
