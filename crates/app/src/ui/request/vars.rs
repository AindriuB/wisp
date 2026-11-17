//! Vars sub-tab: add, rename, edit and remove `Request.vars`.

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled,
    Subscription, Window, div,
};
use wisp_core::redact::is_credential_name;
use wisp_core::request::{KeyValue, Var};

use crate::tab::{RequestTab, TabEvent};
use crate::ui::kv::{KvEvent, KvOptions, KvTable};

/// `Var`s as table rows; `enabled` is always true on the way in.
pub fn vars_to_rows(vars: &[Var]) -> Vec<KeyValue> {
    vars.iter()
        .map(|v| KeyValue {
            name: v.name.clone(),
            value: v.value.clone(),
            enabled: true,
        })
        .collect()
}

/// Table rows as `Var`s; `enabled` is ignored.
pub fn rows_to_vars(rows: &[KeyValue]) -> Vec<Var> {
    rows.iter()
        .map(|r| Var {
            name: r.name.clone(),
            value: r.value.clone(),
        })
        .collect()
}

/// Indexes of rows whose non-empty name repeats an earlier row's.
pub fn duplicate_var_rows(vars: &[Var]) -> Vec<usize> {
    let mut seen = std::collections::HashSet::new();
    vars.iter()
        .enumerate()
        .filter(|(_, v)| !v.name.is_empty() && !seen.insert(v.name.as_str()))
        .map(|(i, _)| i)
        .collect()
}

fn duplicate_notes(vars: &[Var]) -> Vec<Option<SharedString>> {
    let mut notes = vec![None; vars.len()];
    for i in duplicate_var_rows(vars) {
        notes[i] = Some(SharedString::from("Duplicate name: cannot be saved"));
    }
    notes
}

pub struct VarsEditor {
    tab: Entity<RequestTab>,
    table: Entity<KvTable>,
    _subs: Vec<Subscription>,
}

impl VarsEditor {
    pub fn new(tab: Entity<RequestTab>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let vars = tab.read(cx).draft.vars.clone();
        let rows = vars_to_rows(&vars);
        let table = cx.new(|cx| {
            let mut t = KvTable::new(
                rows,
                KvOptions {
                    name_placeholder: "name".into(),
                    value_placeholder: "value".into(),
                    toggles: false,
                    mask_values: false,
                },
                window,
                cx,
            );
            t.set_row_mask(is_credential_name, window, cx);
            t.set_annotations(duplicate_notes(&vars), cx);
            t
        });
        let subs = vec![
            cx.subscribe(&table, Self::on_table),
            cx.subscribe_in(&tab, window, Self::on_tab),
        ];
        Self {
            tab,
            table,
            _subs: subs,
        }
    }

    fn on_table(&mut self, _: Entity<KvTable>, event: &KvEvent, cx: &mut Context<Self>) {
        let KvEvent::Changed(rows) = event;
        let vars = rows_to_vars(rows);
        if self.tab.read(cx).draft.vars != vars {
            self.tab
                .update(cx, |t, cx| t.edit(|d| d.vars = vars.clone(), cx));
        }
        let notes = duplicate_notes(&vars);
        self.table.update(cx, |t, cx| t.set_annotations(notes, cx));
    }

    fn on_tab(
        &mut self,
        _: &Entity<RequestTab>,
        event: &TabEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let TabEvent::DraftEdited | TabEvent::DraftReplaced = event {
            let vars = self.tab.read(cx).draft.vars.clone();
            if rows_to_vars(&self.table.read(cx).rows()) != vars {
                let rows = vars_to_rows(&vars);
                self.table.update(cx, |t, cx| t.set_rows(rows, window, cx));
            }
            let notes = duplicate_notes(&vars);
            self.table.update(cx, |t, cx| t.set_annotations(notes, cx));
        }
    }
}

impl Render for VarsEditor {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.table.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vars_round_trip_keeps_order_and_duplicates() {
        let vars = vec![
            Var {
                name: "b".into(),
                value: "1".into(),
            },
            Var {
                name: "a".into(),
                value: "2".into(),
            },
            Var {
                name: "b".into(),
                value: "3".into(),
            },
        ];
        let rows = vars_to_rows(&vars);
        assert!(rows.iter().all(|r| r.enabled));
        assert_eq!(rows_to_vars(&rows), vars);
    }

    #[test]
    fn vars_duplicate_rows_flag_the_later_one() {
        let v = |n: &str| Var {
            name: n.into(),
            value: String::new(),
        };
        assert_eq!(
            duplicate_var_rows(&[v("k"), v("a"), v("k"), v("k")]),
            [2, 3]
        );
        assert!(duplicate_var_rows(&[v(""), v("")]).is_empty());
        assert_eq!(duplicate_notes(&[v("k"), v("k")])[0], None);
        assert!(duplicate_notes(&[v("k"), v("k")])[1].is_some());
    }

    #[test]
    fn vars_rows_ignore_enabled_on_the_way_out() {
        let rows = vec![KeyValue {
            name: "a".into(),
            value: "x".into(),
            enabled: false,
        }];
        assert_eq!(
            rows_to_vars(&rows),
            vec![Var {
                name: "a".into(),
                value: "x".into()
            }]
        );
    }
}
