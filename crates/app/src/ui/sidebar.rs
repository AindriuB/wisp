//! Sidebar: environment switcher on top, the collection tree below.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Hsla,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement, PromptLevel, Render,
    SharedString, StatefulInteractiveElement as _, Styled, Subscription, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::select::{Select, SelectEvent, SelectState};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme as _, IndexPath, Sizable as _};
use wisp_core::Method;
use wisp_storage::collection::Node;
use wisp_storage::ops;

use crate::actions::{OpenCollection, OpenPalette, palette_hint};
use crate::ui::collections::{CloseRequested, CollectionSwitcher};
use crate::workspace::{Workspace, WorkspaceEvent, confirmed_open};

const NO_ENV: &str = "No environment";
const METHOD_WIDTH: f32 = 46.0;

/// One visible tree row.
#[derive(Debug, Clone, PartialEq)]
enum RowKind {
    Folder { name: String, open: bool },
    Request { name: String, method: Method },
    Invalid { file: String, error: String },
}

#[derive(Debug, Clone, PartialEq)]
struct Row {
    path: PathBuf,
    depth: usize,
    kind: RowKind,
}

fn method_label(m: Method) -> &'static str {
    match m {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Patch => "PATCH",
        Method::Delete => "DELETE",
        Method::Head => "HEAD",
        Method::Options => "OPTIONS",
    }
}

/// The collapsed set after `old` became `new`: every entry at or under `old` moves with it.
fn rebase_collapsed(set: &HashSet<PathBuf>, old: &Path, new: &Path) -> HashSet<PathBuf> {
    set.iter()
        .map(|p| match p.strip_prefix(old) {
            Ok(rest) => new.join(rest),
            Err(_) => p.clone(),
        })
        .collect()
}

/// The visible rows in tree order; children of collapsed folders are skipped.
fn flatten(nodes: &[Node], collapsed: &HashSet<PathBuf>) -> Vec<Row> {
    fn walk(nodes: &[Node], collapsed: &HashSet<PathBuf>, depth: usize, out: &mut Vec<Row>) {
        for n in nodes {
            match n {
                Node::Folder {
                    path,
                    name,
                    children,
                } => {
                    let open = !collapsed.contains(path);
                    out.push(Row {
                        path: path.clone(),
                        depth,
                        kind: RowKind::Folder {
                            name: name.clone(),
                            open,
                        },
                    });
                    if open {
                        walk(children, collapsed, depth + 1, out);
                    }
                }
                Node::Request(meta) => out.push(Row {
                    path: meta.path.clone(),
                    depth,
                    kind: RowKind::Request {
                        name: meta.name.clone(),
                        method: meta.method,
                    },
                }),
                Node::Invalid { path, error } => out.push(Row {
                    path: path.clone(),
                    depth,
                    kind: RowKind::Invalid {
                        file: path
                            .file_name()
                            .map(|f| f.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        error: error.clone(),
                    },
                }),
            }
        }
    }
    let mut out = Vec::new();
    walk(nodes, collapsed, 0, &mut out);
    out
}

/// Every folder path, depth first. The root (empty path) is not included.
fn folder_paths(nodes: &[Node]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for n in nodes {
        if let Node::Folder { path, children, .. } = n {
            out.push(path.clone());
            out.extend(folder_paths(children));
        }
    }
    out
}

/// Number of entries below a folder, at any depth.
fn descendant_count(nodes: &[Node], folder: &Path) -> usize {
    fn all(nodes: &[Node]) -> usize {
        nodes
            .iter()
            .map(|n| match n {
                Node::Folder { children, .. } => 1 + all(children),
                _ => 1,
            })
            .sum()
    }
    for n in nodes {
        if let Node::Folder { path, children, .. } = n {
            if path == folder {
                return all(children);
            }
            if folder.starts_with(path) {
                return descendant_count(children, folder);
            }
        }
    }
    0
}

/// Destinations for "Move to…": the root plus every folder that is not the node
/// itself or inside it.
fn move_targets(nodes: &[Node], moving: &Path) -> Vec<PathBuf> {
    std::iter::once(PathBuf::new())
        .chain(folder_paths(nodes))
        .filter(|f| !f.starts_with(moving))
        .collect()
}

/// New selection index after an arrow key; clamps at both ends.
fn step(current: Option<usize>, len: usize, delta: isize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match current {
        None => {
            if delta < 0 {
                len - 1
            } else {
                0
            }
        }
        Some(i) => i.saturating_add_signed(delta).min(len - 1),
    })
}

/// Clear the path-keyed state when the shown collection root changes.
fn reset_for_root(
    shown_root: &mut Option<PathBuf>,
    new_root: Option<PathBuf>,
    pending: &mut Option<Pending>,
    selected: &mut Option<PathBuf>,
    collapsed: &mut HashSet<PathBuf>,
) {
    if *shown_root != new_root {
        *shown_root = new_root;
        *pending = None;
        *selected = None;
        collapsed.clear();
    }
}

fn target_label(dir: &Path) -> String {
    if dir.as_os_str().is_empty() {
        "(collection root)".into()
    } else {
        dir.display().to_string()
    }
}

fn delete_message(name: &str, folder_items: Option<usize>) -> String {
    match folder_items {
        Some(n) => format!("Delete folder \"{name}\" and its {n} item(s)? This cannot be undone."),
        None => format!("Delete \"{name}\"? This cannot be undone."),
    }
}

#[derive(Clone)]
enum TextOp {
    NewRequest(PathBuf),
    NewFolder(PathBuf),
    Rename { path: PathBuf, folder: bool },
}

enum Pending {
    Text {
        op: TextOp,
        input: Entity<InputState>,
        _sub: Subscription,
    },
    Move {
        path: PathBuf,
        targets: Vec<PathBuf>,
    },
}

pub struct Sidebar {
    workspace: Entity<Workspace>,
    switcher: Entity<CollectionSwitcher>,
    env: Entity<SelectState<Vec<SharedString>>>,
    env_items: Vec<SharedString>,
    collapsed: HashSet<PathBuf>,
    selected: Option<PathBuf>,
    pending: Option<Pending>,
    /// Root of the collection the state above belongs to.
    shown_root: Option<PathBuf>,
    focus: FocusHandle,
    scroll: gpui::ScrollHandle,
    _subs: Vec<Subscription>,
}

impl EventEmitter<CloseRequested> for Sidebar {}

impl Sidebar {
    pub fn new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let env = cx.new(|cx| SelectState::new(Vec::<SharedString>::new(), None, window, cx));
        let mut subs = Vec::new();
        subs.push(cx.subscribe_in(
            &env,
            window,
            |this, _, ev: &SelectEvent<Vec<SharedString>>, window, cx| {
                let SelectEvent::Confirm(value) = ev;
                if let Some(value) = value {
                    this.choose_env(value.clone(), window, cx);
                }
            },
        ));
        let switcher = cx.new(|cx| CollectionSwitcher::new(workspace.clone(), cx));
        subs.push(cx.subscribe(&switcher, |_, _, ev: &CloseRequested, cx| {
            cx.emit(CloseRequested(ev.0.clone()));
        }));
        subs.push(cx.subscribe_in(
            &workspace,
            window,
            |this, _, ev: &WorkspaceEvent, window, cx| {
                if matches!(
                    ev,
                    WorkspaceEvent::CollectionChanged | WorkspaceEvent::EnvironmentChanged
                ) {
                    this.reset_on_switch(cx);
                    this.sync_env(window, cx);
                }
                cx.notify();
            },
        ));
        let focus = cx.focus_handle();
        let mut this = Self {
            workspace,
            switcher,
            env,
            env_items: Vec::new(),
            collapsed: HashSet::new(),
            selected: None,
            pending: None,
            shown_root: None,
            focus,
            scroll: gpui::ScrollHandle::new(),
            _subs: subs,
        };
        this.reset_on_switch(cx);
        this.sync_env(window, cx);
        this
    }

    /// Clear the path-keyed state when the active collection is another one.
    fn reset_on_switch(&mut self, cx: &mut Context<Self>) {
        let root = self
            .workspace
            .read(cx)
            .active_collection()
            .map(|c| c.tree.root.clone());
        reset_for_root(
            &mut self.shown_root,
            root,
            &mut self.pending,
            &mut self.selected,
            &mut self.collapsed,
        );
    }

    fn notice(&self, text: String, cx: &mut Context<Self>) {
        self.workspace
            .update(cx, |_, cx| cx.emit(WorkspaceEvent::Notice(text.into())));
    }

    /// Bring the select's items and selection in line with the workspace.
    fn sync_env(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (names, active) = match self.workspace.read(cx).active_collection() {
            Some(c) => (
                c.environments.clone(),
                c.active_env.as_ref().map(|e| e.name.clone()),
            ),
            None => (Vec::new(), None),
        };
        let items: Vec<SharedString> = std::iter::once(SharedString::from(NO_ENV))
            .chain(names.iter().map(|n| SharedString::from(n.clone())))
            .collect();
        let row = active
            .and_then(|a| names.iter().position(|n| *n == a))
            .map_or(0, |i| i + 1);
        let changed = items != self.env_items;
        self.env_items = items.clone();
        self.env.update(cx, |s, cx| {
            if changed {
                s.set_items(items, window, cx);
            }
            s.set_selected_index(Some(IndexPath::default().row(row)), window, cx);
        });
    }

    fn choose_env(&mut self, value: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        let name = (value.as_ref() != NO_ENV).then(|| value.to_string());
        let result = self
            .workspace
            .update(cx, |w, cx| w.set_environment(name.as_deref(), cx));
        if let Err(e) = result {
            self.notice(format!("Cannot switch environment: {e:#}"), cx);
        }
        // Success re-syncs through the event; on failure this restores the old pick.
        self.sync_env(window, cx);
    }

    fn rows(&self, cx: &Context<Self>) -> Vec<Row> {
        self.workspace
            .read(cx)
            .active_collection()
            .map(|c| flatten(&c.tree.nodes, &self.collapsed))
            .unwrap_or_default()
    }

    fn open_row(&mut self, row: &Row, cx: &mut Context<Self>) {
        self.selected = Some(row.path.clone());
        match &row.kind {
            RowKind::Folder { .. } => self.toggle(&row.path),
            RowKind::Request { .. } => {
                let path = row.path.clone();
                self.workspace.update(cx, |w, cx| w.open_request(&path, cx));
            }
            RowKind::Invalid { .. } => {}
        }
        cx.notify();
    }

    fn toggle(&mut self, path: &Path) {
        if !self.collapsed.remove(path) {
            self.collapsed.insert(path.to_path_buf());
        }
    }

    fn on_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        if let Some(p) = &self.pending {
            if ev.keystroke.key == "escape"
                && matches!(p, Pending::Text { .. } | Pending::Move { .. })
            {
                self.pending = None;
                cx.notify();
            }
            return;
        }
        let rows = self.rows(cx);
        let cur = self
            .selected
            .as_ref()
            .and_then(|s| rows.iter().position(|r| &r.path == s));
        let delta = match ev.keystroke.key.as_str() {
            "down" => 1,
            "up" => -1,
            "enter" => {
                if let Some(row) = cur.and_then(|i| rows.get(i)) {
                    self.open_row(row, cx);
                }
                return;
            }
            "left" | "right" => {
                if let Some(Row {
                    path,
                    kind: RowKind::Folder { open, .. },
                    ..
                }) = cur.and_then(|i| rows.get(i))
                    && *open == (ev.keystroke.key == "left")
                {
                    let path = path.clone();
                    self.toggle(&path);
                    cx.notify();
                }
                return;
            }
            _ => return,
        };
        if let Some(i) = step(cur, rows.len(), delta) {
            self.selected = Some(rows[i].path.clone());
            cx.notify();
        }
    }

    fn begin_text(
        &mut self,
        op: TextOp,
        initial: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let placeholder = match op {
            TextOp::NewRequest(_) => "Request name",
            TextOp::NewFolder(_) => "Folder name",
            TextOp::Rename { .. } => "New name",
        };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        input.update(cx, |s, cx| s.set_value(initial, window, cx));
        let sub = cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                this.commit_text(cx);
            }
        });
        input.update(cx, |s, cx| s.focus(window, cx));
        self.pending = Some(Pending::Text {
            op,
            input,
            _sub: sub,
        });
        cx.notify();
    }

    fn begin_move(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let targets = self
            .workspace
            .read(cx)
            .active_collection()
            .map(|c| move_targets(&c.tree.nodes, &path))
            .unwrap_or_default();
        self.pending = Some(Pending::Move { path, targets });
        cx.notify();
    }

    fn root(&self, cx: &Context<Self>) -> Option<PathBuf> {
        self.workspace
            .read(cx)
            .active_collection()
            .map(|c| c.tree.root.clone())
    }

    fn commit_text(&mut self, cx: &mut Context<Self>) {
        let Some(Pending::Text { op, input, .. }) = &self.pending else {
            return;
        };
        let (op, name) = (op.clone(), input.read(cx).value().to_string());
        let Some(root) = self.root(cx) else { return };
        let result: Result<(), wisp_storage::StorageError> = match op {
            TextOp::NewRequest(dir) => ops::create_request(&root, &dir, &name).map(|rel| {
                self.workspace.update(cx, |w, cx| {
                    w.reload_tree(cx);
                    w.open_request(&rel, cx);
                });
                self.selected = Some(rel);
            }),
            TextOp::NewFolder(parent) => ops::create_folder(&root, &parent, &name).map(|rel| {
                self.workspace.update(cx, |w, cx| w.reload_tree(cx));
                self.collapsed.remove(&rel);
                self.selected = Some(rel);
            }),
            TextOp::Rename { path, folder } => {
                let renamed = if folder {
                    ops::rename_folder(&root, &path, &name)
                } else {
                    ops::rename_request(&root, &path, &name)
                };
                renamed.map(|new| self.after_move(&path, new, cx))
            }
        };
        match result {
            Ok(()) => self.pending = None,
            Err(e) => self.notice(e.to_string(), cx),
        }
        cx.notify();
    }

    fn after_move(&mut self, old: &Path, new: PathBuf, cx: &mut Context<Self>) {
        self.workspace.update(cx, |w, cx| {
            w.on_node_moved(old, &new, cx);
            w.reload_tree(cx);
        });
        self.collapsed = rebase_collapsed(&self.collapsed, old, &new);
        self.selected = Some(new);
    }

    fn move_to(&mut self, path: &Path, dest: &Path, cx: &mut Context<Self>) {
        let Some(root) = self.root(cx) else { return };
        match ops::move_node(&root, path, dest) {
            Ok(new) => {
                self.pending = None;
                self.after_move(path, new, cx);
            }
            Err(e) => self.notice(e.to_string(), cx),
        }
        cx.notify();
    }

    fn delete(&mut self, root: &Path, path: &Path, cx: &mut Context<Self>) {
        match ops::delete_node(root, path) {
            Ok(()) => {
                self.workspace.update(cx, |w, cx| {
                    w.on_node_deleted(root, path, cx);
                    w.reload_tree_of(root, cx);
                });
                self.collapsed.retain(|c| !c.starts_with(path));
                if self
                    .selected
                    .as_deref()
                    .is_some_and(|s| s.starts_with(path))
                {
                    self.selected = None;
                }
            }
            Err(e) => self.notice(e.to_string(), cx),
        }
        cx.notify();
    }

    fn method_colour(m: Method, cx: &Context<Self>) -> Hsla {
        let t = cx.theme();
        match m {
            Method::Get => t.success,
            Method::Post => t.warning,
            Method::Put | Method::Patch => t.info,
            Method::Delete => t.danger,
            Method::Head | Method::Options => t.muted_foreground,
        }
    }

    fn render_row(&self, row: &Row, active: Option<&Path>, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.selected.as_deref() == Some(row.path.as_path());
        let is_active = active == Some(row.path.as_path());
        let theme = cx.theme();
        let (hover, active_bg, muted) =
            (theme.list_hover, theme.list_active, theme.muted_foreground);
        let id: SharedString = format!("row-{}", row.path.display()).into();
        let base = div()
            .id(id)
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .h(px(26.0))
            .pl(px(8.0 + 14.0 * row.depth as f32))
            .pr_2()
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .when(is_active || selected, |d| d.bg(active_bg));
        let row_c = row.clone();
        let this = cx.entity().downgrade();
        let base = base.on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            window.focus(&this.focus, cx);
            this.open_row(&row_c, cx);
        }));
        match &row.kind {
            RowKind::Folder { name, open } => {
                let (path, name_c) = (row.path.clone(), name.clone());
                let el = base
                    .child(div().w(px(12.0)).text_color(muted).child(if *open {
                        "▾"
                    } else {
                        "▸"
                    }))
                    .child(div().flex_1().truncate().child(name.clone()));
                el.context_menu(move |menu, _, _| {
                    let item = |label: &'static str,
                                f: fn(
                        &mut Sidebar,
                        &Path,
                        &str,
                        &mut Window,
                        &mut Context<Sidebar>,
                    )| {
                        let (this, path, name) = (this.clone(), path.clone(), name_c.clone());
                        PopupMenuItem::new(label).on_click(move |_, window, cx| {
                            let (path, name) = (path.clone(), name.clone());
                            let _ = this.update(cx, |s, cx| f(s, &path, &name, window, cx));
                        })
                    };
                    menu.item(item("New request", |s, p, _, w, cx| {
                        s.begin_text(TextOp::NewRequest(p.to_path_buf()), String::new(), w, cx)
                    }))
                    .item(item("New folder", |s, p, _, w, cx| {
                        s.begin_text(TextOp::NewFolder(p.to_path_buf()), String::new(), w, cx)
                    }))
                    .item(item("Rename", |s, p, n, w, cx| {
                        s.begin_text(
                            TextOp::Rename {
                                path: p.to_path_buf(),
                                folder: true,
                            },
                            n.to_string(),
                            w,
                            cx,
                        )
                    }))
                    .item(item("Move to…", |s, p, _, _, cx| {
                        s.begin_move(p.to_path_buf(), cx)
                    }))
                    .item(item("Delete", |s, p, n, w, cx| {
                        s.begin_delete_named(p, n, w, cx)
                    }))
                })
                .into_any_element()
            }
            RowKind::Request { name, method } => {
                let (path, name_c) = (row.path.clone(), name.clone());
                let colour = Self::method_colour(*method, cx);
                let el = base
                    .child(
                        div()
                            .w(px(METHOD_WIDTH))
                            .flex_none()
                            .text_xs()
                            .text_color(colour)
                            .child(method_label(*method)),
                    )
                    .child(div().flex_1().truncate().child(name.clone()));
                el.context_menu(move |menu, _, _| {
                    let item = |label: &'static str,
                                f: fn(
                        &mut Sidebar,
                        &Path,
                        &str,
                        &mut Window,
                        &mut Context<Sidebar>,
                    )| {
                        let (this, path, name) = (this.clone(), path.clone(), name_c.clone());
                        PopupMenuItem::new(label).on_click(move |_, window, cx| {
                            let (path, name) = (path.clone(), name.clone());
                            let _ = this.update(cx, |s, cx| f(s, &path, &name, window, cx));
                        })
                    };
                    menu.item(item("Rename", |s, p, n, w, cx| {
                        s.begin_text(
                            TextOp::Rename {
                                path: p.to_path_buf(),
                                folder: false,
                            },
                            n.to_string(),
                            w,
                            cx,
                        )
                    }))
                    .item(item("Move to…", |s, p, _, _, cx| {
                        s.begin_move(p.to_path_buf(), cx)
                    }))
                    .item(item("Delete", |s, p, n, w, cx| {
                        s.confirm_delete(p, n, None, w, cx)
                    }))
                })
                .into_any_element()
            }
            RowKind::Invalid { file, error } => {
                let (path, file_c, error) = (row.path.clone(), file.clone(), error.clone());
                let el = base
                    .child(div().w(px(12.0)).text_color(muted).child("⚠"))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_color(muted)
                            .child(file.clone()),
                    )
                    .tooltip(move |window, cx| Tooltip::new(error.clone()).build(window, cx));
                el.context_menu(move |menu, _, _| {
                    let (this, path, name) = (this.clone(), path.clone(), file_c.clone());
                    menu.item(PopupMenuItem::new("Delete").on_click(move |_, window, cx| {
                        let (path, name) = (path.clone(), name.clone());
                        let _ = this
                            .update(cx, |s, cx| s.confirm_delete(&path, &name, None, window, cx));
                    }))
                })
                .into_any_element()
            }
        }
    }

    fn confirm_delete(
        &mut self,
        path: &Path,
        name: &str,
        items: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let message = delete_message(name, items);
        let answer = window.prompt(
            PromptLevel::Warning,
            "Delete from disk?",
            Some(&message),
            &["Delete", "Cancel"],
            cx,
        );
        let path = path.to_path_buf();
        // The collection the prompt was raised for, not whichever is active after it.
        let root = self.root(cx);
        cx.spawn(async move |this, cx| {
            let confirmed = matches!(answer.await, Ok(0));
            let _ = this.update(cx, |this, cx| {
                let ix = root
                    .as_deref()
                    .and_then(|r| this.workspace.read(cx).coll_ix(r));
                if let (Some(_), Some(root)) = (confirmed_open(confirmed, ix), root) {
                    this.delete(&root, &path, cx);
                }
            });
        })
        .detach();
    }

    /// Delete confirmation for a folder, looking up its name and item count.
    fn begin_delete_named(
        &mut self,
        path: &Path,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self
            .workspace
            .read(cx)
            .active_collection()
            .map_or(0, |c| descendant_count(&c.tree.nodes, path));
        self.confirm_delete(path, name, Some(count), window, cx);
    }

    fn render_pending(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pending = self.pending.as_ref()?;
        let border = cx.theme().border;
        let panel = div()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .border_b_1()
            .border_color(border);
        Some(match pending {
            Pending::Text { op, input, .. } => {
                let title = match op {
                    TextOp::NewRequest(d) => format!("New request in {}", target_label(d)),
                    TextOp::NewFolder(d) => format!("New folder in {}", target_label(d)),
                    TextOp::Rename { .. } => "Rename".to_string(),
                };
                panel
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(title),
                    )
                    .child(Input::new(input))
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(
                                Button::new("sidebar-ok")
                                    .small()
                                    .primary()
                                    .label("OK")
                                    .on_click(cx.listener(|this, _, _, cx| this.commit_text(cx))),
                            )
                            .child(
                                Button::new("sidebar-cancel")
                                    .small()
                                    .label("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.pending = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .into_any_element()
            }
            Pending::Move { path, targets } => {
                let mut panel = panel.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("Move {} to…", path.display())),
                );
                for (i, t) in targets.iter().enumerate() {
                    let (from, dest) = (path.clone(), t.clone());
                    panel = panel.child(
                        Button::new(("sidebar-move", i))
                            .small()
                            .ghost()
                            .label(target_label(t))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.move_to(&from, &dest, cx)),
                            ),
                    );
                }
                panel
                    .child(
                        Button::new("sidebar-move-cancel")
                            .small()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pending = None;
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            }
        })
    }
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ws = self.workspace.read(cx);
        let open = ws.active_collection().is_some();
        let error = ws.active_collection().and_then(|c| c.error.clone());
        let active_path: Option<PathBuf> = ws.active_tab().and_then(|t| t.read(cx).path.clone());
        let border = cx.theme().border;
        let danger = cx.theme().danger;

        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .p_2()
            .border_b_1()
            .border_color(border)
            .child(div().flex_1().child(Select::new(&self.env).small()))
            .child(
                Button::new("sidebar-palette")
                    .small()
                    .ghost()
                    .label(palette_hint())
                    .on_click(|_: &ClickEvent, window, cx| {
                        window.dispatch_action(Box::new(OpenPalette), cx)
                    }),
            );

        let mut body = div()
            .id("sidebar-tree")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .py_1();
        if !open {
            body = body.child(
                div().p_2().child(
                    Button::new("sidebar-open")
                        .label("Open collection…")
                        .on_click(|_: &ClickEvent, window, cx| {
                            window.dispatch_action(Box::new(OpenCollection), cx)
                        }),
                ),
            );
        } else {
            if let Some(e) = error {
                body = body.child(div().p_2().text_xs().text_color(danger).child(e));
            }
            let rows = self.rows(cx);
            for row in &rows {
                body = body.child(self.render_row(row, active_path.as_deref(), cx));
            }
        }
        let this = cx.entity().downgrade();
        // The scrolling tree sits in a bounded relative wrapper so the shared
        // handle can drive a vertical scrollbar layer on top of it.
        let body = div()
            .id("sidebar-tree-wrap")
            .relative()
            .flex_1()
            .min_h_0()
            .child(body)
            .vertical_scrollbar(&self.scroll);
        let body = body.context_menu(move |menu, _, _| {
            if !open {
                return menu;
            }
            let item = |label: &'static str, folder: bool| {
                let this = this.clone();
                PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    let _ = this.update(cx, |s, cx| {
                        let op = if folder {
                            TextOp::NewFolder(PathBuf::new())
                        } else {
                            TextOp::NewRequest(PathBuf::new())
                        };
                        s.begin_text(op, String::new(), window, cx)
                    });
                })
            };
            menu.item(item("New request", false))
                .item(item("New folder", true))
        });

        let pending = self.render_pending(cx);
        div()
            .id("sidebar")
            .key_context("Sidebar")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| this.on_key(ev, cx)))
            .size_full()
            .flex()
            .flex_col()
            .child(self.switcher.clone())
            .child(header)
            .children(pending)
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_storage::collection::RequestMeta;

    fn req(path: &str, name: &str) -> Node {
        Node::Request(RequestMeta {
            path: path.into(),
            name: name.into(),
            method: Method::Get,
        })
    }

    fn tree() -> Vec<Node> {
        vec![
            Node::Folder {
                path: "users".into(),
                name: "Users".into(),
                children: vec![
                    req("users/list.toml", "List"),
                    Node::Folder {
                        path: "users/admin".into(),
                        name: "Admin".into(),
                        children: vec![req("users/admin/x.toml", "X")],
                    },
                ],
            },
            req("ping.toml", "Ping"),
            Node::Invalid {
                path: "bad.toml".into(),
                error: "oops".into(),
            },
        ]
    }

    #[test]
    fn flatten_keeps_order_and_depth() {
        let rows = flatten(&tree(), &HashSet::new());
        let got: Vec<_> = rows
            .iter()
            .map(|r| (r.path.to_str().unwrap(), r.depth))
            .collect();
        assert_eq!(
            got,
            [
                ("users", 0),
                ("users/list.toml", 1),
                ("users/admin", 1),
                ("users/admin/x.toml", 2),
                ("ping.toml", 0),
                ("bad.toml", 0)
            ]
        );
    }

    #[test]
    fn collapsed_folder_hides_children_and_survives_rebuild() {
        let collapsed: HashSet<PathBuf> = ["users".into()].into();
        let rows = flatten(&tree(), &collapsed);
        assert_eq!(rows.len(), 3);
        assert!(matches!(rows[0].kind, RowKind::Folder { open: false, .. }));
        let again = flatten(&tree(), &collapsed);
        assert_eq!(again.len(), 3);
        assert!(matches!(again[0].kind, RowKind::Folder { open: false, .. }));
    }

    #[test]
    fn rebase_collapsed_rewrites_everything_under_the_old_path() {
        let set: HashSet<PathBuf> = ["users".into(), "users/admin".into(), "other".into()].into();
        let got = rebase_collapsed(&set, Path::new("users"), Path::new("people"));
        let want: HashSet<PathBuf> =
            ["people".into(), "people/admin".into(), "other".into()].into();
        assert_eq!(got, want);
    }

    #[test]
    fn collections_root_change_clears_path_keyed_state() {
        let mut shown = Some(PathBuf::from("/a"));
        let mut pending = Some(Pending::Move {
            path: "users".into(),
            targets: vec![PathBuf::new()],
        });
        let mut selected = Some(PathBuf::from("users"));
        let mut collapsed: HashSet<PathBuf> = ["users".into()].into();

        reset_for_root(
            &mut shown,
            Some("/a".into()),
            &mut pending,
            &mut selected,
            &mut collapsed,
        );
        assert!(pending.is_some() && selected.is_some() && !collapsed.is_empty());

        reset_for_root(
            &mut shown,
            Some("/b".into()),
            &mut pending,
            &mut selected,
            &mut collapsed,
        );
        assert_eq!(shown, Some(PathBuf::from("/b")));
        assert!(pending.is_none() && selected.is_none() && collapsed.is_empty());
    }

    #[test]
    fn invalid_row_shows_file_name_and_error() {
        let rows = flatten(&tree(), &HashSet::new());
        assert_eq!(
            rows.last().unwrap().kind,
            RowKind::Invalid {
                file: "bad.toml".into(),
                error: "oops".into()
            }
        );
    }

    #[test]
    fn move_targets_exclude_the_node_and_its_subtree() {
        let t = tree();
        let all = move_targets(&t, Path::new("nothing"));
        assert_eq!(all, [PathBuf::new(), "users".into(), "users/admin".into()]);
        let t2 = move_targets(&t, Path::new("users"));
        assert_eq!(t2, [PathBuf::new()]);
    }

    #[test]
    fn descendant_count_is_recursive() {
        assert_eq!(descendant_count(&tree(), Path::new("users")), 3);
        assert_eq!(descendant_count(&tree(), Path::new("users/admin")), 1);
        assert_eq!(descendant_count(&tree(), Path::new("missing")), 0);
    }

    #[test]
    fn arrow_stepping_clamps() {
        assert_eq!(step(None, 0, 1), None);
        assert_eq!(step(None, 3, 1), Some(0));
        assert_eq!(step(None, 3, -1), Some(2));
        assert_eq!(step(Some(2), 3, 1), Some(2));
        assert_eq!(step(Some(0), 3, -1), Some(0));
        assert_eq!(step(Some(1), 3, 1), Some(2));
    }

    #[test]
    fn delete_message_names_item_and_count() {
        assert!(delete_message("Users", Some(3)).contains("3 item"));
        assert!(delete_message("Ping", None).contains("\"Ping\""));
    }
}
