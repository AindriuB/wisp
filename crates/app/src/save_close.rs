//! Button choices shared by the tab, collection and quit prompts.

use gpui::{Context, Entity};

use crate::tab::RequestTab;
use crate::workspace::{Workspace, WorkspaceEvent};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ClosePrompt {
    Tab { can_save: bool },
    Collection,
    Quit,
}

impl ClosePrompt {
    pub fn buttons(self) -> &'static [&'static str] {
        match self {
            Self::Tab { can_save: true } => &["Save and close", "Discard", "Cancel"],
            Self::Tab { can_save: false } => &["Discard", "Cancel"],
            Self::Collection => &["Save all and close", "Discard", "Cancel"],
            Self::Quit => &["Save all and quit", "Discard", "Cancel"],
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Decision {
    Close,
    KeepOpen(String),
    Cancel,
}

fn saved_tab(title: &str, saved: anyhow::Result<()>, dirty: bool) -> Result<(), String> {
    saved.map_err(|e| format!("Could not save \"{title}\": {e}"))?;
    if dirty {
        return Err(format!("\"{title}\" still has unsaved changes"));
    }
    Ok(())
}

/// Evaluate saves lazily: Discard and Cancel never write, and one failure stops Save all.
fn decide(
    prompt: ClosePrompt,
    button: Option<usize>,
    mut saves: impl Iterator<Item = Result<(), String>>,
) -> Decision {
    let can_save = prompt != (ClosePrompt::Tab { can_save: false });
    match (can_save, button) {
        (true, Some(0)) => match saves.find_map(Result::err) {
            Some(notice) => Decision::KeepOpen(notice),
            None => Decision::Close,
        },
        (true, Some(1)) | (false, Some(0)) => Decision::Close,
        _ => Decision::Cancel,
    }
}

/// Resolve the answer against the tabs still open when the prompt returns.
pub fn finish(
    prompt: ClosePrompt,
    button: Option<usize>,
    tabs: &[Entity<RequestTab>],
    workspace: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> bool {
    workspace.prompt_up = false;
    let saves = tabs.iter().map(|tab| {
        if !matches!(prompt, ClosePrompt::Tab { .. }) && !tab.read(cx).is_dirty() {
            return Ok(());
        }
        let result = workspace.save(tab, cx);
        let tab = tab.read(cx);
        saved_tab(&tab.title(), result, tab.is_dirty())
    });
    match decide(prompt, button, saves) {
        Decision::Close => true,
        Decision::KeepOpen(notice) => {
            cx.emit(WorkspaceEvent::Notice(notice.into()));
            false
        }
        Decision::Cancel => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_close_requires_a_successful_clean_save() {
        let prompt = ClosePrompt::Tab { can_save: true };
        assert_eq!(prompt.buttons(), ["Save and close", "Discard", "Cancel"]);
        for (saved, dirty, expected) in [
            (Ok(()), false, Decision::Close),
            (
                Err(anyhow::anyhow!("write failed")),
                true,
                Decision::KeepOpen("Could not save \"one\": write failed".into()),
            ),
            (
                Err(anyhow::anyhow!("Changed on disk")),
                false,
                Decision::KeepOpen("Could not save \"one\": Changed on disk".into()),
            ),
            (
                Err(anyhow::anyhow!("Open a collection to save")),
                true,
                Decision::KeepOpen("Could not save \"one\": Open a collection to save".into()),
            ),
            (
                Ok(()),
                true,
                Decision::KeepOpen("\"one\" still has unsaved changes".into()),
            ),
        ] {
            assert_eq!(
                decide(
                    prompt,
                    Some(0),
                    std::iter::once_with(|| saved_tab("one", saved, dirty))
                ),
                expected
            );
        }
    }

    #[test]
    fn discard_cancel_and_dismiss_do_not_attempt_a_save() {
        for (prompt, discard, cancel) in [
            (ClosePrompt::Tab { can_save: false }, 0, 1),
            (ClosePrompt::Tab { can_save: true }, 1, 2),
            (ClosePrompt::Collection, 1, 2),
            (ClosePrompt::Quit, 1, 2),
        ] {
            assert_eq!(prompt.buttons()[discard], "Discard");
            assert_eq!(prompt.buttons()[cancel], "Cancel");
            for (button, expected) in [
                (Some(discard), Decision::Close),
                (Some(cancel), Decision::Cancel),
                (None, Decision::Cancel),
            ] {
                assert_eq!(
                    decide(
                        prompt,
                        button,
                        std::iter::once_with(|| panic!("unexpected save"))
                    ),
                    expected
                );
            }
        }
    }

    #[test]
    fn save_all_stops_at_the_first_failed_tab() {
        for prompt in [ClosePrompt::Collection, ClosePrompt::Quit] {
            let mut attempted = Vec::new();
            let saves = ["one", "two", "three"].into_iter().map(|name| {
                attempted.push(name);
                let result = if name == "two" {
                    Err(anyhow::anyhow!("write failed"))
                } else {
                    Ok(())
                };
                saved_tab(name, result, false)
            });
            assert_eq!(
                decide(prompt, Some(0), saves),
                Decision::KeepOpen("Could not save \"two\": write failed".into())
            );
            assert_eq!(attempted, ["one", "two"]);
        }
    }
}
