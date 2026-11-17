//! Root layout: sidebar | (tab bar, disk banner, request pane / response pane).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui::{
    AppContext as _, Context, DismissEvent, Entity, EntityId, FocusHandle, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement, PathPromptOptions, PromptLevel, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{ActiveTheme as _, IconName, Sizable as _};

use crate::actions::{
    CancelRequest, ClearHistory, CloseCollection, CloseTab, KeepMine, NewCollection,
    NewEnvironment, NewRequest, NextTab, OpenCollection, OpenImport, OpenPalette, PrevTab,
    ReloadFromDisk, SaveRequest, SendRequest, ShareKeychainSecrets, ShowCookieJar, ShowEnvSecrets,
    ShowHistory, SwitchEnvironment, ToggleConfirmSend, ToggleHistoryBodies, ToggleLayout,
    ToggleTheme,
};
use crate::guard::{self, Answer, Next};
use crate::palette::PaletteMode;
use crate::save_close::{self, ClosePrompt};
use crate::settings::{SplitAxis, ThemeMode};
use crate::tab::{DiskState, RequestTab};
use crate::theme;
use crate::ui::collections::CloseRequested;
use crate::ui::cookie_jar::CookieJarView;
use crate::ui::create::{self, NewEnvironmentView};
use crate::ui::env_secrets::EnvSecretsView;
use crate::ui::history::HistoryView;
use crate::ui::import::ImportView;
use crate::ui::palette::PaletteView;
use crate::ui::request::RequestPane;
use crate::ui::response::ResponsePane;
use crate::ui::sidebar::Sidebar;
use crate::ui::url_bar::{UrlBar, active_guard_state};
use crate::vault::{BannerAction, EnvAction, PromptSpec};
use crate::workspace::{CloseStep, Workspace, WorkspaceEvent, close_step, confirmed_open};

/// Close `tab`, asking first when it holds unsaved changes. After the answer
/// the tab is looked up by entity: its index may have moved or it may be gone.
fn request_close_tab(
    ws: &Entity<Workspace>,
    tab: &Entity<RequestTab>,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let (dirty, title) = {
        let t = tab.read(cx);
        (t.is_dirty(), t.title())
    };
    let active = window.has_active_prompt() || ws.read(cx).prompt_up;
    match close_step(dirty, active) {
        CloseStep::Nothing => {}
        CloseStep::Close => ws.update(cx, |w, cx| w.close_tab_entity(tab, cx)),
        CloseStep::Prompt => {
            ws.update(cx, |w, _| w.prompt_up = true);
            let can_save = ws
                .read(cx)
                .collections
                .iter()
                .any(|c| c.group.tabs.contains(tab));
            let prompt = ClosePrompt::Tab { can_save };
            let detail = if can_save {
                format!("\"{title}\" has unsaved changes.")
            } else {
                "Open a collection to save".into()
            };
            let answer = window.prompt(
                PromptLevel::Warning,
                "Close tab with unsaved changes?",
                Some(&detail),
                prompt.buttons(),
                cx,
            );
            let (ws, tab) = (ws.clone(), tab.clone());
            cx.spawn(async move |cx| {
                let button = answer.await.ok();
                ws.update(cx, |w, cx| {
                    if save_close::finish(prompt, button, std::slice::from_ref(&tab), w, cx) {
                        w.close_tab_entity(&tab, cx);
                    }
                });
            })
            .detach();
        }
    }
}

/// Views belonging to one open tab, created lazily on first render.
struct TabViews {
    url_bar: Entity<UrlBar>,
    request: Entity<RequestPane>,
    response: Entity<ResponsePane>,
    _sub: Subscription,
}

pub struct WorkspaceView {
    workspace: Entity<Workspace>,
    sidebar: Entity<Sidebar>,
    views: HashMap<EntityId, TabViews>,
    palette: Option<(Entity<PaletteView>, Subscription)>,
    import: Option<(Entity<ImportView>, Subscription)>,
    new_env: Option<(Entity<NewEnvironmentView>, Subscription)>,
    history: Option<(Entity<HistoryView>, Subscription)>,
    cookies: Option<(Entity<CookieJarView>, Subscription)>,
    secrets: Option<(Entity<EnvSecretsView>, Subscription)>,
    notice: Option<SharedString>,
    /// Collection roots and environments already offered the `suggest_confirm` hint this run.
    hint_shown: HashSet<(PathBuf, String)>,
    focus: FocusHandle,
    _workspace_sub: Subscription,
    _sidebar_sub: Subscription,
    _appearance: Subscription,
}

/// Narrowest request or response pane in the horizontal split, in px.
const MIN_PANE_WIDTH: f32 = 240.;
/// Shortest request or response pane in the vertical split, in px.
const MIN_PANE_HEIGHT: f32 = 140.;

/// Widest the sidebar may be: leaves 480 px for the main area, within 120..=480.
fn sidebar_max(window_width: f32) -> f32 {
    (window_width - 480.).clamp(120., 480.)
}

/// The OS appearance is followed only while the setting is `System`.
fn follows_system(mode: ThemeMode) -> bool {
    mode == ThemeMode::System
}

/// Focus must come back to the root only when it is unchanged since the
/// render that scheduled the check (so focus given to an element created
/// after that frame is never taken), no in-window prompt is up, and the
/// focused element is not part of the drawn tree (its view unmounted).
/// Future gpui-component Root dialogs/sheets/notifications need the same
/// guard as prompts.
fn should_reclaim(focus_unchanged: bool, prompt_active: bool, focus_in_tree: bool) -> bool {
    focus_unchanged && !prompt_active && !focus_in_tree
}

fn next_theme(mode: ThemeMode) -> ThemeMode {
    match mode {
        ThemeMode::System => ThemeMode::Light,
        ThemeMode::Light => ThemeMode::Dark,
        ThemeMode::Dark => ThemeMode::System,
    }
}

impl WorkspaceView {
    pub fn new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sidebar = cx.new(|cx| Sidebar::new(workspace.clone(), window, cx));
        let _workspace_sub = cx.subscribe_in(&workspace, window, |this, _, event, window, cx| {
            match event {
                WorkspaceEvent::Notice(text) => this.notice = Some(text.clone()),
                WorkspaceEvent::Prompt(spec) => this.show_prompt(spec.clone(), window, cx),
                WorkspaceEvent::SuggestConfirm { root, env } => {
                    this.offer_guard(root.clone(), env.clone(), window, cx)
                }
                _ => {}
            }
            cx.notify();
        });
        let _sidebar_sub = cx.subscribe_in(
            &sidebar,
            window,
            |this, _, ev: &CloseRequested, window, cx| {
                if let Some(ix) = this.workspace.read(cx).coll_ix(&ev.0) {
                    this.close_collection_at(ix, window, cx);
                }
            },
        );
        let ws = workspace.clone();
        let _appearance = window.observe_window_appearance(move |window, cx| {
            let mode = ws.read(cx).settings.theme;
            if !follows_system(mode) {
                return;
            }
            theme::apply(mode, Some(window), cx);
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            workspace,
            sidebar,
            views: HashMap::new(),
            palette: None,
            import: None,
            new_env: None,
            history: None,
            cookies: None,
            secrets: None,
            notice: None,
            hint_shown: HashSet::new(),
            focus,
            _workspace_sub,
            _sidebar_sub,
            _appearance,
        }
    }

    fn open_palette(&mut self, mode: PaletteMode, window: &mut Window, cx: &mut Context<Self>) {
        let palette = cx.new(|cx| PaletteView::new(self.workspace.clone(), mode, window, cx));
        let sub = cx.subscribe(&palette, |this, _, _: &DismissEvent, cx| {
            this.palette = None;
            cx.notify();
        });
        self.palette = Some((palette, sub));
        cx.notify();
    }

    fn open_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dialog = cx.new(|cx| ImportView::new(self.workspace.clone(), window, cx));
        let sub = cx.subscribe(&dialog, |this, _, _: &DismissEvent, cx| {
            this.import = None;
            cx.notify();
        });
        self.import = Some((dialog, sub));
        cx.notify();
    }

    fn toggle_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.take().is_none() {
            let panel = cx.new(|cx| HistoryView::new(self.workspace.clone(), window, cx));
            let sub = cx.subscribe(&panel, |this, _, _: &DismissEvent, cx| {
                this.history = None;
                cx.notify();
            });
            self.history = Some((panel, sub));
        }
        cx.notify();
    }

    fn toggle_secrets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.secrets.take().is_none() {
            let panel = cx.new(|cx| EnvSecretsView::new(self.workspace.clone(), window, cx));
            let sub = cx.subscribe(&panel, |this, _, _: &DismissEvent, cx| {
                this.secrets = None;
                cx.notify();
            });
            self.secrets = Some((panel, sub));
        }
        cx.notify();
    }

    /// Ask the user, then run the action behind the button they pressed. Cancel runs nothing.
    fn show_prompt(&mut self, spec: PromptSpec, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_prompt() || self.workspace.read(cx).prompt_up {
            self.workspace.update(cx, |_, cx| {
                cx.emit(WorkspaceEvent::Notice(
                    "Answer the open prompt first, then try again".into(),
                ))
            });
            return;
        }
        self.workspace.update(cx, |w, _| w.prompt_up = true);
        let buttons: Vec<&str> = spec.buttons.iter().map(String::as_str).collect();
        let answer = window.prompt(
            PromptLevel::Warning,
            &spec.title,
            Some(&spec.detail),
            &buttons,
            cx,
        );
        let ws = self.workspace.clone();
        cx.spawn(async move |_, cx| {
            let pressed = answer.await.ok();
            ws.update(cx, |w, cx| {
                w.prompt_up = false;
                if let Some(i) = pressed {
                    w.answer_prompt(&spec, i, cx);
                }
            });
        })
        .detach();
    }

    /// "Share keychain secrets with another folder": pick the folder, then confirm.
    fn on_share_secrets(&mut self, cx: &mut Context<Self>) {
        if !self.workspace.read(cx).can_share_secrets() {
            self.workspace.update(cx, |_, cx| {
                cx.emit(WorkspaceEvent::Notice(
                    "Not shared: keychain secrets are not enabled for this folder".into(),
                ))
            });
            return;
        }
        let ws = self.workspace.clone();
        cx.spawn(async move |_, cx| {
            let rx = cx.update(|cx| {
                cx.prompt_for_paths(PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: None,
                })
            });
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(path) = paths.first()
            {
                ws.update(cx, |w, cx| w.request_share(path.clone(), cx));
            }
        })
        .detach();
    }

    /// The banners for the active collection's environment and kept-in-memory refresh tokens.
    fn env_banners(&self, cx: &mut Context<Self>) -> Vec<gpui::Div> {
        let (banners, tokens) = {
            let w = self.workspace.read(cx);
            let banners = w
                .active_collection()
                .and_then(|c| c.env_status.as_ref())
                .map(|s| s.banners())
                .unwrap_or_default();
            (banners, w.token_banners.clone())
        };
        let strip = |text: String| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .bg(theme::banner_bg(cx.theme()))
                .text_color(cx.theme().foreground)
                .border_b_1()
                .border_color(cx.theme().border)
                .child(div().flex_1().child(text))
        };
        let mut out = Vec::new();
        for (i, b) in banners.into_iter().enumerate() {
            let mut row = strip(b.text);
            if let Some(a) = b.action {
                let (label, action) = match a {
                    BannerAction::UseKeychain => {
                        ("Use keychain secrets…", EnvAction::Bind { retry: None })
                    }
                    BannerAction::SameCollectionMoved => (
                        "This is the same collection, moved",
                        EnvAction::PrepareRebind,
                    ),
                };
                row = row.child(
                    Button::new(("env-banner", i))
                        .label(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let action = action.clone();
                            this.workspace
                                .update(cx, |w, cx| w.request_env_action(action, cx));
                        })),
                );
            }
            out.push(row);
        }
        out.extend(tokens.into_iter().map(strip));
        out
    }

    fn toggle_cookies(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.cookies.take().is_none() {
            let panel = cx.new(|cx| CookieJarView::new(self.workspace.clone(), window, cx));
            let sub = cx.subscribe(&panel, |this, _, _: &DismissEvent, cx| {
                this.cookies = None;
                cx.notify();
            });
            self.cookies = Some((panel, sub));
        }
        cx.notify();
    }

    fn on_clear_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((root, name)) = self.workspace.read(cx).active_collection().map(|c| {
            (
                c.tree.root.clone(),
                c.tree.root.file_name().map_or_else(
                    || c.tree.root.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                ),
            )
        }) else {
            self.workspace.update(cx, |_, cx| {
                cx.emit(WorkspaceEvent::Notice("Open a collection first".into()))
            });
            return;
        };
        if window.has_active_prompt() || self.workspace.read(cx).prompt_up {
            return;
        }
        self.workspace.update(cx, |w, _| w.prompt_up = true);
        let answer = window.prompt(
            PromptLevel::Warning,
            "Clear history?",
            Some(&format!(
                "This removes all history for \"{name}\". It cannot be undone."
            )),
            &["Clear", "Cancel"],
            cx,
        );
        let ws = self.workspace.clone();
        cx.spawn(async move |_, cx| {
            let clear = matches!(answer.await, Ok(0));
            ws.update(cx, |w, cx| {
                w.prompt_up = false;
                // The collection captured before the prompt, if it is still open.
                if confirmed_open(clear, w.coll_ix(&root)).is_some() {
                    w.clear_history(root, cx);
                }
            });
        })
        .detach();
    }

    fn with_active(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Workspace, &Entity<RequestTab>, &mut Context<Workspace>),
    ) {
        self.workspace.update(cx, |w, cx| {
            if let Some(tab) = w.active_tab() {
                f(w, &tab, cx);
            }
        });
    }

    /// The one interactive send entry point: asks first when the target is guarded.
    fn on_send(&mut self, _: &SendRequest, window: &mut Window, cx: &mut Context<Self>) {
        let Some((tab, target, guarded)) = active_guard_state(self.workspace.read(cx)) else {
            return;
        };
        if guarded {
            self.confirm_send(tab, target, window, cx);
        } else {
            self.send_now(&tab, cx);
        }
    }

    fn send_now(&mut self, tab: &Entity<RequestTab>, cx: &mut Context<Self>) {
        self.workspace.update(cx, |w, cx| {
            if let Err(e) = w.send(tab, cx)
                && crate::workspace::refused_body_message(&e).is_none()
            {
                let text = format!("Cannot send: {}", crate::workspace::describe_problems(&e));
                cx.emit(WorkspaceEvent::Notice(text.into()));
            }
        });
    }

    /// Show a prompt through `prompt_up`; `done` gets the pressed button (`None`: dismissed).
    /// False, with a notice, when another prompt is up.
    fn ask(
        &mut self,
        (title, detail): (&str, &str),
        buttons: &[&str],
        notify: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Option<usize>, &mut Window, &mut Context<Self>) + 'static,
    ) -> bool {
        if window.has_active_prompt() || self.workspace.read(cx).prompt_up {
            if notify {
                self.workspace.update(cx, |_, cx| {
                    cx.emit(WorkspaceEvent::Notice(
                        "Answer the open prompt first, then try again".into(),
                    ))
                });
            }
            return false;
        }
        self.workspace.update(cx, |w, _| w.prompt_up = true);
        let answer = window.prompt(PromptLevel::Warning, title, Some(detail), buttons, cx);
        cx.spawn_in(window, async move |this, cx| {
            let pressed = answer.await.ok();
            this.update_in(cx, |this, window, cx| {
                this.workspace.update(cx, |w, _| w.prompt_up = false);
                done(this, pressed, window, cx);
            })
            .ok();
        })
        .detach();
        true
    }

    fn confirm_send(
        &mut self,
        tab: Entity<RequestTab>,
        target: guard::Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let env = target.env.clone().unwrap_or_default();
        let detail = {
            let d = &tab.read(cx).draft;
            format!(
                "{} {}",
                format!("{:?}", d.method).to_uppercase(),
                guard::host_of(&d.url)
            )
        };
        self.ask(
            (&format!("Send to {env}?"), &detail),
            &["Send", "Cancel"],
            true,
            window,
            cx,
            move |this, pressed, window, cx| {
                let answer = if pressed == Some(0) {
                    Answer::Confirm
                } else {
                    Answer::Cancel
                };
                let active = active_guard_state(this.workspace.read(cx));
                let guarded = active.as_ref().is_some_and(|a| a.2);
                let current = active.as_ref().map(|a| a.1.clone());
                match guard::after_prompt(&target, current.as_ref(), answer, guarded) {
                    Next::Send => {
                        if let Some((active, _, _)) = &active {
                            this.send_now(active, cx);
                        }
                    }
                    Next::Abort => {}
                    Next::Reprompt => {
                        if let Some((active, current, _)) = active {
                            this.confirm_send(active, current, window, cx);
                        }
                    }
                    Next::AbortChanged => this.workspace.update(cx, |_, cx| {
                        cx.emit(WorkspaceEvent::Notice(
                            "Environment changed; not sent".into(),
                        ))
                    }),
                }
            },
        );
    }

    /// Flip the guard of the active collection's active environment.
    fn toggle_guard(&mut self, cx: &mut Context<Self>) {
        self.set_guard(None, cx);
    }

    /// Set (`Some`) or flip (`None`) the guard of the active collection's active environment.
    fn set_guard(&mut self, force: Option<bool>, cx: &mut Context<Self>) {
        const SUFFIX: &str = "set per user; a renamed environment loses it";
        self.workspace.update(cx, |w, cx| {
            let ix = w.active_collection_ix();
            let env = ix
                .and_then(|i| w.collections.get(i))
                .and_then(|c| c.active_env.as_ref().map(|e| e.name.clone()));
            let (Some(ix), Some(env)) = (ix, env) else {
                cx.emit(WorkspaceEvent::Notice("Select an environment first".into()));
                return;
            };
            let Some(root) = w.collection_root(ix) else {
                return;
            };
            let apply = move |w: &mut Workspace, cx: &mut Context<Workspace>| {
                let Some(id) = w.coll_ix(&root).and_then(|i| w.known_collection_id(i)) else {
                    cx.emit(WorkspaceEvent::Notice(
                        "Could not create the collection id; guard not changed".into(),
                    ));
                    return;
                };
                let key = crate::settings::GuardKey {
                    collection_id: id,
                    environment: env.clone(),
                };
                let on = force.unwrap_or_else(|| !w.settings.confirm_send.contains(&key));
                w.update_settings(
                    |s| {
                        if on {
                            s.confirm_send.insert(key);
                        } else {
                            s.confirm_send.remove(&key);
                        }
                    },
                    cx,
                );
                let state = if on { "on" } else { "off" };
                cx.emit(WorkspaceEvent::Notice(
                    format!("Confirm before send is {state} for {env}; {SUFFIX}").into(),
                ));
            };
            if w.known_collection_id(ix).is_some() {
                apply(w, cx);
            } else {
                let root = w.collection_root(ix);
                if let Some(root) = root {
                    w.resolve_id_for_guard(root, cx, apply);
                }
            }
        });
    }

    /// The environment commits `suggest_confirm = true`: offer once per run to turn the guard on.
    fn offer_guard(
        &mut self,
        root: PathBuf,
        env: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = (root.clone(), env.clone());
        if self.hint_shown.contains(&key) {
            return;
        }
        let guarded = {
            let ws = self.workspace.read(cx);
            ws.coll_ix(&root).is_some_and(|ix| {
                guard::needs_confirm(
                    &ws.settings,
                    ws.known_collection_id(ix).as_deref(),
                    Some(&env),
                )
            })
        };
        if guarded {
            return;
        }
        let active = {
            let ws = self.workspace.read(cx);
            ws.coll_ix(&root) == ws.active_collection_ix()
        };
        if !active {
            return;
        }
        let shown = self.ask(
            (
                &format!("{env}: This environment suggests confirming sends. Enable?"),
                "The choice is yours and is stored per user.",
            ),
            &["Enable", "Not now"],
            false,
            window,
            cx,
            move |this, pressed, _, cx| {
                let still_active = {
                    let ws = this.workspace.read(cx);
                    ws.active_collection().is_some_and(|c| {
                        c.tree.root == root && c.active_env.as_ref().is_some_and(|e| e.name == env)
                    })
                };
                if pressed == Some(0) && still_active {
                    this.set_guard(Some(true), cx);
                }
            },
        );
        if shown {
            self.hint_shown.insert(key);
        }
    }

    fn on_save(&mut self, _: &SaveRequest, _: &mut Window, cx: &mut Context<Self>) {
        self.with_active(cx, |w, tab, cx| {
            if let Err(e) = w.save(tab, cx) {
                cx.emit(WorkspaceEvent::Notice(format!("Save failed: {e:#}").into()));
            }
        });
    }

    fn on_open_collection(&mut self, _: &OpenCollection, _: &mut Window, cx: &mut Context<Self>) {
        // Opening adds a collection beside the open ones; nothing is discarded.
        let ws = self.workspace.clone();
        cx.spawn(async move |_, cx| {
            let rx = cx.update(|cx| {
                cx.prompt_for_paths(PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: None,
                })
            });
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(path) = paths.first()
            {
                ws.update(cx, |w, cx| {
                    if let Err(e) = w.open_collection(path, cx) {
                        cx.emit(WorkspaceEvent::Notice(format!("Cannot open: {e:#}").into()));
                    }
                });
            }
        })
        .detach();
    }

    fn on_new_collection(&mut self, _: &NewCollection, _: &mut Window, cx: &mut Context<Self>) {
        let ws = self.workspace.clone();
        cx.spawn(async move |_, cx| {
            let rx = cx.update(|cx| {
                cx.prompt_for_paths(PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: Some("Create".into()),
                })
            });
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(path) = paths.first()
            {
                let outcome =
                    create::collection_outcome(&wisp_storage::ops::create_collection(path));
                let (notice, open) = create::collection_notice(&outcome);
                ws.update(cx, |w, cx| {
                    if let Some(n) = notice {
                        cx.emit(WorkspaceEvent::Notice(n.into()));
                    }
                    if open && let Err(e) = w.open_collection(path, cx) {
                        cx.emit(WorkspaceEvent::Notice(format!("Cannot open: {e:#}").into()));
                    }
                });
            }
        })
        .detach();
    }

    fn on_new_environment(
        &mut self,
        _: &NewEnvironment,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.read(cx).active_collection_ix().is_none() {
            self.workspace.update(cx, |_, cx| {
                cx.emit(WorkspaceEvent::Notice("Open a collection first".into()))
            });
            return;
        }
        if self.new_env.is_some() {
            return;
        }
        let dialog = cx.new(|cx| NewEnvironmentView::new(self.workspace.clone(), window, cx));
        let sub = cx.subscribe(&dialog, |this, _, _: &DismissEvent, cx| {
            this.new_env = None;
            cx.notify();
        });
        self.new_env = Some((dialog, sub));
        cx.notify();
    }

    fn on_close_collection(
        &mut self,
        _: &CloseCollection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.workspace.read(cx).active_collection_ix() else {
            self.workspace.update(cx, |_, cx| {
                cx.emit(WorkspaceEvent::Notice("No collection is open".into()))
            });
            return;
        };
        self.close_collection_at(ix, window, cx);
    }

    /// Close collection `ix`, asking first when it has unsaved tabs. The palette action and
    /// the sidebar switcher both come through here.
    fn close_collection_at(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.workspace.read(cx).collection_dirty(ix, cx) {
            self.workspace
                .update(cx, |w, cx| w.close_collection(ix, cx));
            return;
        }
        if window.has_active_prompt() || self.workspace.read(cx).prompt_up {
            return;
        }
        let Some(root) = self.workspace.read(cx).collection_root(ix) else {
            return;
        };
        self.workspace.update(cx, |w, _| w.prompt_up = true);
        let prompt = ClosePrompt::Collection;
        let answer = window.prompt(
            PromptLevel::Warning,
            "Close collection with unsaved changes?",
            Some("Closing this collection closes its tabs."),
            prompt.buttons(),
            cx,
        );
        let ws = self.workspace.clone();
        cx.spawn(async move |_, cx| {
            let button = answer.await.ok();
            ws.update(cx, |w, cx| {
                let Some(ix) = w.coll_ix(&root) else {
                    w.prompt_up = false;
                    return;
                };
                let tabs = w.collections[ix].group.tabs.clone();
                if save_close::finish(prompt, button, &tabs, w, cx) {
                    w.close_collection(ix, cx);
                }
            });
        })
        .detach();
    }

    fn on_toggle_theme(&mut self, _: &ToggleTheme, window: &mut Window, cx: &mut Context<Self>) {
        let mode = self.workspace.update(cx, |w, cx| {
            let next = next_theme(w.settings.theme);
            w.update_settings(|s| s.theme = next, cx);
            next
        });
        theme::apply(mode, Some(window), cx);
        cx.notify();
    }

    fn on_toggle_layout(&mut self, _: &ToggleLayout, _: &mut Window, cx: &mut Context<Self>) {
        self.workspace.update(cx, |w, cx| {
            w.update_settings(
                |s| {
                    s.split = match s.split {
                        SplitAxis::Vertical => SplitAxis::Horizontal,
                        SplitAxis::Horizontal => SplitAxis::Vertical,
                    }
                },
                cx,
            );
        });
        cx.notify();
    }

    /// Create the per-tab views the first time a tab is rendered and drop closed ones.
    fn tab_views(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tabs: Vec<_> = self.workspace.read(cx).all_tabs().cloned().collect();
        self.views
            .retain(|id, _| tabs.iter().any(|t| t.entity_id() == *id));
        for tab in tabs {
            if self.views.contains_key(&tab.entity_id()) {
                continue;
            }
            let w = self.workspace.clone();
            let views = TabViews {
                url_bar: cx.new(|cx| UrlBar::new(w.clone(), tab.clone(), window, cx)),
                request: cx.new(|cx| RequestPane::new(w.clone(), tab.clone(), window, cx)),
                response: cx.new(|cx| ResponsePane::new(w.clone(), tab.clone(), window, cx)),
                _sub: cx.subscribe(&tab, |_, _, _, cx| cx.notify()),
            };
            self.views.insert(tab.entity_id(), views);
        }
    }

    fn banner(&self, tab: &Entity<RequestTab>, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let (text, keep, primary): (&str, &str, (&str, bool)) = match tab.read(cx).disk {
            DiskState::InSync => return None,
            DiskState::ChangedOnDisk => ("Changed on disk", "Keep mine", ("Reload", true)),
            DiskState::DeletedOnDisk => ("Deleted on disk", "Keep", ("Close", false)),
        };
        let reload = primary.1;
        Some(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .bg(theme::banner_bg(cx.theme()))
                .text_color(cx.theme().foreground)
                .border_b_1()
                .border_color(cx.theme().border)
                .child(div().flex_1().child(text))
                .child(
                    Button::new("banner-primary")
                        .label(primary.0)
                        .on_click(cx.listener(move |_, _, window, cx| {
                            if reload {
                                window.dispatch_action(Box::new(ReloadFromDisk), cx);
                            } else {
                                window.dispatch_action(Box::new(CloseTab), cx);
                            }
                        })),
                )
                .child(Button::new("banner-keep").label(keep).on_click(cx.listener(
                    |_, _, window, cx| {
                        window.dispatch_action(Box::new(KeepMine), cx);
                    },
                ))),
        )
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.tab_views(window, cx);
        // The callback runs after this frame is drawn, so `contains_focused`
        // reflects the new tree; focus that moved meanwhile is left alone.
        let focus = self.focus.clone();
        let focused_at_render = window.focused(cx);
        window.on_next_frame(move |window, cx| {
            if should_reclaim(
                window.focused(cx) == focused_at_render,
                window.has_active_prompt(),
                focus.contains_focused(window, cx),
            ) {
                window.focus(&focus, cx);
            }
        });
        let (tabs, active, split) = {
            let w = self.workspace.read(cx);
            (w.tabs().to_vec(), w.active_tab_ix(), w.settings.split)
        };

        let titles: Vec<Tab> = tabs
            .iter()
            .enumerate()
            .map(|(ix, t)| {
                let t = t.read(cx);
                let dirty = if t.is_dirty() { "● " } else { "" };
                let ws = self.workspace.clone();
                let tab_entity = tabs[ix].clone();
                // Stop mouse-down so pressing × does not activate the tab first.
                let close = div()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        Button::new(("tab-close", ix))
                            .tab_stop(false)
                            .icon(IconName::Close)
                            .ghost()
                            .xsmall()
                            .tooltip("Close tab")
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                request_close_tab(&ws, &tab_entity, window, cx);
                            }),
                    );
                Tab::new()
                    .label(format!("{dirty}{}", t.title()))
                    .suffix(close)
            })
            .collect();
        let ws = self.workspace.clone();
        let tab_bar = TabBar::new("tabs")
            .selected_index(active.unwrap_or(0))
            .on_click(move |ix, _, cx| {
                let ix = *ix;
                ws.update(cx, |w, cx| w.activate_tab(ix, cx));
            })
            .children(titles);

        let active_tab = active.and_then(|ix| tabs.get(ix).cloned());
        let content = match active_tab
            .as_ref()
            .and_then(|t| self.views.get(&t.entity_id()))
        {
            Some(v) => {
                let request = div()
                    .size_full()
                    .min_w_0()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(v.url_bar.clone())
                    .child(div().flex_1().min_h_0().child(v.request.clone()));
                let response = div()
                    .size_full()
                    .min_w_0()
                    .overflow_hidden()
                    .child(v.response.clone());
                let (group, min_pane) = match split {
                    SplitAxis::Vertical => (v_resizable("split-v"), MIN_PANE_HEIGHT),
                    SplitAxis::Horizontal => (h_resizable("split-h"), MIN_PANE_WIDTH),
                };
                let pane = || resizable_panel().size_range(px(min_pane)..px(f32::MAX));
                div().flex_1().min_h_0().child(
                    group
                        .child(pane().child(request))
                        .child(pane().child(response)),
                )
            }
            None => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child("No request open"),
        };

        let banner = active_tab.as_ref().and_then(|t| self.banner(t, cx));
        let env_banners = self.env_banners(cx);
        let main = div()
            .size_full()
            .min_w_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(tab_bar)
            .children(env_banners)
            .children(banner)
            .child(content);

        let layout = h_resizable("root")
            .child(
                resizable_panel()
                    .size(px(240.))
                    .size_range(px(120.)..px(sidebar_max(f32::from(window.viewport_size().width))))
                    .child(self.sidebar.clone()),
            )
            .child(resizable_panel().child(main));

        let overlay = self.palette.as_ref().map(|(p, _)| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .bg(cx.theme().overlay)
                .flex()
                .justify_center()
                .pt(px(80.))
                .child(p.clone())
        });

        let import_overlay = self.import.as_ref().map(|(d, _)| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .bg(cx.theme().overlay)
                .flex()
                .justify_center()
                .pt(px(60.))
                .child(d.clone())
        });

        let new_env_overlay = self.new_env.as_ref().map(|(d, _)| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .bg(cx.theme().overlay)
                .flex()
                .justify_center()
                .pt(px(60.))
                .child(d.clone())
        });

        let history_overlay = self.history.as_ref().map(|(d, _)| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .bg(cx.theme().overlay)
                .flex()
                .justify_center()
                .pt(px(60.))
                .child(d.clone())
        });

        let cookies_overlay = self.cookies.as_ref().map(|(d, _)| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .bg(cx.theme().overlay)
                .flex()
                .justify_center()
                .pt(px(60.))
                .child(d.clone())
        });

        let secrets_overlay = self.secrets.as_ref().map(|(d, _)| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .bg(cx.theme().overlay)
                .flex()
                .justify_center()
                .pt(px(60.))
                .child(d.clone())
        });

        div()
            .id("wisp-root")
            .track_focus(&self.focus)
            .key_context("Wisp")
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(Self::on_send))
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_open_collection))
            .on_action(cx.listener(Self::on_new_collection))
            .on_action(cx.listener(Self::on_new_environment))
            .on_action(cx.listener(Self::on_close_collection))
            .on_action(cx.listener(Self::on_toggle_theme))
            .on_action(cx.listener(Self::on_toggle_layout))
            .on_action(cx.listener(|this, _: &CancelRequest, _, cx| {
                this.with_active(cx, |w, tab, cx| w.cancel(tab, cx));
            }))
            .on_action(cx.listener(|this, _: &NewRequest, _, cx| {
                this.workspace
                    .update(cx, |w, cx| drop(w.new_scratch_tab(String::new(), cx)));
            }))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                let active = this.workspace.read(cx).active_tab();
                if let Some(tab) = active {
                    request_close_tab(&this.workspace, &tab, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &NextTab, _, cx| {
                this.workspace.update(cx, |w, cx| {
                    if let Some(ix) = w.active_tab_ix() {
                        w.activate_tab((ix + 1) % w.tabs().len(), cx);
                    }
                });
            }))
            .on_action(cx.listener(|this, _: &PrevTab, _, cx| {
                this.workspace.update(cx, |w, cx| {
                    if let Some(ix) = w.active_tab_ix() {
                        w.activate_tab((ix + w.tabs().len() - 1) % w.tabs().len(), cx);
                    }
                });
            }))
            .on_action(cx.listener(|this, _: &OpenPalette, window, cx| {
                this.open_palette(PaletteMode::All, window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenImport, window, cx| {
                if this.import.is_none() {
                    this.open_import(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ShowHistory, window, cx| {
                this.toggle_history(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ShowCookieJar, window, cx| {
                this.toggle_cookies(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ShowEnvSecrets, window, cx| {
                this.toggle_secrets(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ShareKeychainSecrets, _, cx| {
                this.on_share_secrets(cx);
            }))
            .on_action(cx.listener(|this, _: &ClearHistory, window, cx| {
                this.on_clear_history(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleConfirmSend, _, cx| {
                this.toggle_guard(cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleHistoryBodies, _, cx| {
                this.workspace.update(cx, |w, cx| {
                    let on = !w.settings.history_bodies;
                    w.update_settings(|s| s.history_bodies = on, cx);
                    let text = if on {
                        "Response bodies will be stored in history"
                    } else {
                        "Response bodies will not be stored in history"
                    };
                    cx.emit(WorkspaceEvent::Notice(text.into()));
                });
            }))
            .on_action(cx.listener(|this, _: &SwitchEnvironment, window, cx| {
                this.open_palette(PaletteMode::Environments, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ReloadFromDisk, _, cx| {
                this.with_active(cx, |w, tab, cx| w.reload_from_disk(tab, cx));
            }))
            .on_action(cx.listener(|this, _: &KeepMine, _, cx| {
                this.with_active(cx, |w, tab, cx| w.keep_mine(tab, cx));
            }))
            .child(div().flex_1().min_h_0().child(layout))
            .children(self.notice.clone().map(|n| {
                div()
                    .id("notice")
                    .px_2()
                    .py_1()
                    .bg(cx.theme().secondary)
                    .child(n)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.notice = None;
                        cx.notify();
                    }))
            }))
            .children(overlay)
            .children(import_overlay)
            .children(new_env_overlay)
            .children(history_overlay)
            .children(cookies_overlay)
            .children(secrets_overlay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_max_is_240_at_the_minimum_window() {
        assert_eq!(sidebar_max(720.), 240.);
    }

    #[test]
    fn sidebar_max_caps_at_480_on_wide_windows() {
        assert_eq!(sidebar_max(1600.), 480.);
    }

    #[test]
    fn sidebar_max_never_drops_below_120() {
        assert_eq!(sidebar_max(300.), 120.);
    }

    #[test]
    fn reclaims_when_focus_is_on_nothing() {
        assert!(should_reclaim(true, false, false));
    }

    #[test]
    fn keeps_focus_inside_the_tree() {
        assert!(!should_reclaim(true, false, true));
    }

    #[test]
    fn leaves_focus_that_moved_since_render() {
        assert!(!should_reclaim(false, false, false));
    }

    #[test]
    fn leaves_focus_while_prompt_active() {
        assert!(!should_reclaim(true, true, false));
    }

    #[test]
    fn appearance_followed_only_for_system() {
        assert!(follows_system(ThemeMode::System));
        assert!(!follows_system(ThemeMode::Light));
        assert!(!follows_system(ThemeMode::Dark));
    }

    #[test]
    fn theme_cycles_system_light_dark() {
        assert_eq!(next_theme(ThemeMode::System), ThemeMode::Light);
        assert_eq!(next_theme(ThemeMode::Light), ThemeMode::Dark);
        assert_eq!(next_theme(ThemeMode::Dark), ThemeMode::System);
    }
}
