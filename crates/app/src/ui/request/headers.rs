//! Headers sub-tab: the request's own headers plus a read-only list of inherited ones.

use gpui::{
    AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement, Render,
    Styled, Subscription, Window, div,
};
use gpui_component::ActiveTheme as _;
use wisp_core::request::KeyValue;

use super::params::annotations;
use crate::tab::{RequestTab, TabEvent};
use crate::ui::kv::{KvEvent, KvOptions, KvTable};
use crate::ui::url_bar::folder_names;
use crate::workspace::{Workspace, WorkspaceEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inherited {
    pub name: String,
    pub value: String,
    pub source: String,
    pub overridden: bool,
}

/// Inherited headers, nearest source first. `levels` is nearest first
/// (innermost folder ... outermost folder, collection). Disabled headers do not
/// apply and are left out. A header is overridden when the request or a nearer
/// level sets the same name (case-insensitive).
pub fn inherited_headers(own: &[KeyValue], levels: &[(String, &[KeyValue])]) -> Vec<Inherited> {
    let mut seen: Vec<String> = own
        .iter()
        .filter(|h| h.enabled)
        .map(|h| h.name.to_ascii_lowercase())
        .collect();
    let mut out = Vec::new();
    for (source, headers) in levels {
        let mut here = Vec::new();
        for h in headers.iter().filter(|h| h.enabled) {
            let key = h.name.to_ascii_lowercase();
            out.push(Inherited {
                name: h.name.clone(),
                value: h.value.clone(),
                source: source.clone(),
                overridden: seen.contains(&key),
            });
            here.push(key);
        }
        seen.extend(here);
    }
    out
}

pub struct HeadersEditor {
    workspace: Entity<Workspace>,
    tab: Entity<RequestTab>,
    table: Entity<KvTable>,
    inherited: Vec<Inherited>,
    _subs: Vec<Subscription>,
}

impl HeadersEditor {
    pub fn new(
        workspace: Entity<Workspace>,
        tab: Entity<RequestTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let rows = tab.read(cx).draft.headers.clone();
        let table = cx.new(|cx| {
            KvTable::new(
                rows,
                KvOptions {
                    name_placeholder: "Header".into(),
                    value_placeholder: "value".into(),
                    toggles: true,
                    mask_values: false,
                },
                window,
                cx,
            )
        });
        let subs = vec![
            cx.subscribe(&table, Self::on_table),
            cx.subscribe_in(&tab, window, Self::on_tab),
            cx.subscribe(&workspace, Self::on_workspace),
        ];
        let mut this = Self {
            workspace,
            tab,
            table,
            inherited: Vec::new(),
            _subs: subs,
        };
        this.refresh(cx);
        this
    }

    fn on_table(&mut self, _: Entity<KvTable>, event: &KvEvent, cx: &mut Context<Self>) {
        let KvEvent::Changed(rows) = event;
        let rows = rows.clone();
        if self.tab.read(cx).draft.headers != rows {
            self.tab
                .update(cx, |t, cx| t.edit(|d| d.headers = rows, cx));
        }
    }

    fn on_tab(
        &mut self,
        _: &Entity<RequestTab>,
        event: &TabEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TabEvent::DraftReplaced => {
                let rows = self.tab.read(cx).draft.headers.clone();
                self.table.update(cx, |t, cx| t.set_rows(rows, window, cx));
                self.refresh(cx);
            }
            TabEvent::DraftEdited => self.refresh(cx),
            _ => {}
        }
    }

    fn on_workspace(
        &mut self,
        _: Entity<Workspace>,
        event: &WorkspaceEvent,
        cx: &mut Context<Self>,
    ) {
        if matches!(
            event,
            WorkspaceEvent::EnvironmentChanged | WorkspaceEvent::CollectionChanged
        ) {
            self.refresh(cx);
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let t = self.tab.read(cx);
        let ws = self.workspace.read(cx);
        let scope = ws.scope_for(t.path.as_deref());
        let names = folder_names(t.path.as_deref());
        let mut levels: Vec<(String, &[KeyValue])> = Vec::new();
        for (i, f) in scope.folders.iter().enumerate().rev() {
            let label = if names.len() == scope.folders.len() {
                format!("folder {}", names[i])
            } else {
                "folder".to_string()
            };
            levels.push((label, f.headers.as_slice()));
        }
        levels.push((
            "collection".to_string(),
            scope.collection.headers.as_slice(),
        ));
        self.inherited = inherited_headers(&t.draft.headers, &levels);
        let notes = annotations(&t.draft.headers, &t.draft, &scope);
        self.table.update(cx, |t, cx| t.set_annotations(notes, cx));
        cx.notify();
    }
}

impl Render for HeadersEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div().flex().flex_col().px_2().pt_2().gap_1();
        if !self.inherited.is_empty() {
            list = list.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Inherited"),
            );
        }
        for (i, h) in self.inherited.iter().enumerate() {
            let mut row = div()
                .id(("inherited", i))
                .flex()
                .flex_row()
                .gap_2()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(div().w(gpui::px(160.)).child(h.name.clone()))
                .child(div().flex_1().child(h.value.clone()))
                .child(div().child(h.source.clone()));
            if h.overridden {
                row = row.line_through();
            }
            list = list.child(row);
        }
        div().child(self.table.clone()).child(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(name: &str, value: &str, enabled: bool) -> KeyValue {
        KeyValue {
            name: name.into(),
            value: value.into(),
            enabled,
        }
    }

    #[test]
    fn nearer_wins_and_overridden_are_flagged() {
        let folder = [h("X-A", "f", true), h("x-b", "f", true)];
        let coll = [
            h("x-a", "c", true),
            h("X-C", "c", true),
            h("X-D", "c", false),
        ];
        let levels = vec![
            ("folder f".to_string(), &folder[..]),
            ("collection".to_string(), &coll[..]),
        ];
        let own = [h("X-B", "own", true)];
        let got = inherited_headers(&own, &levels);
        let flags: Vec<(&str, bool)> = got
            .iter()
            .map(|i| (i.value.as_str(), i.overridden))
            .collect();
        assert_eq!(
            flags,
            [("f", false), ("f", true), ("c", true), ("c", false)]
        );
        assert_eq!(got[2].source, "collection");
    }

    #[test]
    fn disabled_own_header_does_not_override() {
        let coll = [h("X-A", "c", true)];
        let levels = vec![("collection".to_string(), &coll[..])];
        let got = inherited_headers(&[h("X-A", "o", false)], &levels);
        assert!(!got[0].overridden);
    }
}
