//! Request assertion editor.

use gpui::{
    AppContext as _, Context, Entity, EntityId, InteractiveElement as _, IntoElement,
    ParentElement, Render, Styled, Subscription, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{ActiveTheme as _, Sizable as _};
use wisp_core::assert::{Assertion, Expected, MAX_ASSERTIONS, Op, check_path, check_regex};

use crate::tab::{RequestTab, TabEvent};
use crate::workspace::Workspace;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Status,
    Header,
    Json,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueType {
    String,
    Int,
    Float,
    Bool,
}
#[derive(Clone)]
struct RowInput {
    kind: Kind,
    target: String,
    op: Op,
    value: String,
    value_type: ValueType,
}

impl RowInput {
    fn from_assertion(a: &Assertion) -> Option<Self> {
        let (kind, target, op, expected) = match a {
            Assertion::Status { op, expected } => (Kind::Status, String::new(), *op, expected),
            Assertion::Header { name, op, expected } => (Kind::Header, name.clone(), *op, expected),
            Assertion::Json { path, op, expected } => (Kind::Json, path.clone(), *op, expected),
            Assertion::Unrecognised { .. } => return None,
        };
        let (value_type, value) = match expected {
            Expected::Str(v) => (ValueType::String, v.clone()),
            Expected::Int(v) => (ValueType::Int, v.to_string()),
            Expected::Float(v) => (ValueType::Float, v.to_string()),
            Expected::Bool(v) => (ValueType::Bool, v.to_string()),
        };
        Some(Self {
            kind,
            target,
            op,
            value,
            value_type,
        })
    }
    fn assertion(&self) -> Option<Assertion> {
        let expected = match self.value_type {
            ValueType::String => Expected::Str(self.value.clone()),
            ValueType::Int => Expected::Int(self.value.parse().ok()?),
            ValueType::Float => Expected::Float(self.value.parse().ok()?),
            ValueType::Bool => Expected::Bool(self.value.parse().ok()?),
        };
        let (kind, target) = match self.kind {
            Kind::Status => ("status", None),
            Kind::Header => ("header", Some(self.target.as_str())),
            Kind::Json => ("json", Some(self.target.as_str())),
        };
        // Use the same schema gate as the storage reader before replacing the draft row.
        Assertion::from_parts(kind, target, self.op.as_str(), Some(expected)).ok()
    }
    fn reset_value_type(&mut self) {
        let ty = if self.op == Op::Exists {
            ValueType::Bool
        } else if self.kind == Kind::Status && self.op == Op::Equals {
            ValueType::Int
        } else {
            ValueType::String
        };
        if self.value_type != ty {
            self.value_type = ty;
            self.value = match ty {
                ValueType::Bool => "true",
                ValueType::Int => "200",
                _ => "",
            }
            .into();
        }
    }
}
fn row_error(row: &RowInput) -> Option<String> {
    if row.kind == Kind::Status
        && row.op == Op::Equals
        && !row
            .value
            .parse::<i64>()
            .is_ok_and(|n| (100..=599).contains(&n))
    {
        return Some(
            if row.value.parse::<i64>().is_err() {
                "Status must be an integer between 100 and 599; this input is not saved"
            } else {
                "Status must be between 100 and 599; this input is not saved"
            }
            .into(),
        );
    }
    if row.kind == Kind::Header && row.target.is_empty() {
        return Some("Header name is required; this input is not saved".into());
    }
    if row.kind == Kind::Json
        && let Err(e) = check_path(&row.target)
    {
        return Some(e);
    }
    if row.op == Op::Matches
        && let Err(e) = check_regex(&row.value)
    {
        return Some(e);
    }
    if row.assertion().is_none() {
        return Some("Invalid expected value; this input is not saved".into());
    }
    None
}
fn can_add(count: usize) -> bool {
    count < MAX_ASSERTIONS
}
fn row_warning(row: &RowInput) -> Option<&'static str> {
    row.value
        .contains("{{")
        .then_some("Expected values are literal; variables are not expanded")
}
fn replace_row(rows: &mut [Assertion], index: usize, row: &RowInput) {
    if let (Some(existing), Some(replacement)) = (rows.get_mut(index), row.assertion())
        && !matches!(existing, Assertion::Unrecognised { .. })
    {
        *existing = replacement;
    }
}
struct Slot {
    row: Option<RowInput>,
    target: Entity<InputState>,
    value: Entity<InputState>,
    _subs: [Subscription; 2],
}
pub struct AssertionsEditor {
    tab: Entity<RequestTab>,
    slots: Vec<Slot>,
    limit_notice: bool,
    _sub: Subscription,
}
impl AssertionsEditor {
    pub fn new(
        _workspace: Entity<Workspace>,
        tab: Entity<RequestTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let sub = cx.subscribe_in(
            &tab,
            window,
            |this: &mut Self, _, event: &TabEvent, window, cx| {
                if *event == TabEvent::DraftReplaced {
                    this.rebuild(window, cx);
                }
            },
        );
        let mut this = Self {
            tab,
            slots: Vec::new(),
            limit_notice: false,
            _sub: sub,
        };
        this.rebuild(window, cx);
        this
    }
    fn rebuild(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.tab.read(cx).draft.assertions.clone();
        self.limit_notice = false;
        self.slots.clear();
        for a in rows {
            let row = RowInput::from_assertion(&a);
            let target_text = row.as_ref().map_or("", |r| r.target.as_str()).to_string();
            let value_text = row.as_ref().map_or("", |r| r.value.as_str()).to_string();
            let target = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Target")
                    .default_value(target_text)
            });
            let value = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Expected")
                    .default_value(value_text)
            });
            let s1 = cx.subscribe_in(&target, window, Self::on_input);
            let s2 = cx.subscribe_in(&value, window, Self::on_input);
            self.slots.push(Slot {
                row,
                target,
                value,
                _subs: [s1, s2],
            });
        }
        cx.notify();
    }
    fn on_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let id: EntityId = input.entity_id();
        let Some((i, target)) = self.slots.iter().enumerate().find_map(|(i, s)| {
            if s.target.entity_id() == id {
                Some((i, true))
            } else if s.value.entity_id() == id {
                Some((i, false))
            } else {
                None
            }
        }) else {
            return;
        };
        let Some(row) = &mut self.slots[i].row else {
            return;
        };
        let text = input.read(cx).value().to_string();
        if target {
            row.target = text;
        } else {
            row.value = text;
        }
        self.commit_row(i, cx);
    }
    fn commit_row(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(row) = self.slots.get(i).and_then(|s| s.row.clone()) {
            self.tab.update(cx, |t, cx| {
                t.edit(|d| replace_row(&mut d.assertions, i, &row), cx)
            });
        }
        cx.notify();
    }
    fn change_row(
        &mut self,
        i: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut RowInput),
    ) {
        let Some(row) = self.slots.get_mut(i).and_then(|s| s.row.as_mut()) else {
            return;
        };
        f(row);
        let value = row.value.clone();
        let target = row.target.clone();
        self.commit_row(i, cx);
        self.slots[i]
            .value
            .update(cx, |s, cx| s.set_value(value, window, cx));
        self.slots[i]
            .target
            .update(cx, |s, cx| s.set_value(target, window, cx));
    }
    fn structural(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Vec<Assertion>, &mut Vec<Option<RowInput>>),
    ) {
        let mut inputs: Vec<_> = self.slots.iter().map(|slot| slot.row.clone()).collect();
        self.tab.update(cx, |t, cx| {
            t.edit(|d| f(&mut d.assertions, &mut inputs), cx)
        });
        self.rebuild(window, cx);
        for (slot, row) in self.slots.iter_mut().zip(inputs) {
            if let Some(row) = row {
                let target = row.target.clone();
                let value = row.value.clone();
                slot.row = Some(row);
                slot.target
                    .update(cx, |s, cx| s.set_value(target, window, cx));
                slot.value
                    .update(cx, |s, cx| s.set_value(value, window, cx));
            }
        }
    }
}
impl Render for AssertionsEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div()
            .id("assertions-editor")
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .overflow_y_scrollbar();
        for (i, slot) in self.slots.iter().enumerate() {
            let mut line = div()
                .flex()
                .flex_col()
                .gap_1()
                .border_b_1()
                .border_color(cx.theme().border)
                .pb_2();
            let mut controls = div().flex().items_center().gap_1();
            if let Some(row) = &slot.row {
                let kind = match row.kind {
                    Kind::Status => "status",
                    Kind::Header => "header",
                    Kind::Json => "json",
                };
                controls = controls.child(
                    Button::new(("assert-kind", i))
                        .label(kind)
                        .small()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.change_row(i, window, cx, |r| {
                                r.kind = match r.kind {
                                    Kind::Status => Kind::Header,
                                    Kind::Header => Kind::Json,
                                    Kind::Json => Kind::Status,
                                };
                                r.op = Op::Equals;
                                r.reset_value_type();
                            })
                        })),
                );
                if row.kind != Kind::Status {
                    controls =
                        controls.child(div().w(px(160.)).child(Input::new(&slot.target).small()));
                }
                controls = controls.child(
                    Button::new(("assert-op", i))
                        .label(row.op.as_str())
                        .small()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.change_row(i, window, cx, |r| {
                                r.op = match (r.kind, r.op) {
                                    (Kind::Status, Op::Equals) => Op::Matches,
                                    (Kind::Status, _) => Op::Equals,
                                    (_, Op::Equals) => Op::Contains,
                                    (_, Op::Contains) => Op::Matches,
                                    (_, Op::Matches) => Op::Exists,
                                    (_, Op::Exists) => Op::Equals,
                                };
                                r.reset_value_type();
                            })
                        })),
                );
                if row.kind == Kind::Json && matches!(row.op, Op::Equals | Op::Contains) {
                    let label = match row.value_type {
                        ValueType::String => "string",
                        ValueType::Int => "integer",
                        ValueType::Float => "float",
                        ValueType::Bool => "boolean",
                    };
                    controls = controls.child(
                        Button::new(("assert-type", i))
                            .label(label)
                            .small()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.change_row(i, window, cx, |r| {
                                    r.value_type = match r.value_type {
                                        ValueType::String => ValueType::Int,
                                        ValueType::Int => ValueType::Float,
                                        ValueType::Float => ValueType::Bool,
                                        ValueType::Bool => ValueType::String,
                                    };
                                    r.value = match r.value_type {
                                        ValueType::String => "",
                                        ValueType::Int => "0",
                                        ValueType::Float => "0.0",
                                        ValueType::Bool => "true",
                                    }
                                    .into();
                                })
                            })),
                    );
                }
                controls = controls.child(
                    div()
                        .flex_1()
                        .min_w(px(100.))
                        .child(Input::new(&slot.value).small()),
                );
                if let Some(error) = row_error(row) {
                    line = line.child(div().text_sm().text_color(cx.theme().danger).child(error));
                }
                if let Some(warning) = row_warning(row) {
                    line = line.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().warning)
                            .child(warning),
                    );
                }
            } else if let Some(Assertion::Unrecognised { reason, .. }) =
                self.tab.read(cx).draft.assertions.get(i)
            {
                controls = controls.child(
                    div()
                        .flex_1()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("Unrecognised assertion: {reason}")),
                );
            }
            let len = self.slots.len();
            for (label, delta) in [("↑", -1isize), ("↓", 1)] {
                if (delta < 0 && i > 0) || (delta > 0 && i + 1 < len) {
                    controls = controls.child(
                        Button::new((
                            if delta < 0 {
                                "assert-up"
                            } else {
                                "assert-down"
                            },
                            i,
                        ))
                        .label(label)
                        .ghost()
                        .xsmall()
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.structural(window, cx, |rows, inputs| {
                                    inputs.swap(i, i.saturating_add_signed(delta));
                                    rows.swap(i, i.saturating_add_signed(delta))
                                })
                            },
                        )),
                    );
                }
            }
            controls = controls.child(
                Button::new(("assert-delete", i))
                    .label("Remove")
                    .ghost()
                    .xsmall()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.structural(window, cx, |rows, inputs| {
                            if i < rows.len() {
                                inputs.remove(i);
                                rows.remove(i);
                            }
                        })
                    })),
            );
            root = root.child(line.child(controls));
        }
        root = root.child(
            Button::new("assert-add")
                .label("Add assertion")
                .small()
                .on_click(cx.listener(|this, _, window, cx| {
                    if !can_add(this.slots.len()) {
                        this.limit_notice = true;
                        cx.notify();
                        return;
                    }
                    this.limit_notice = false;
                    this.structural(window, cx, |rows, inputs| {
                        let assertion = Assertion::Status {
                            op: Op::Equals,
                            expected: Expected::Int(200),
                        };
                        inputs.push(RowInput::from_assertion(&assertion));
                        rows.push(assertion)
                    });
                })),
        );
        if self.limit_notice {
            root = root.child(
                div()
                    .text_color(cx.theme().danger)
                    .child("A request can have at most 50 assertions"),
            );
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(kind: Kind, op: Op, target: &str, value: &str, value_type: ValueType) -> RowInput {
        RowInput {
            kind,
            op,
            target: target.into(),
            value: value.into(),
            value_type,
        }
    }
    #[test]
    fn validation_and_literal_warning() {
        let cases = [
            (
                row(Kind::Status, Op::Equals, "", "99", ValueType::Int),
                true,
            ),
            (
                row(Kind::Status, Op::Equals, "", "600", ValueType::Int),
                true,
            ),
            (
                row(Kind::Status, Op::Equals, "", "200", ValueType::Int),
                false,
            ),
            (
                row(Kind::Status, Op::Matches, "", "[", ValueType::String),
                true,
            ),
            (
                row(Kind::Json, Op::Equals, "not a path", "x", ValueType::String),
                true,
            ),
            (
                row(
                    Kind::Json,
                    Op::Equals,
                    "$.name",
                    "{{name}}",
                    ValueType::String,
                ),
                false,
            ),
        ];
        assert!(can_add(49));
        assert!(!can_add(50));
        for (input, invalid) in cases {
            assert_eq!(row_error(&input).is_some(), invalid);
        }
        assert!(
            row_warning(&row(
                Kind::Header,
                Op::Equals,
                "X",
                "{{x}}",
                ValueType::String
            ))
            .is_some()
        );
    }
    #[test]
    fn editing_rows_preserves_unrecognised_bytes() {
        let unknown = Assertion::Unrecognised {
            raw: "[[assert]]\nkind = 'future'\nvalue = 'bytes'\n".into(),
            reason: "not supported".into(),
        };
        let mut rows = vec![
            unknown.clone(),
            Assertion::Status {
                op: Op::Equals,
                expected: Expected::Int(200),
            },
        ];
        let edit = row(Kind::Status, Op::Equals, "", "201", ValueType::Int);
        replace_row(&mut rows, 0, &edit);
        replace_row(&mut rows, 1, &edit);
        assert_eq!(rows[0], unknown);
        assert_eq!(
            rows[1],
            Assertion::Status {
                op: Op::Equals,
                expected: Expected::Int(201)
            }
        );
    }

    #[test]
    fn invalid_scalar_input_keeps_the_last_saved_value_until_corrected() {
        for (kind, target, value_type, valid, invalid, corrected) in [
            (Kind::Status, "", ValueType::Int, "200", "20x", "201"),
            (Kind::Json, "$.count", ValueType::Int, "1", "", "2"),
            (Kind::Json, "$.ratio", ValueType::Float, "1.5", "-", "2.5"),
            (
                Kind::Header,
                "x-ready",
                ValueType::Bool,
                "true",
                "tru",
                "false",
            ),
        ] {
            let op = if kind == Kind::Header {
                Op::Exists
            } else {
                Op::Equals
            };
            let mut input = row(kind, op, target, valid, value_type);
            let saved = input.assertion().unwrap();
            let mut rows = vec![saved.clone()];
            input.value = invalid.into();
            assert!(row_error(&input).unwrap().contains("not saved"));
            replace_row(&mut rows, 0, &input);
            assert_eq!(rows[0], saved);

            input.value = corrected.into();
            assert_eq!(row_error(&input), None);
            replace_row(&mut rows, 0, &input);
            assert_eq!(rows[0], input.assertion().unwrap());
            assert_ne!(rows[0], saved);
        }
    }
    #[test]
    fn invalid_schema_edits_preserve_editable_assertions_after_save_and_reopen() {
        use wisp_core::request::Request;
        use wisp_storage::request::{load_request, save_request};
        let cases = [
            (
                row(Kind::Status, Op::Equals, "", "200", ValueType::Int),
                row(Kind::Status, Op::Equals, "", "99", ValueType::Int),
                row(Kind::Status, Op::Equals, "", "201", ValueType::Int),
            ),
            (
                row(Kind::Status, Op::Equals, "", "200", ValueType::Int),
                row(Kind::Status, Op::Equals, "", "600", ValueType::Int),
                row(Kind::Status, Op::Equals, "", "599", ValueType::Int),
            ),
            (
                row(
                    Kind::Header,
                    Op::Equals,
                    "X-Old",
                    "value",
                    ValueType::String,
                ),
                row(Kind::Header, Op::Equals, "", "value", ValueType::String),
                row(
                    Kind::Header,
                    Op::Equals,
                    "X-New",
                    "value",
                    ValueType::String,
                ),
            ),
            (
                row(Kind::Header, Op::Exists, "X-Old", "true", ValueType::Bool),
                row(Kind::Header, Op::Exists, "", "true", ValueType::Bool),
                row(Kind::Header, Op::Exists, "X-New", "false", ValueType::Bool),
            ),
        ];
        for (valid, invalid, corrected) in cases {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("request.toml");
            let saved = valid.assertion().unwrap();
            let mut request = Request {
                assertions: vec![saved.clone()],
                ..Request::default()
            };
            assert!(row_error(&invalid).is_some());
            replace_row(&mut request.assertions, 0, &invalid);
            assert_eq!(request.assertions, vec![saved.clone()]);
            save_request(&path, &request).unwrap();
            let mut reopened = load_request(&path).unwrap();
            assert_eq!(reopened.assertions, vec![saved]);
            assert!(RowInput::from_assertion(&reopened.assertions[0]).is_some());
            replace_row(&mut reopened.assertions, 0, &corrected);
            save_request(&path, &reopened).unwrap();
            assert_eq!(
                load_request(&path).unwrap().assertions,
                vec![corrected.assertion().unwrap()]
            );
        }
    }
}
