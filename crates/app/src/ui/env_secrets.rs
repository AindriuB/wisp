//! The Environment secrets panel: the active environment's secret names and mode, and the
//! actions that move it between file and keychain or edit one keychain value.
//!
//! The panel shows names and never values. A typed value lives in a masked input until Set is
//! pressed, then goes straight to the workspace.

use gpui::{
    AppContext as _, Context, DismissEvent, Entity, EventEmitter, FocusHandle,
    InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div, px,
};
use gpui_component::button::Button;
use gpui_component::input::{Input, InputState};
use gpui_component::{ActiveTheme as _, Disableable as _, v_flex};
use wisp_storage::vault::SecretValue;

use crate::actions::{CancelRequest, ShareKeychainSecrets};
use crate::vault::{self, EnvAction};
use crate::workspace::{Workspace, WorkspaceEvent};

pub struct EnvSecretsView {
    workspace: Entity<Workspace>,
    name: Entity<InputState>,
    value: Entity<InputState>,
    focus: FocusHandle,
    prev_focus: Option<FocusHandle>,
    _sub: Subscription,
}

impl EventEmitter<DismissEvent> for EnvSecretsView {}

impl EnvSecretsView {
    pub fn new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let prev_focus = window.focused(cx);
        window.focus(&focus, cx);
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Variable name"));
        let value = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Value")
                .masked(true)
        });
        let _sub = cx.subscribe(&workspace, |_, _, event, cx| {
            if matches!(
                event,
                WorkspaceEvent::SecretsChanged
                    | WorkspaceEvent::EnvironmentChanged
                    | WorkspaceEvent::CollectionChanged
            ) {
                cx.notify();
            }
        });
        workspace.update(cx, |w, cx| w.probe_keychain(cx));
        Self {
            workspace,
            name,
            value,
            focus,
            prev_focus,
            _sub,
        }
    }

    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(h) = self.prev_focus.take() {
            window.focus(&h, cx);
        }
        cx.emit(DismissEvent);
    }

    fn act(&mut self, action: EnvAction, cx: &mut Context<Self>) {
        self.workspace
            .update(cx, |w, cx| w.request_env_action(action, cx));
    }

    fn set_value(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let var = self.name.read(cx).value().trim().to_string();
        let value = SecretValue::new(self.value.read(cx).value().to_string());
        self.value.update(cx, |s, cx| s.set_value("", window, cx));
        self.act(EnvAction::SetValue { var, value }, cx);
    }
}

impl Render for EnvSecretsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let (model, probe) = {
            let w = self.workspace.read(cx);
            let c = w.active_collection();
            (
                vault::panel_model(
                    c.and_then(|c| c.active_env.as_ref()),
                    c.and_then(|c| c.env_status.as_ref()),
                ),
                w.keychain_probe.clone(),
            )
        };
        let keychain_ok = matches!(probe, Some(Ok(())));
        let title = match &model.env {
            Some(e) => format!("Environment secrets: {e}"),
            None => "Environment secrets".to_string(),
        };
        let mut body = v_flex()
            .gap_2()
            .child(div().text_lg().child(title))
            .child(div().text_color(muted).child(format!("Mode: {}", model.mode)));
        match &probe {
            Some(Err(reason)) => {
                body = body.child(
                    div()
                        .text_color(cx.theme().danger)
                        .child(format!("Keychain unavailable: {reason}")),
                );
            }
            None => body = body.child(div().text_color(muted).child("Checking the keychain...")),
            Some(Ok(())) => {}
        }
        if model.env.is_some() {
            if model.names.is_empty() {
                body = body.child(div().text_color(muted).child("No secrets"));
            } else {
                let rows = model.names.iter().enumerate().map(|(ix, n)| {
                    let var = n.clone();
                    let mut row = div()
                        .id(("secret-row", ix))
                        .flex()
                        .flex_row()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .child(div().flex_1().truncate().child(SharedString::from(n.clone())));
                    if model.set_and_delete {
                        row = row.child(
                            Button::new(("secret-delete", ix))
                                .label("Delete")
                                .disabled(!keychain_ok)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.act(EnvAction::DeleteValue { var: var.clone() }, cx)
                                })),
                        );
                    }
                    row
                });
                body = body.child(
                    div()
                        .id("secret-list")
                        .max_h(px(240.))
                        .overflow_y_scroll()
                        .children(rows),
                );
            }
            if model.set_and_delete {
                body = body.child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(div().w(px(200.)).child(Input::new(&self.name)))
                        .child(div().flex_1().child(Input::new(&self.value)))
                        .child(
                            Button::new("secret-set")
                                .label("Set value")
                                .disabled(!keychain_ok)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.set_value(window, cx)
                                })),
                        ),
                );
            }
            let mut actions = div().flex().flex_row().gap_2();
            if model.move_to_keychain {
                actions = actions.child(
                    Button::new("secret-to-keychain")
                        .label("Move to keychain")
                        .disabled(!keychain_ok)
                        .on_click(
                            cx.listener(|this, _, _, cx| this.act(EnvAction::MoveToKeychain, cx)),
                        ),
                );
            }
            if model.move_to_file {
                actions = actions.child(
                    Button::new("secret-to-file")
                        .label("Move to file")
                        .disabled(!keychain_ok)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.act(EnvAction::MoveToFile { allow_drop: false }, cx)
                        })),
                );
            }
            if model.import {
                actions = actions.child(
                    Button::new("secret-import")
                        .label("Import secrets file")
                        .disabled(!keychain_ok)
                        .on_click(cx.listener(|this, _, _, cx| this.act(EnvAction::ImportFile, cx))),
                );
            }
            if model.share {
                actions = actions.child(
                    Button::new("secret-share")
                        .label("Share keychain secrets with another folder…")
                        .on_click(cx.listener(|_, _, window, cx| {
                            window.dispatch_action(Box::new(ShareKeychainSecrets), cx)
                        })),
                );
            }
            body = body.child(actions);
        }
        body = body.child(
            Button::new("secret-close")
                .label("Close")
                .on_click(cx.listener(|this, _, window, cx| this.dismiss(window, cx))),
        );
        div()
            .id("env-secrets-panel")
            .track_focus(&self.focus)
            .w(px(720.))
            .p_4()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .on_action(cx.listener(|this, _: &CancelRequest, window, cx| this.dismiss(window, cx)))
            .child(body)
    }
}
