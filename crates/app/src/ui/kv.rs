use gpui::{Context, EventEmitter, IntoElement, ParentElement, Render, SharedString, Window, div};
use wisp_core::request::KeyValue;

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

/// Placeholder key/value table; the owning task fills in the behaviour.
pub struct KvTable {
    rows: Vec<KeyValue>,
    _options: KvOptions,
    _notes: Vec<Option<SharedString>>,
}

impl KvTable {
    pub fn new(
        rows: Vec<KeyValue>,
        options: KvOptions,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            rows,
            _options: options,
            _notes: Vec::new(),
        }
    }

    pub fn set_rows(&mut self, rows: Vec<KeyValue>, _window: &mut Window, _cx: &mut Context<Self>) {
        self.rows = rows;
    }

    pub fn rows(&self) -> Vec<KeyValue> {
        self.rows.clone()
    }

    pub fn set_annotations(&mut self, notes: Vec<Option<SharedString>>, _cx: &mut Context<Self>) {
        self._notes = notes;
    }
}

impl EventEmitter<KvEvent> for KvTable {}

impl Render for KvTable {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child("Key/value table")
    }
}
