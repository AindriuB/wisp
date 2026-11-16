//! Results of assertions evaluated for the response.
use crate::tab::{CheckOutcome, RequestTab, TabEvent};
use crate::workspace::Workspace;
use gpui::{
    Context, Entity, InteractiveElement as _, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, div,
};
use gpui_component::ActiveTheme as _;
use gpui_component::scroll::ScrollableElement as _;
use wisp_core::assert::{Assertion, Expected, Verdict};

fn detail_text(outcome: &CheckOutcome) -> Option<String> {
    match &outcome.result.verdict {
        Verdict::Pass => None,
        Verdict::Skipped { reason } => Some(reason.clone()),
        Verdict::Fail { actual } => {
            let expected = match &outcome.assertion {
                Assertion::Status { expected, .. }
                | Assertion::Header { expected, .. }
                | Assertion::Json { expected, .. } => match expected {
                    Expected::Str(v) => v.clone(),
                    Expected::Int(v) => v.to_string(),
                    Expected::Float(v) => v.to_string(),
                    Expected::Bool(v) => v.to_string(),
                },
                Assertion::Unrecognised { .. } => "unknown".into(),
            };
            let cap = |s: &str| s.chars().take(80).collect::<String>();
            Some(format!(
                "Expected: {} · Actual: {}",
                cap(&expected),
                cap(actual.as_deref().unwrap_or("(missing)"))
            ))
        }
    }
}
pub struct ChecksPane {
    tab: Entity<RequestTab>,
    _sub: Subscription,
}
impl ChecksPane {
    pub fn new(
        _workspace: Entity<Workspace>,
        tab: Entity<RequestTab>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let sub = cx.subscribe(&tab, |_, _, event: &TabEvent, cx| {
            if *event == TabEvent::ResponseChanged {
                cx.notify();
            }
        });
        Self { tab, _sub: sub }
    }
}
impl Render for ChecksPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let outcomes = &self.tab.read(cx).response.assertions;
        let mut root = div()
            .id("checks-pane")
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .overflow_y_scrollbar();
        if outcomes.is_empty() {
            return root.child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("No checks ran"),
            );
        }
        for outcome in outcomes {
            let (status, color) = match &outcome.result.verdict {
                Verdict::Pass => ("Pass", cx.theme().success),
                Verdict::Fail { .. } => ("Fail", cx.theme().danger),
                Verdict::Skipped { .. } => ("Skipped", cx.theme().muted_foreground),
            };
            let mut row = div().flex().flex_col().gap_1().child(
                div()
                    .flex()
                    .gap_2()
                    .child(div().text_color(color).child(status))
                    .child(outcome.result.label.clone()),
            );
            if let Some(detail) = detail_text(outcome) {
                row = row.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(detail),
                );
            }
            root = root.child(row);
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_core::assert::{AssertionResult, Op};
    fn outcome(verdict: Verdict) -> CheckOutcome {
        CheckOutcome {
            assertion: Assertion::Header {
                name: "x".into(),
                op: Op::Equals,
                expected: Expected::Str("é".repeat(81)),
            },
            result: AssertionResult {
                label: "header x equals".into(),
                verdict,
            },
        }
    }
    #[test]
    fn details_cap_each_value_and_show_skips() {
        let detail = detail_text(&outcome(Verdict::Fail {
            actual: Some("a".repeat(81)),
        }))
        .unwrap();
        assert_eq!(
            detail,
            format!("Expected: {} · Actual: {}", "é".repeat(80), "a".repeat(80))
        );
        assert_eq!(
            detail_text(&outcome(Verdict::Skipped {
                reason: "no body".into()
            }))
            .as_deref(),
            Some("no body")
        );
        assert_eq!(detail_text(&outcome(Verdict::Pass)), None);
    }
}
