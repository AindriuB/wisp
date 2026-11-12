//! The ⌘K palette (requests and commands) and the ⌘E environment switcher.

use std::ops::Range;

use gpui::{
    Action, App, AppContext as _, Context, DismissEvent, Entity, EventEmitter, FontWeight,
    HighlightStyle, InteractiveElement, IntoElement, ParentElement, Render, SharedString, Styled,
    StyledText, WeakEntity, Window, div, px,
};
use gpui_component::command::{Command, CommandItem, CommandState};
use gpui_component::kbd::Kbd;
use gpui_component::{ActiveTheme as _, Disableable as _, IndexPath, h_flex};

use crate::actions::{
    CancelRequest, ClearHistory, CloseCollection, CloseTab, NewRequest, OpenCollection, OpenImport,
    OpenPalette, ReloadFromDisk, SaveRequest, SendRequest, ShareKeychainSecrets, ShowCookieJar, ShowEnvSecrets, ShowHistory,
    SwitchCollection, SwitchEnvironment, ToggleHistoryBodies, ToggleLayout, ToggleTheme,
};
use crate::palette::{self, CommandId, Match, PaletteItem, PaletteKind, PaletteMode};
use crate::workspace::{Workspace, WorkspaceEvent};

const LIMIT: usize = 50;

/// The action each command dispatches. Exhaustive: a new `CommandId` fails to
/// compile until it is mapped.
fn action_for(id: CommandId) -> Box<dyn Action> {
    match id {
        CommandId::Send => Box::new(SendRequest),
        CommandId::Save => Box::new(SaveRequest),
        CommandId::NewRequest => Box::new(NewRequest),
        CommandId::CloseTab => Box::new(CloseTab),
        CommandId::OpenCollection => Box::new(OpenCollection),
        CommandId::SwitchCollection => Box::new(SwitchCollection),
        CommandId::CloseCollection => Box::new(CloseCollection),
        CommandId::SwitchEnvironment => Box::new(SwitchEnvironment),
        CommandId::ToggleTheme => Box::new(ToggleTheme),
        CommandId::ToggleLayout => Box::new(ToggleLayout),
        CommandId::ReloadFromDisk => Box::new(ReloadFromDisk),
        CommandId::Import => Box::new(OpenImport),
        CommandId::ShowHistory => Box::new(ShowHistory),
        CommandId::ShowCookieJar => Box::new(ShowCookieJar),
        CommandId::ClearHistory => Box::new(ClearHistory),
        CommandId::ToggleHistoryBodies => Box::new(ToggleHistoryBodies),
        CommandId::EnvSecrets => Box::new(ShowEnvSecrets),
        CommandId::ShareSecrets => Box::new(ShareKeychainSecrets),
    }
}

/// Byte ranges in `text` for the matched char `positions` (adjacent ones merged).
fn byte_ranges(text: &str, positions: &[usize]) -> Vec<Range<usize>> {
    let bounds: Vec<(usize, char)> = text.char_indices().collect();
    let mut out: Vec<Range<usize>> = Vec::new();
    for &p in positions {
        let Some(&(start, c)) = bounds.get(p) else {
            continue;
        };
        let end = start + c.len_utf8();
        match out.last_mut() {
            Some(last) if last.end == start => last.end = end,
            _ => out.push(start..end),
        }
    }
    out
}

pub struct PaletteView {
    workspace: Entity<Workspace>,
    mode: PaletteMode,
    state: Entity<CommandState>,
    items: Vec<PaletteItem>,
    matches: Vec<Match>,
    has_collection: bool,
    /// Row to highlight once the list has been laid out.
    preselect: Option<usize>,
    /// Focus to restore when the palette closes.
    prev_focus: Option<gpui::FocusHandle>,
}

impl PaletteView {
    pub fn new(
        workspace: Entity<Workspace>,
        mode: PaletteMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let state = cx.new(|cx| CommandState::new(window, cx));
        let mut this = Self {
            workspace,
            mode,
            state,
            items: Vec::new(),
            matches: Vec::new(),
            has_collection: false,
            preselect: None,
            prev_focus: window.focused(cx),
        };
        this.reload(window, cx);
        this
    }

    /// Return focus to where it was, then ask the host to close the palette.
    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(h) = self.prev_focus.take() {
            window.focus(&h, cx);
        }
        cx.emit(DismissEvent);
    }

    /// Rebuild items for the current mode and reset the query.
    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (items, active, has_collection) = {
            let ws = self.workspace.read(cx);
            let open: Vec<(String, std::path::PathBuf)> = ws
                .collections
                .iter()
                .map(|c| (c.name(), c.tree.root.clone()))
                .collect();
            if self.mode == PaletteMode::Collections {
                (
                    palette::collection_items(&open, ws.active_collection_ix()),
                    None,
                    !open.is_empty(),
                )
            } else {
                match ws.active_collection() {
                    Some(c) => {
                        let active = c.active_env.as_ref().map(|e| e.name.clone());
                        (
                            palette::items(
                                Some(&c.tree),
                                &c.environments,
                                active.as_deref(),
                                self.mode,
                            ),
                            active,
                            true,
                        )
                    }
                    None => (palette::items(None, &[], None, self.mode), None, false),
                }
            }
        };
        let mut items = items;
        palette::retain_available(&mut items, self.workspace.read(cx).can_share_secrets());
        self.items = items;
        self.has_collection = has_collection;
        self.matches = palette::filter(&self.items, "", LIMIT);
        self.preselect = (self.mode == PaletteMode::Environments && has_collection)
            .then(|| {
                self.items.iter().position(|it| match &it.kind {
                    PaletteKind::Environment(e) => *e == active,
                    _ => false,
                })
            })
            .flatten();
        let state = self.state.clone();
        state.update(cx, |s, cx| s.set_query("", window, cx));
        state.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    fn on_query(&mut self, query: &str, cx: &mut Context<Self>) {
        self.matches = palette::filter(&self.items, query, LIMIT);
        cx.notify();
    }

    fn confirm(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.matches.get(row).and_then(|m| self.items.get(m.index)) else {
            return;
        };
        match item.kind.clone() {
            PaletteKind::Request(path) => {
                self.workspace.update(cx, |w, cx| w.open_request(&path, cx));
            }
            PaletteKind::Environment(name) => {
                let res = self
                    .workspace
                    .update(cx, |w, cx| w.set_environment(name.as_deref(), cx));
                if let Err(e) = res {
                    self.workspace.update(cx, |_, cx| {
                        cx.emit(WorkspaceEvent::Notice(
                            format!("Cannot switch environment: {e}").into(),
                        ));
                    });
                }
            }
            PaletteKind::Collection(root) => {
                self.workspace.update(cx, |w, cx| {
                    if let Some(ix) = w.coll_ix(&root) {
                        w.activate_collection(ix, cx);
                    }
                });
            }
            PaletteKind::Command(CommandId::SwitchCollection) => {
                self.mode = PaletteMode::Collections;
                self.reload(window, cx);
                return;
            }
            PaletteKind::Command(CommandId::SwitchEnvironment) => {
                // Re-open in place: dispatching the action would have the root
                // close the palette it just opened.
                self.mode = PaletteMode::Environments;
                self.reload(window, cx);
                return;
            }
            PaletteKind::Command(id) => {
                // Dispatched while the palette still holds focus so the action
                // bubbles through the root view.
                window.dispatch_action(action_for(id), cx);
            }
        }
        self.dismiss(window, cx);
    }

    fn item_child(
        item: &PaletteItem,
        positions: &[usize],
    ) -> impl Fn(&mut Window, &mut App) -> gpui::AnyElement + 'static {
        let label = SharedString::from(item.label.clone());
        let detail = SharedString::from(item.detail.clone());
        let ranges = byte_ranges(&item.label, positions);
        let action = match &item.kind {
            PaletteKind::Command(id) => Some(action_for(*id)),
            _ => None,
        };
        move |window, cx| {
            let accent = cx.theme().accent_foreground;
            let muted = cx.theme().muted_foreground;
            let highlights = ranges.iter().map(move |r| {
                (
                    r.clone(),
                    HighlightStyle {
                        font_weight: Some(FontWeight::BOLD),
                        color: Some(accent),
                        ..Default::default()
                    },
                )
            });
            let kbd = action
                .as_ref()
                .and_then(|a| Kbd::global_binding_for_action(a.as_ref(), window));
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(StyledText::new(label.clone()).with_highlights(highlights))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_xs()
                        .text_color(muted)
                        .child(detail.clone()),
                )
                .children(kbd)
                .into_any_element()
        }
    }
}

impl EventEmitter<DismissEvent> for PaletteView {}

impl Render for PaletteView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak: WeakEntity<Self> = cx.entity().downgrade();
        let environments = self.mode == PaletteMode::Environments;
        let collections = self.mode == PaletteMode::Collections;
        let mut command = Command::new(&self.state)
            .filterable(false)
            .placeholder(if environments {
                "Switch environment"
            } else if collections {
                "Switch collection"
            } else {
                "Search requests and commands"
            })
            .on_query({
                let weak = weak.clone();
                move |q, _, cx| {
                    weak.update(cx, |this, cx| this.on_query(q, cx)).ok();
                }
            })
            .on_confirm({
                let weak = weak.clone();
                move |path: IndexPath, window, cx| {
                    weak.update(cx, |this, cx| this.confirm(path.row, window, cx))
                        .ok();
                }
            });
        if environments && !self.has_collection {
            command = command.item(
                CommandItem::new()
                    .label("Open a collection first")
                    .disabled(true),
            );
        } else if collections && !self.has_collection {
            command = command.item(
                CommandItem::new()
                    .label("No collection is open")
                    .disabled(true),
            );
        } else {
            for m in &self.matches {
                let item = &self.items[m.index];
                command = command.item(
                    CommandItem::new()
                        .label(item.label.clone())
                        .child(Self::item_child(item, &m.positions)),
                );
            }
        }
        if let Some(row) = self.preselect.take() {
            let state = self.state.clone();
            window.defer(cx, move |window, cx| {
                state.update(cx, |s, cx| {
                    s.set_selected_index(Some(IndexPath::new(row)), window, cx)
                });
            });
        }
        div()
            .id("palette")
            .w(px(560.))
            .on_action(cx.listener(|this, _: &CancelRequest, window, cx| this.dismiss(window, cx)))
            // Already open: reload in place rather than letting the root stack
            // a second palette over this one.
            .on_action(cx.listener(|this, _: &OpenPalette, window, cx| {
                this.mode = PaletteMode::All;
                this.reload(window, cx);
            }))
            .on_action(cx.listener(|this, _: &SwitchCollection, window, cx| {
                this.mode = PaletteMode::Collections;
                this.reload(window, cx);
            }))
            .on_action(cx.listener(|this, _: &SwitchEnvironment, window, cx| {
                this.mode = PaletteMode::Environments;
                this.reload(window, cx);
            }))
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.dismiss(window, cx)))
            .child(command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_ranges_merge_adjacent_and_respect_utf8() {
        assert_eq!(byte_ranges("Get user", &[0, 1, 4]), vec![0..2, 4..5]);
        assert_eq!(byte_ranges("Ünï", &[1, 2]), vec![2..5]);
        assert!(byte_ranges("abc", &[]).is_empty());
        assert!(byte_ranges("abc", &[9]).is_empty());
    }
}
