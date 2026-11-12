//! The sidebar's collection switcher: the active collection's name on top and, with two or
//! more open, a list to switch between them or close one. It holds no workspace state; it
//! reads the open list and calls the workspace (switch) or asks the root view (close).

use std::path::PathBuf;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    ClickEvent, Context, Entity, EventEmitter, InteractiveElement as _, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement as _, Styled, Subscription, Window, div,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme as _, Sizable as _};

use crate::workspace::{Workspace, WorkspaceEvent};

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionRow {
    pub name: String,
    pub path: PathBuf,
    pub active: bool,
}

/// Rows in open order from `(name, root)` pairs and the active index. Two collections with
/// the same name stay apart by their path, which is the row's identity and tooltip.
pub fn rows(open: &[(String, PathBuf)], active: Option<usize>) -> Vec<CollectionRow> {
    open.iter()
        .enumerate()
        .map(|(i, (name, path))| CollectionRow {
            name: name.clone(),
            path: path.clone(),
            active: active == Some(i),
        })
        .collect()
}

/// Asks the owner to close the collection at this root, through the shared close path.
pub struct CloseRequested(pub PathBuf);

pub struct CollectionSwitcher {
    workspace: Entity<Workspace>,
    list_open: bool,
    _sub: Subscription,
}

impl EventEmitter<CloseRequested> for CollectionSwitcher {}

impl CollectionSwitcher {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let _sub = cx.subscribe(&workspace, |_, _, ev: &WorkspaceEvent, cx| {
            if matches!(ev, WorkspaceEvent::CollectionChanged) {
                cx.notify();
            }
        });
        Self {
            workspace,
            list_open: false,
            _sub,
        }
    }

    fn open_list(&self, cx: &Context<Self>) -> Vec<CollectionRow> {
        let ws = self.workspace.read(cx);
        let mut open = Vec::new();
        while let Some(root) = ws.collection_root(open.len()) {
            let name = root.file_name().map_or_else(
                || root.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            open.push((name, root));
        }
        rows(&open, ws.active_collection_ix())
    }
}

impl Render for CollectionSwitcher {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let all = self.open_list(cx);
        let Some(active) = all.iter().find(|r| r.active).cloned() else {
            return div();
        };
        let many = all.len() > 1;
        let show_list = many && self.list_open;
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;

        let title = if many {
            format!("{} ▾", active.name)
        } else {
            active.name.clone()
        };
        let path_tip = SharedString::from(active.path.display().to_string());
        let head = Button::new("collection-title")
            .small()
            .ghost()
            .label(title)
            .tooltip(path_tip)
            .when(many, |b| {
                b.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.list_open = !this.list_open;
                    cx.notify();
                }))
            });

        div()
            .flex()
            .flex_col()
            .px_2()
            .pt_2()
            .child(head)
            .when(show_list, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_col()
                        .border_1()
                        .border_color(border)
                        .children(all.into_iter().enumerate().map(|(i, row)| {
                            let tip = SharedString::from(row.path.display().to_string());
                            let close_root = row.path.clone();
                            let mark = if row.active { "✓ " } else { "   " };
                            div()
                                .id(("collection-row", i))
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_1()
                                .px_2()
                                .text_sm()
                                .when(!row.active, |d| d.text_color(muted))
                                .child(
                                    div()
                                        .id(("collection-pick", i))
                                        .flex_1()
                                        .cursor_pointer()
                                        .child(format!("{mark}{}", row.name))
                                        .tooltip(move |window, cx| {
                                            Tooltip::new(tip.clone()).build(window, cx)
                                        })
                                        .on_click(cx.listener(
                                            move |this, _: &ClickEvent, _, cx| {
                                                this.list_open = false;
                                                this.workspace.update(cx, |w, cx| {
                                                    w.activate_collection(i, cx)
                                                });
                                                cx.notify();
                                            },
                                        )),
                                )
                                .child(
                                    Button::new(("collection-close", i))
                                        .xsmall()
                                        .ghost()
                                        .label("×")
                                        .tooltip("Close collection")
                                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                            cx.emit(CloseRequested(close_root.clone()));
                                        })),
                                )
                        })),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(items: &[(&str, &str)]) -> Vec<(String, PathBuf)> {
        items
            .iter()
            .map(|(n, p)| (n.to_string(), PathBuf::from(p)))
            .collect()
    }

    #[test]
    fn none_open_gives_no_rows() {
        assert!(rows(&[], None).is_empty());
    }

    #[test]
    fn one_open_is_the_active_row() {
        let r = rows(&open(&[("api", "/w/api")]), Some(0));
        assert_eq!(r.len(), 1);
        assert!(r[0].active);
        assert_eq!(r[0].path, PathBuf::from("/w/api"));
    }

    #[test]
    fn three_open_keep_order_and_mark_only_the_active() {
        let r = rows(&open(&[("a", "/a"), ("b", "/b"), ("c", "/c")]), Some(1));
        let names: Vec<_> = r.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "c"]);
        let flags: Vec<_> = r.iter().map(|x| x.active).collect();
        assert_eq!(flags, [false, true, false]);
    }

    #[test]
    fn duplicate_names_differ_by_path() {
        let r = rows(&open(&[("api", "/one/api"), ("api", "/two/api")]), Some(0));
        assert_eq!(r[0].name, r[1].name);
        assert_ne!(r[0].path, r[1].path);
    }
}
