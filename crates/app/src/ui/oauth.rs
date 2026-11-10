//! The authorize status strip: Waiting / Authorized / Failed, the authorization URL and Cancel.

use gpui::{
    AnyElement, App, ClickEvent, ClipboardItem, Entity, IntoElement, ParentElement, SharedString,
    Styled, Window, div,
};
use gpui_component::ActiveTheme as _;
use gpui_component::Sizable as _;
use gpui_component::button::Button;
use gpui_component::input::{Input, InputState};

use crate::tab::AuthorizeState;

/// The one-line status for a state; `None` when idle.
pub fn status_text(state: &AuthorizeState) -> Option<String> {
    match state {
        AuthorizeState::Idle => None,
        AuthorizeState::Waiting { url: None, .. } => Some("Waiting".to_string()),
        AuthorizeState::Waiting { url: Some(_), .. } => {
            Some("Waiting for the browser to finish authorizing".to_string())
        }
        AuthorizeState::Authorized => Some("Authorized".to_string()),
        AuthorizeState::Cancelled => Some("Cancelled".to_string()),
        AuthorizeState::Failed(message) => Some(format!("Failed: {message}")),
    }
}

/// The strip for `state`, or `None` when idle. `url_input` holds the selectable URL text.
pub fn strip(
    state: &AuthorizeState,
    url_input: &Entity<InputState>,
    on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Option<AnyElement> {
    let text = status_text(state)?;
    let theme = cx.theme();
    let colour = match state {
        AuthorizeState::Authorized => theme.success,
        AuthorizeState::Failed(_) => theme.danger,
        _ => theme.muted_foreground,
    };
    let mut col = div().flex().flex_col().gap_1().child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .text_color(colour)
                    .child(SharedString::from(text)),
            )
            .children(state.waiting_id().map(|_| {
                Button::new("oauth-cancel")
                    .label("Cancel")
                    .small()
                    .on_click(on_cancel)
            })),
    );
    if let AuthorizeState::Waiting { url: Some(url), .. } = state {
        let url = url.clone();
        col = col.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(div().flex_1().child(Input::new(url_input)))
                .child(
                    Button::new("oauth-copy-url")
                        .label("Copy URL")
                        .small()
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(url.clone()));
                        }),
                ),
        );
    }
    Some(col.into_any_element())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_core::RequestId;

    #[test]
    fn oauth_status_text_covers_every_state() {
        assert_eq!(status_text(&AuthorizeState::Idle), None);
        let waiting = AuthorizeState::Waiting {
            id: RequestId(1),
            url: None,
        };
        assert_eq!(status_text(&waiting).as_deref(), Some("Waiting"));
        assert_eq!(
            status_text(&AuthorizeState::Authorized).as_deref(),
            Some("Authorized")
        );
        assert_eq!(
            status_text(&AuthorizeState::Cancelled).as_deref(),
            Some("Cancelled")
        );
        assert_eq!(
            status_text(&AuthorizeState::Failed("state mismatch".into())).as_deref(),
            Some("Failed: state mismatch")
        );
    }
}
