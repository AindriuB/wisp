//! Root layout: sidebar | (tab bar, disk banner, request pane / response pane).

use std::collections::HashMap;

use gpui::{
    AppContext as _, Context, DismissEvent, Entity, EntityId, FocusHandle, InteractiveElement as _,
    IntoElement, ParentElement, PathPromptOptions, PromptLevel, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div, px,
};
use gpui_component::ActiveTheme as _;
use gpui_component::button::Button;
use gpui_component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_component::tab::{Tab, TabBar};

use crate::actions::{
    CancelRequest, CloseTab, KeepMine, NewRequest, NextTab, OpenCollection, OpenPalette, PrevTab,
    ReloadFromDisk, SaveRequest, SendRequest, SwitchEnvironment, ToggleLayout, ToggleTheme,
};
use crate::palette::PaletteMode;
use crate::settings::{SplitAxis, ThemeMode};
use crate::tab::{DiskState, RequestTab};
use crate::theme;
use crate::ui::palette::PaletteView;
use crate::ui::request::RequestPane;
use crate::ui::response::ResponsePane;
use crate::ui::sidebar::Sidebar;
use crate::ui::url_bar::UrlBar;
use crate::workspace::{Workspace, WorkspaceEvent};

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
    notice: Option<SharedString>,
    focus: FocusHandle,
    _workspace_sub: Subscription,
    _appearance: Subscription,
}

/// The OS appearance is followed only while the setting is `System`.
fn follows_system(mode: ThemeMode) -> bool {
    mode == ThemeMode::System
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
        let _workspace_sub = cx.subscribe(&workspace, |this, _, event, cx| {
            if let WorkspaceEvent::Notice(text) = event {
                this.notice = Some(text.clone());
            }
            cx.notify();
        });
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
            notice: None,
            focus,
            _workspace_sub,
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

    fn on_send(&mut self, _: &SendRequest, _: &mut Window, cx: &mut Context<Self>) {
        self.with_active(cx, |w, tab, cx| {
            if let Err(e) = w.send(tab, cx) {
                cx.emit(WorkspaceEvent::Notice(format!("Cannot send: {e:?}").into()));
            }
        });
    }

    fn on_save(&mut self, _: &SaveRequest, _: &mut Window, cx: &mut Context<Self>) {
        self.with_active(cx, |w, tab, cx| {
            if let Err(e) = w.save(tab, cx) {
                cx.emit(WorkspaceEvent::Notice(format!("Save failed: {e:#}").into()));
            }
        });
    }

    fn on_open_collection(
        &mut self,
        _: &OpenCollection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Opening replaces the collection and closes every tab: ask first when
        // a draft would be lost. Cancel changes nothing.
        let confirm = self
            .workspace
            .update(cx, |w, cx| w.open_needs_confirm(cx))
            .then(|| {
                window.prompt(
                    PromptLevel::Warning,
                    "Discard unsaved changes?",
                    Some("Opening another collection closes all tabs."),
                    &["Discard", "Cancel"],
                    cx,
                )
            });
        let ws = self.workspace.clone();
        cx.spawn(async move |_, cx| {
            if let Some(answer) = confirm
                && !matches!(answer.await, Ok(0))
            {
                return;
            }
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
        let tabs = self.workspace.read(cx).tabs.clone();
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
                .bg(cx.theme().warning)
                .text_color(cx.theme().warning_foreground)
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
        let (tabs, active, split) = {
            let w = self.workspace.read(cx);
            (w.tabs.clone(), w.active, w.settings.split)
        };

        let titles: Vec<Tab> = tabs
            .iter()
            .map(|t| {
                let t = t.read(cx);
                let dirty = if t.is_dirty() { "● " } else { "" };
                Tab::new().label(format!("{dirty}{}", t.title()))
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
                    .flex()
                    .flex_col()
                    .child(v.url_bar.clone())
                    .child(div().flex_1().min_h_0().child(v.request.clone()));
                let response = div().size_full().child(v.response.clone());
                let group = match split {
                    SplitAxis::Vertical => v_resizable("split-v"),
                    SplitAxis::Horizontal => h_resizable("split-h"),
                };
                div().flex_1().min_h_0().child(
                    group
                        .child(resizable_panel().child(request))
                        .child(resizable_panel().child(response)),
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
        let main = div()
            .size_full()
            .flex()
            .flex_col()
            .child(tab_bar)
            .children(banner)
            .child(content);

        let layout = h_resizable("root")
            .child(
                resizable_panel()
                    .size(px(240.))
                    .size_range(px(120.)..px(480.))
                    .child(self.sidebar.clone()),
            )
            .child(resizable_panel().child(main));

        let overlay = self.palette.as_ref().map(|(p, _)| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .justify_center()
                .pt(px(80.))
                .child(p.clone())
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
            .on_action(cx.listener(Self::on_toggle_theme))
            .on_action(cx.listener(Self::on_toggle_layout))
            .on_action(cx.listener(|this, _: &CancelRequest, _, cx| {
                this.with_active(cx, |w, tab, cx| w.cancel(tab, cx));
            }))
            .on_action(cx.listener(|this, _: &NewRequest, _, cx| {
                this.workspace
                    .update(cx, |w, cx| drop(w.new_scratch_tab(String::new(), cx)));
            }))
            .on_action(cx.listener(|this, _: &CloseTab, _, cx| {
                this.workspace.update(cx, |w, cx| {
                    if let Some(ix) = w.active {
                        w.close_tab(ix, cx);
                    }
                });
            }))
            .on_action(cx.listener(|this, _: &NextTab, _, cx| {
                this.workspace.update(cx, |w, cx| {
                    if let Some(ix) = w.active {
                        w.activate_tab((ix + 1) % w.tabs.len(), cx);
                    }
                });
            }))
            .on_action(cx.listener(|this, _: &PrevTab, _, cx| {
                this.workspace.update(cx, |w, cx| {
                    if let Some(ix) = w.active {
                        w.activate_tab((ix + w.tabs.len() - 1) % w.tabs.len(), cx);
                    }
                });
            }))
            .on_action(cx.listener(|this, _: &OpenPalette, window, cx| {
                this.open_palette(PaletteMode::All, window, cx);
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
