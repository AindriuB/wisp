//! GraphQL body editor: a query editor and a variables JSON editor.

use gpui::{App, Entity, IntoElement, ParentElement, Styled, Window, div, px};
use gpui_component::ActiveTheme as _;
use gpui_component::Sizable as _;
use gpui_component::button::Button;
use gpui_component::input::{Editor, EditorState};

use crate::pretty::pretty_json;

pub const NOT_OBJECT: &str = "Variables are not a JSON object";

/// Pretty-printed variables, or `None` when the text is not valid JSON.
pub fn format_variables(text: &str) -> Option<String> {
    pretty_json(text.as_bytes()).ok()
}

/// The inline note for the variables text: blank and objects need none.
/// It never blocks Send; resolve reports the problem at send time.
pub fn variables_note(text: &str) -> Option<&'static str> {
    if text.trim().is_empty() {
        return None;
    }
    let object = pretty_json(text.as_bytes()).is_ok() && text.trim_start().starts_with('{');
    (!object).then_some(NOT_OBJECT)
}

fn label(text: &'static str, cx: &App) -> impl IntoElement {
    div().text_color(cx.theme().muted_foreground).child(text)
}

/// The two labelled editors with the Format action on Variables.
pub fn view(
    query: &Entity<EditorState>,
    variables: &Entity<EditorState>,
    note: Option<&'static str>,
    on_format: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(label("Query", cx))
        .child(Editor::new(query).h(px(200.)))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(label("Variables", cx))
                .child(
                    Button::new("graphql-format")
                        .label("Format")
                        .small()
                        .on_click(move |_, window, cx| on_format(window, cx)),
                )
                .child(
                    div()
                        .text_color(cx.theme().warning)
                        .child(note.unwrap_or("")),
                ),
        )
        .child(Editor::new(variables).h(px(140.)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphql_editor_format_valid_variables() {
        assert_eq!(
            format_variables(r#"{"a":1,"b":[1,2]}"#)
                .as_deref()
                .map(|s| s.contains('\n')),
            Some(true)
        );
    }

    #[test]
    fn graphql_editor_format_invalid_variables_is_none() {
        assert_eq!(format_variables("{\"a\": }"), None);
    }

    #[test]
    fn graphql_editor_note_blank_is_none() {
        assert_eq!(variables_note(""), None);
        assert_eq!(variables_note("  \n "), None);
    }

    #[test]
    fn graphql_editor_note_array_and_invalid() {
        assert_eq!(variables_note("[1,2]"), Some(NOT_OBJECT));
        assert_eq!(variables_note("{\"a\": }"), Some(NOT_OBJECT));
    }

    #[test]
    fn graphql_editor_note_object_is_none() {
        assert_eq!(variables_note(r#"{"a":1}"#), None);
    }
}
