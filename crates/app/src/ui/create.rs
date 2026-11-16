//! Creating a collection or an environment from the app.
//!
//! The pure helpers at the top (name check, outcome-to-notice mapping) are
//! what the unit tests cover; the view below only wires them to an input.

use std::path::{Path, PathBuf};

use gpui::{
    AppContext as _, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement, Render, SharedString, Styled, Window, div,
    px,
};
use gpui_component::ActiveTheme as _;
use gpui_component::input::{Input, InputEvent, InputState};
use wisp_storage::StorageError;
use wisp_storage::ops;

use crate::actions::CancelRequest;
use crate::workspace::{Workspace, WorkspaceEvent};

/// What creating a collection in a folder came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollectionOutcome {
    /// Created: open it.
    Created,
    /// The folder already holds a collection: open it, with a notice.
    AlreadyPresent,
    /// Nothing was created: show the message, open nothing.
    Failed(String),
}

pub fn collection_outcome(result: &Result<PathBuf, StorageError>) -> CollectionOutcome {
    match result {
        Ok(_) => CollectionOutcome::Created,
        Err(StorageError::Exists { .. }) => CollectionOutcome::AlreadyPresent,
        Err(e) => CollectionOutcome::Failed(format!("Cannot create collection: {e}")),
    }
}

/// The notice for an outcome, if it has one; and whether to open the folder.
pub fn collection_notice(outcome: &CollectionOutcome) -> (Option<String>, bool) {
    match outcome {
        CollectionOutcome::Created => (None, true),
        CollectionOutcome::AlreadyPresent => (Some("Already a collection: opened it".into()), true),
        CollectionOutcome::Failed(m) => (Some(m.clone()), false),
    }
}

/// Check a typed environment name against the names already present. The
/// storage layer stays the authority; this gives the message before any write.
pub fn check_env_name(name: &str, existing: &[String]) -> Result<(), String> {
    if name.is_empty() {
        return Err("Enter a name".into());
    }
    let chars_ok = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !chars_ok {
        return Err("Use only letters, digits, '-', '_' and '.'".into());
    }
    if existing.iter().any(|e| e == name) {
        return Err(format!("An environment named \"{name}\" already exists"));
    }
    Ok(())
}

/// Create `name` under `root`, mapping a refusal to its message.
pub fn create_env(root: &std::path::Path, name: &str, existing: &[String]) -> Result<(), String> {
    check_env_name(name, existing)?;
    ops::create_environment(root, name)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// The collection the environment goes into: the one captured when the dialog opened, and
/// only while it is still open and active (`set_environment` acts on the active collection).
fn resolve_env_target(
    target: Option<&Path>,
    open_ix: Option<usize>,
    active_ix: Option<usize>,
) -> Result<PathBuf, String> {
    let label = |t: &Path| {
        t.file_name().map_or_else(
            || t.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    };
    match (target, open_ix) {
        (None, _) => Err("Open a collection first".to_string()),
        (Some(t), None) => Err(format!(
            "The collection \"{}\" was closed; no environment was created.",
            label(t)
        )),
        (Some(t), Some(i)) if active_ix != Some(i) => Err(format!(
            "The collection \"{}\" is no longer active; no environment was created.",
            label(t)
        )),
        (Some(t), Some(_)) => Ok(t.to_path_buf()),
    }
}

/// A one-line dialog asking for the new environment's name.
pub struct NewEnvironmentView {
    workspace: Entity<Workspace>,
    /// The collection the dialog was opened for.
    target: Option<PathBuf>,
    input: Entity<InputState>,
    error: Option<SharedString>,
    focus: FocusHandle,
    prev_focus: Option<FocusHandle>,
}

impl EventEmitter<DismissEvent> for NewEnvironmentView {}

impl Focusable for NewEnvironmentView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl NewEnvironmentView {
    pub fn new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prev_focus = window.focused(cx);
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Environment name"));
        cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, window, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                this.commit(window, cx);
            }
        })
        .detach();
        input.update(cx, |s, cx| s.focus(window, cx));
        let target = workspace
            .read(cx)
            .active_collection()
            .map(|c| c.tree.root.clone());
        Self {
            workspace,
            target,
            input,
            error: None,
            focus: cx.focus_handle(),
            prev_focus,
        }
    }

    /// Return focus to where it was, then ask the host to close the dialog.
    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(h) = self.prev_focus.take() {
            window.focus(&h, cx);
        }
        cx.emit(DismissEvent);
    }

    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.input.read(cx).value().trim().to_string();
        let (root, existing) = {
            let ws = self.workspace.read(cx);
            let open_ix = self.target.as_deref().and_then(|r| ws.coll_ix(r));
            let root = match resolve_env_target(
                self.target.as_deref(),
                open_ix,
                ws.active_collection_ix(),
            ) {
                Ok(r) => r,
                Err(e) => {
                    self.error = Some(e.into());
                    cx.notify();
                    return;
                }
            };
            let existing = ws
                .active_collection()
                .map(|c| c.environments.clone())
                .unwrap_or_default();
            (root, existing)
        };
        match create_env(&root, &name, &existing) {
            Ok(()) => {
                self.workspace.update(cx, |w, cx| {
                    w.reload_tree_of(&root, cx);
                    if let Err(e) = w.set_environment(Some(&name), cx) {
                        cx.emit(WorkspaceEvent::Notice(
                            format!("Cannot switch environment: {e:#}").into(),
                        ));
                    }
                });
                self.dismiss(window, cx);
            }
            Err(m) => {
                self.error = Some(m.into());
                cx.notify();
            }
        }
    }
}

impl Render for NewEnvironmentView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let mut panel = div()
            .id("new-environment")
            .track_focus(&self.focus)
            .w(px(420.))
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(theme.background)
            .border_1()
            .border_color(theme.border)
            .rounded_md()
            .on_action(cx.listener(|this, _: &CancelRequest, window, cx| this.dismiss(window, cx)))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("New environment"),
            )
            .child(Input::new(&self.input));
        if let Some(e) = &self.error {
            panel = panel.child(div().text_xs().text_color(theme.danger).child(e.clone()));
        }
        panel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_target_is_the_captured_active_collection_or_a_refusal() {
        let t = Path::new("/work/alpha");
        assert_eq!(
            resolve_env_target(Some(t), Some(1), Some(1)),
            Ok(t.to_path_buf())
        );
        let e = resolve_env_target(Some(t), None, Some(0)).unwrap_err();
        assert!(e.contains("alpha") && e.contains("closed"), "{e}");
        let e = resolve_env_target(Some(t), Some(1), Some(0)).unwrap_err();
        assert!(e.contains("alpha") && e.contains("no longer active"), "{e}");
        assert!(resolve_env_target(Some(t), Some(1), None).is_err());
        assert!(resolve_env_target(None, Some(0), Some(0)).is_err());
    }

    fn exists() -> StorageError {
        StorageError::Exists {
            path: "x/collection.toml".into(),
        }
    }

    #[test]
    fn env_names_are_checked_for_empty_bad_characters_and_duplicates() {
        let existing = vec!["dev".to_string()];
        let cases: [(&str, bool); 8] = [
            ("staging", true),
            ("a-b_c.1", true),
            ("", false),
            ("dev", false),
            ("a/b", false),
            ("a b", false),
            ("naïve", false),
            ("é", false),
        ];
        for (name, ok) in cases {
            assert_eq!(check_env_name(name, &existing).is_ok(), ok, "{name:?}");
        }
        assert_eq!(check_env_name("", &[]).unwrap_err(), "Enter a name");
        assert!(
            check_env_name("dev", &existing)
                .unwrap_err()
                .contains("already exists")
        );
    }

    #[test]
    fn collection_outcomes_map_to_notices() {
        let ok = Ok(PathBuf::from("c/collection.toml"));
        assert_eq!(collection_outcome(&ok), CollectionOutcome::Created);
        assert_eq!(collection_notice(&CollectionOutcome::Created), (None, true));

        let present = collection_outcome(&Err(exists()));
        assert_eq!(present, CollectionOutcome::AlreadyPresent);
        assert_eq!(
            collection_notice(&present),
            (Some("Already a collection: opened it".to_string()), true)
        );

        let other = collection_outcome(&Err(StorageError::NotACollection { path: "p".into() }));
        let (notice, open) = collection_notice(&other);
        assert!(!open);
        assert!(notice.unwrap().starts_with("Cannot create collection:"));
    }

    #[test]
    fn create_env_writes_once_and_refuses_the_rest() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        ops::create_collection(&root.join("c")).unwrap();
        let root = root.join("c");
        assert!(create_env(&root, "dev", &[]).is_ok());
        assert!(root.join("environments/dev.toml").exists());
        // Present on disk but absent from the stale list: storage refuses.
        assert!(create_env(&root, "dev", &[]).is_err());
        // Invalid: nothing created.
        assert!(create_env(&root, "a/b", &[]).is_err());
        assert!(!root.join("environments/a").exists());
    }
}
