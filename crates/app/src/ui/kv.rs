use std::rc::Rc;

use gpui::{
    AppContext as _, Context, Entity, EntityId, EventEmitter, InteractiveElement as _, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled, Subscription,
    Window, div, px, size,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme as _, Icon, IconName, Sizable as _, v_virtual_list};
use wisp_core::request::KeyValue;

/// Rows above this count render through a virtualised list.
const VIRTUALISE_ABOVE: usize = 50;
const ROW_HEIGHT: f32 = 30.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KvOptions {
    pub name_placeholder: SharedString,
    pub value_placeholder: SharedString,
    pub toggles: bool,
    pub mask_values: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum KvEvent {
    Changed(Vec<KeyValue>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Name,
    Value,
}

/// What an edit did to the rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edit {
    Unchanged,
    Updated,
    /// The trailing row became a real row; a fresh trailing row is needed.
    Appended,
}

/// GPUI-free row bookkeeping. Display slots are the real rows plus one
/// always-present empty trailing slot, which is never part of `rows()`.
#[derive(Debug, Default)]
struct Model {
    rows: Vec<KeyValue>,
}

impl Model {
    fn new(rows: Vec<KeyValue>) -> Self {
        Self { rows }
    }

    fn rows(&self) -> Vec<KeyValue> {
        self.rows.clone()
    }

    fn len(&self) -> usize {
        self.rows.len()
    }

    /// Real rows plus the trailing one.
    fn slots(&self) -> usize {
        self.rows.len() + 1
    }

    fn is_trailing(&self, slot: usize) -> bool {
        slot == self.rows.len()
    }

    fn replace(&mut self, rows: Vec<KeyValue>) {
        self.rows = rows;
    }

    fn edit(&mut self, slot: usize, field: Field, text: &str) -> Edit {
        if self.is_trailing(slot) {
            if text.is_empty() {
                return Edit::Unchanged;
            }
            let mut row = KeyValue {
                enabled: true,
                ..KeyValue::default()
            };
            match field {
                Field::Name => row.name = text.to_string(),
                Field::Value => row.value = text.to_string(),
            }
            self.rows.push(row);
            return Edit::Appended;
        }
        let Some(row) = self.rows.get_mut(slot) else {
            return Edit::Unchanged;
        };
        let target = match field {
            Field::Name => &mut row.name,
            Field::Value => &mut row.value,
        };
        if target == text {
            return Edit::Unchanged;
        }
        *target = text.to_string();
        Edit::Updated
    }

    fn toggle(&mut self, slot: usize, enabled: bool) -> bool {
        match self.rows.get_mut(slot) {
            Some(row) if row.enabled != enabled => {
                row.enabled = enabled;
                true
            }
            _ => false,
        }
    }

    /// Removes a real row; the trailing slot cannot be deleted.
    fn delete(&mut self, slot: usize) -> bool {
        if slot < self.rows.len() {
            self.rows.remove(slot);
            true
        } else {
            false
        }
    }
}

struct Slot {
    name: Entity<InputState>,
    value: Entity<InputState>,
    _subs: [Subscription; 2],
}

/// Reusable editor for a list of enabled/name/value rows.
pub struct KvTable {
    model: Model,
    options: KvOptions,
    notes: Vec<Option<SharedString>>,
    slots: Vec<Slot>,
}

impl KvTable {
    pub fn new(
        rows: Vec<KeyValue>,
        options: KvOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            model: Model::new(rows),
            options,
            notes: Vec::new(),
            slots: Vec::new(),
        };
        this.sync_slots(window, cx);
        this
    }

    /// Replaces the content, reusing input entities; emits no event.
    pub fn set_rows(&mut self, rows: Vec<KeyValue>, window: &mut Window, cx: &mut Context<Self>) {
        self.model.replace(rows);
        self.sync_slots(window, cx);
        cx.notify();
    }

    pub fn rows(&self) -> Vec<KeyValue> {
        self.model.rows()
    }

    pub fn set_annotations(&mut self, notes: Vec<Option<SharedString>>, cx: &mut Context<Self>) {
        self.notes = notes;
        cx.notify();
    }

    fn make_slot(&self, window: &mut Window, cx: &mut Context<Self>) -> Slot {
        let name_ph = self.options.name_placeholder.clone();
        let value_ph = self.options.value_placeholder.clone();
        let masked = self.options.mask_values;
        let name = cx.new(|cx| InputState::new(window, cx).placeholder(name_ph));
        let value = cx.new(|cx| {
            let state = InputState::new(window, cx).placeholder(value_ph);
            if masked { state.masked(true) } else { state }
        });
        let s1 = cx.subscribe_in(&name, window, Self::on_input);
        let s2 = cx.subscribe_in(&value, window, Self::on_input);
        Slot {
            name,
            value,
            _subs: [s1, s2],
        }
    }

    /// Brings the input slots in line with the model: one per row plus the
    /// trailing one, reusing existing entities and only touching text that differs.
    fn sync_slots(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let want = self.model.slots();
        self.slots.truncate(want);
        while self.slots.len() < want {
            let slot = self.make_slot(window, cx);
            self.slots.push(slot);
        }
        for i in 0..want {
            let (name, value) = match self.model.rows.get(i) {
                Some(r) => (r.name.clone(), r.value.clone()),
                None => (String::new(), String::new()),
            };
            for (state, text) in [(&self.slots[i].name, name), (&self.slots[i].value, value)] {
                if state.read(cx).value().as_ref() != text {
                    state.update(cx, |s, cx| s.set_value(text, window, cx));
                }
            }
        }
    }

    fn locate(&self, id: EntityId) -> Option<(usize, Field)> {
        self.slots.iter().enumerate().find_map(|(i, s)| {
            if s.name.entity_id() == id {
                Some((i, Field::Name))
            } else if s.value.entity_id() == id {
                Some((i, Field::Value))
            } else {
                None
            }
        })
    }

    fn on_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let Some((slot, field)) = self.locate(input.entity_id()) else {
            return;
        };
        let text = input.read(cx).value();
        match self.model.edit(slot, field, text.as_ref()) {
            Edit::Unchanged => {}
            Edit::Updated => self.emit_changed(cx),
            Edit::Appended => {
                let fresh = self.make_slot(window, cx);
                self.slots.push(fresh);
                self.emit_changed(cx);
            }
        }
    }

    fn set_enabled(&mut self, slot: usize, enabled: bool, cx: &mut Context<Self>) {
        if self.model.toggle(slot, enabled) {
            self.emit_changed(cx);
        }
    }

    fn delete_row(&mut self, slot: usize, cx: &mut Context<Self>) {
        if self.model.delete(slot) {
            self.slots.remove(slot);
            if slot < self.notes.len() {
                self.notes.remove(slot);
            }
            self.emit_changed(cx);
        }
    }

    fn emit_changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(KvEvent::Changed(self.model.rows()));
        cx.notify();
    }

    fn render_row(&self, slot: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let view = cx.entity();
        let trailing = self.model.is_trailing(slot);
        let inputs = &self.slots[slot];
        let mut row = div()
            .id(("kv-row", slot))
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .h(px(ROW_HEIGHT))
            .px_1();

        if self.options.toggles {
            row = row.child(if trailing {
                div().w(px(20.))
            } else {
                let enabled = self.model.rows[slot].enabled;
                let view = view.clone();
                div().w(px(20.)).child(
                    Checkbox::new(("kv-enabled", slot))
                        .tab_stop(false)
                        .checked(enabled)
                        .on_click(move |checked, _, cx| {
                            let checked = *checked;
                            view.update(cx, |this, cx| this.set_enabled(slot, checked, cx));
                        }),
                )
            });
        }

        row = row.child(
            div().flex_1().child(
                Input::new(&inputs.name)
                    .small()
                    .appearance(false)
                    .bordered(false),
            ),
        );
        let mut value = Input::new(&inputs.value)
            .small()
            .appearance(false)
            .bordered(false);
        if self.options.mask_values {
            value = value.mask_toggle();
        }
        row = row.child(div().flex_1().child(value));

        let note = self
            .notes
            .get(slot)
            .cloned()
            .flatten()
            .filter(|_| !trailing);
        row = row.child(match note {
            Some(note) => div()
                .id(("kv-note", slot))
                .w(px(20.))
                .text_color(cx.theme().warning)
                .child(Icon::new(IconName::TriangleAlert).small())
                .tooltip(move |window, cx| Tooltip::new(note.clone()).build(window, cx)),
            None => div().id(("kv-note", slot)).w(px(20.)),
        });

        row = row.child(if trailing {
            div().w(px(24.))
        } else {
            div().w(px(24.)).child(
                Button::new(("kv-delete", slot))
                    .tab_stop(false)
                    .icon(IconName::Close)
                    .ghost()
                    .xsmall()
                    .tooltip("Delete row")
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| this.delete_row(slot, cx));
                    }),
            )
        });
        row.into_any_element()
    }
}

impl EventEmitter<KvEvent> for KvTable {}

impl Render for KvTable {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.model.slots();
        if self.model.len() > VIRTUALISE_ABOVE {
            let sizes = Rc::new(vec![size(px(1.), px(ROW_HEIGHT)); count]);
            div().size_full().child(v_virtual_list(
                cx.entity(),
                "kv-rows",
                sizes,
                |this, range, _window, cx| {
                    range
                        .filter(|i| *i < this.slots.len())
                        .map(|i| this.render_row(i, cx))
                        .collect::<Vec<_>>()
                },
            ))
        } else {
            let mut list = div().flex().flex_col().w_full();
            for i in 0..count {
                list = list.child(self.render_row(i, cx));
            }
            list
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kv(name: &str, value: &str, enabled: bool) -> KeyValue {
        KeyValue {
            name: name.into(),
            value: value.into(),
            enabled,
        }
    }

    #[test]
    fn trailing_slot_is_never_a_row() {
        let m = Model::new(vec![kv("a", "1", true)]);
        assert_eq!(m.slots(), 2);
        assert_eq!(m.rows().len(), 1);
        assert!(m.is_trailing(1));
        assert!(!m.is_trailing(0));
    }

    #[test]
    fn empty_model_has_one_trailing_slot() {
        let m = Model::default();
        assert_eq!(m.slots(), 1);
        assert!(m.rows().is_empty());
    }

    #[test]
    fn typing_in_trailing_name_appends_enabled_row() {
        let mut m = Model::default();
        assert_eq!(m.edit(0, Field::Name, "k"), Edit::Appended);
        assert_eq!(m.rows(), vec![kv("k", "", true)]);
        assert_eq!(m.slots(), 2);
    }

    #[test]
    fn typing_in_trailing_value_appends_enabled_row() {
        let mut m = Model::default();
        assert_eq!(m.edit(0, Field::Value, "v"), Edit::Appended);
        assert_eq!(m.rows(), vec![kv("", "v", true)]);
    }

    #[test]
    fn empty_text_in_trailing_does_not_append() {
        let mut m = Model::default();
        assert_eq!(m.edit(0, Field::Name, ""), Edit::Unchanged);
        assert_eq!(m.slots(), 1);
    }

    #[test]
    fn editing_existing_row_updates_once_and_ignores_echo() {
        let mut m = Model::new(vec![kv("a", "1", true)]);
        assert_eq!(m.edit(0, Field::Value, "2"), Edit::Updated);
        assert_eq!(m.edit(0, Field::Value, "2"), Edit::Unchanged);
        assert_eq!(m.rows(), vec![kv("a", "2", true)]);
    }

    #[test]
    fn edit_out_of_range_is_ignored() {
        let mut m = Model::new(vec![kv("a", "1", true)]);
        assert_eq!(m.edit(5, Field::Name, "x"), Edit::Unchanged);
    }

    #[test]
    fn delete_removes_row_and_shifts_indices() {
        let mut m = Model::new(vec![
            kv("a", "", true),
            kv("b", "", true),
            kv("c", "", true),
        ]);
        assert!(m.delete(1));
        assert_eq!(m.rows(), vec![kv("a", "", true), kv("c", "", true)]);
        assert_eq!(m.slots(), 3);
    }

    #[test]
    fn trailing_cannot_be_deleted() {
        let mut m = Model::new(vec![kv("a", "", true)]);
        assert!(!m.delete(1));
        assert!(!m.delete(9));
        assert_eq!(m.slots(), 2);
    }

    #[test]
    fn toggle_reports_change_only_when_different() {
        let mut m = Model::new(vec![kv("a", "", true)]);
        assert!(m.toggle(0, false));
        assert!(!m.toggle(0, false));
        assert!(!m.toggle(1, true));
        assert!(!m.rows()[0].enabled);
    }

    #[test]
    fn replace_swaps_rows() {
        let mut m = Model::new(vec![kv("a", "", true)]);
        m.replace(vec![kv("x", "y", false), kv("z", "", true)]);
        assert_eq!(m.len(), 2);
        assert_eq!(m.slots(), 3);
    }
}
