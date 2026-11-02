//! Params sub-tab: a toggleable key/value table bound to `draft.params`.

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    Styled as _, Subscription, Window,
};
use wisp_core::request::{KeyValue, Request};
use wisp_core::resolve::Scope;

use crate::highlight::unresolved_names;
use crate::tab::{RequestTab, TabEvent};
use crate::ui::kv::{KvEvent, KvOptions, KvTable};
use crate::workspace::{Workspace, WorkspaceEvent};

/// One note per row listing the `{{vars}}` that are not defined.
pub(super) fn annotations(
    rows: &[KeyValue],
    request: &Request,
    scope: &Scope,
) -> Vec<Option<SharedString>> {
    rows.iter()
        .map(|r| {
            let mut names = unresolved_names(&r.name, request, scope);
            for n in unresolved_names(&r.value, request, scope) {
                if !names.contains(&n) {
                    names.push(n);
                }
            }
            if names.is_empty() {
                None
            } else {
                let list: Vec<String> = names.iter().map(|n| format!("{{{{{n}}}}}")).collect();
                Some(format!("Not defined: {}", list.join(", ")).into())
            }
        })
        .collect()
}

pub struct ParamsEditor {
    workspace: Entity<Workspace>,
    tab: Entity<RequestTab>,
    table: Entity<KvTable>,
    _subs: Vec<Subscription>,
}

impl ParamsEditor {
    pub fn new(
        workspace: Entity<Workspace>,
        tab: Entity<RequestTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let rows = tab.read(cx).draft.params.clone();
        let table = cx.new(|cx| {
            KvTable::new(
                rows,
                KvOptions {
                    name_placeholder: "name".into(),
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
            _subs: subs,
        };
        this.annotate(cx);
        this
    }

    fn on_table(&mut self, _: Entity<KvTable>, event: &KvEvent, cx: &mut Context<Self>) {
        let KvEvent::Changed(rows) = event;
        let rows = rows.clone();
        if self.tab.read(cx).draft.params != rows {
            self.tab.update(cx, |t, cx| t.edit(|d| d.params = rows, cx));
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
            // The URL input edits params too, so follow the draft whenever it differs.
            TabEvent::DraftEdited | TabEvent::DraftReplaced => {
                let rows = self.tab.read(cx).draft.params.clone();
                if self.table.read(cx).rows() != rows {
                    self.table.update(cx, |t, cx| t.set_rows(rows, window, cx));
                }
                self.annotate(cx);
            }
            _ => {}
        }
    }

    fn on_workspace(
        &mut self,
        _: Entity<Workspace>,
        event: &WorkspaceEvent,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, WorkspaceEvent::EnvironmentChanged) {
            self.annotate(cx);
        }
    }

    fn annotate(&mut self, cx: &mut Context<Self>) {
        let t = self.tab.read(cx);
        let notes = {
            let scope = self.workspace.read(cx).scope_for(t.path.as_deref());
            annotations(&t.draft.params, &t.draft, &scope)
        };
        self.table.update(cx, |t, cx| t.set_annotations(notes, cx));
    }
}

impl Render for ParamsEditor {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::div().size_full().child(self.table.clone())
    }
}
