//! The Import dialog: curl, Postman v2.1 or OpenAPI 3 into the open collection.
//!
//! Parsing, file reads and writing all run on the background executor; the
//! pure helpers at the top are what the unit tests cover.

use std::path::{Path, PathBuf};

use gpui::{
    AppContext as _, Context, DismissEvent, Entity, EventEmitter, FocusHandle,
    InteractiveElement as _, IntoElement, ParentElement, PathPromptOptions, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Editor, EditorState, Input, InputState};
use gpui_component::{ActiveTheme as _, Disableable as _, v_flex};
use wisp_storage::import::{
    self, Format, ImportError, ImportItem, ImportResult, ImportWarning, WriteReport,
};
use wisp_storage::ops::{self, slug};

use crate::actions::CancelRequest;
use crate::workspace::Workspace;

/// Shown when a chosen file is none of the supported formats.
const UNSUPPORTED_FORMAT: &str = "Unrecognised file. Supported formats: curl, Postman collection v2.1, OpenAPI 3 (JSON or YAML).";

/// Counts shown in the preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub requests: usize,
    pub folders: usize,
    pub environments: usize,
}

fn count_items(items: &[ImportItem], s: &mut Summary) {
    for item in items {
        match item {
            ImportItem::Folder { items, .. } => {
                s.folders += 1;
                count_items(items, s);
            }
            ImportItem::Request { .. } => s.requests += 1,
        }
    }
}

pub fn summarize(result: &ImportResult) -> Summary {
    let mut s = Summary {
        requests: 0,
        folders: 0,
        environments: result.environments.len(),
    };
    count_items(&result.items, &mut s);
    s
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub fn summary_line(s: &Summary) -> String {
    format!(
        "{}, {}, {}",
        plural(s.requests, "request", "requests"),
        plural(s.folders, "folder", "folders"),
        plural(s.environments, "environment", "environments"),
    )
}

/// Default target folder: curl goes to the collection root, the others into a
/// folder named after the import.
pub fn default_folder(result: &ImportResult, format: Format) -> String {
    match format {
        Format::Curl => String::new(),
        _ => slug(&result.name),
    }
}

/// Folder path under the collection root for the typed name. Slugged, so it
/// is always one safe path component; empty means the root.
pub fn folder_rel(input: &str) -> PathBuf {
    let t = input.trim();
    if t.is_empty() {
        PathBuf::new()
    } else {
        PathBuf::from(slug(t))
    }
}

/// True when the result has secrets, so a secrets environment will be made.
pub fn has_secrets(result: &ImportResult) -> bool {
    result.environments.iter().any(|e| !e.secrets.is_empty())
}

/// One line per warning, in the importer's order.
pub fn warning_lines(warnings: &[ImportWarning]) -> Vec<String> {
    warnings
        .iter()
        .map(|w| {
            if w.at.is_empty() {
                w.message.clone()
            } else {
                format!("{}: {}", w.at, w.message)
            }
        })
        .collect()
}

/// Lines describing what a write created.
pub fn report_lines(r: &WriteReport) -> Vec<String> {
    let mut out = vec![format!(
        "Created {}, {}, {}",
        plural(r.requests, "request", "requests"),
        plural(r.folders, "folder", "folders"),
        plural(r.environments, "environment", "environments"),
    )];
    out.extend(
        r.renamed
            .iter()
            .map(|(from, to)| format!("Renamed \"{from}\" to \"{to}\" (name already in use)")),
    );
    out
}

fn error_line(e: &ImportError) -> String {
    e.to_string()
}

/// Read a regular file of at most the importer's input cap. Checks the
/// metadata first and reads at most one byte past the cap, so a huge file or
/// a FIFO never exhausts memory or blocks the dialog.
fn read_capped(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let max = import::MAX_INPUT_BYTES;
    let too_large = || {
        error_line(&ImportError::TooLarge(format!(
            "file is over {} MiB",
            max >> 20
        )))
    };
    let meta = std::fs::metadata(path).map_err(|e| format!("Cannot read file: {e}"))?;
    if !meta.is_file() {
        return Err("Cannot read file: not a regular file".to_string());
    }
    if meta.len() > max as u64 {
        return Err(too_large());
    }
    let file = std::fs::File::open(path).map_err(|e| format!("Cannot read file: {e}"))?;
    let mut bytes = Vec::new();
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Cannot read file: {e}"))?;
    if bytes.len() > max {
        return Err(too_large());
    }
    Ok(bytes)
}

/// Read, detect and parse a file. Runs on the background executor.
fn parse_file(path: &Path) -> Result<(Format, ImportResult), String> {
    let bytes = read_capped(path)?;
    let format = import::detect(&bytes).ok_or_else(|| UNSUPPORTED_FORMAT.to_string())?;
    let result = match format {
        Format::Curl => import::curl::parse(&String::from_utf8_lossy(&bytes)),
        Format::Postman => import::postman::parse(&bytes),
        Format::OpenApi => import::openapi::parse(&bytes),
    }
    .map_err(|e| error_line(&e))?;
    Ok((format, result))
}

/// Write the import, creating the target folder first when it is missing
/// (`import::write` requires it to exist). Runs on the background executor.
fn write_into(
    root: &Path,
    rel: &Path,
    result: &ImportResult,
    secrets_env: &str,
) -> Result<WriteReport, String> {
    let rel = if rel.as_os_str().is_empty() {
        rel.to_path_buf()
    } else {
        validated_target(root, rel)?
    };
    import::write(root, &rel, result, secrets_env).map_err(|e| error_line(&e))
}

/// Validate the target folder through storage's `ops` path checks before
/// anything is imported into it, creating it when missing. An existing folder
/// is probed by creating (and removing) a scratch folder inside it, which runs
/// the same reserved-name, symlink and confinement checks as any other
/// operation; a failure leaves nothing behind.
fn validated_target(root: &Path, rel: &Path) -> Result<PathBuf, String> {
    let abs = root.join(rel);
    match std::fs::symlink_metadata(&abs) {
        Ok(m) if m.file_type().is_symlink() => Err(format!("{} is a symlink", rel.display())),
        Ok(m) if !m.is_dir() => Err(format!("{} is not a folder", rel.display())),
        Ok(_) => {
            let probe =
                ops::create_folder(root, rel, "wisp-import-probe").map_err(|e| e.to_string())?;
            let _ = std::fs::remove_dir(root.join(&probe));
            Ok(rel.to_path_buf())
        }
        Err(_) => {
            if rel.to_string_lossy().eq_ignore_ascii_case("environments") {
                return Err("`environments` is not editable here".to_string());
            }
            ops::create_folder(root, Path::new(""), &rel.to_string_lossy())
                .map_err(|e| e.to_string())
        }
    }
}

enum Stage {
    Source,
    Preview {
        source: String,
        result: Box<ImportResult>,
        folder: Entity<InputState>,
        secrets: Entity<InputState>,
    },
    Done {
        report: WriteReport,
        warnings: Vec<ImportWarning>,
    },
}

/// The dialog may close unless an import is mid-write.
fn can_dismiss(writing: bool) -> bool {
    !writing
}

pub struct ImportView {
    workspace: Entity<Workspace>,
    stage: Stage,
    paste: Entity<EditorState>,
    error: Option<SharedString>,
    busy: bool,
    /// An import is being written: the dialog cannot be dismissed.
    writing: bool,
    focus: FocusHandle,
    prev_focus: Option<FocusHandle>,
}

impl EventEmitter<DismissEvent> for ImportView {}

impl ImportView {
    pub fn new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let prev_focus = window.focused(cx);
        window.focus(&focus, cx);
        Self {
            workspace,
            stage: Stage::Source,
            paste: cx.new(|cx| EditorState::new(window, cx)),
            error: None,
            busy: false,
            writing: false,
            focus,
            prev_focus,
        }
    }

    /// Return focus to where it was, then ask the host to close the dialog.
    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !can_dismiss(self.writing) {
            return;
        }
        if let Some(h) = self.prev_focus.take() {
            window.focus(&h, cx);
        }
        cx.emit(DismissEvent);
    }

    fn show_parsed(
        &mut self,
        source: String,
        outcome: Result<(Format, ImportResult), String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.busy = false;
        match outcome {
            Ok((format, result)) => {
                let folder_default = default_folder(&result, format);
                let folder = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("Collection root")
                        .default_value(folder_default)
                });
                let secrets = cx.new(|cx| InputState::new(window, cx).default_value("imported"));
                self.error = None;
                self.stage = Stage::Preview {
                    source,
                    result: Box::new(result),
                    folder,
                    secrets,
                };
            }
            Err(e) => self.error = Some(e.into()),
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn preview_paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.paste.read(cx).value().to_string();
        if self.busy || text.trim().is_empty() {
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    import::curl::parse(&text)
                        .map(|r| (Format::Curl, r))
                        .map_err(|e| error_line(&e))
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.show_parsed("Pasted curl".into(), outcome, window, cx);
            })
            .ok();
        })
        .detach();
    }

    fn choose_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            this.update(cx, |this, cx| {
                this.busy = true;
                this.error = None;
                cx.notify();
            })
            .ok();
            let outcome = cx
                .background_executor()
                .spawn(async move { parse_file(&path) })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.show_parsed(name, outcome, window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// The root of the collection open right now, not when the dialog opened.
    fn current_root(&self, cx: &gpui::App) -> Option<PathBuf> {
        self.workspace
            .read(cx)
            .collection
            .as_ref()
            .map(|c| c.tree.root.clone())
    }

    fn run_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.current_root(cx);
        let (
            Some(root),
            Stage::Preview {
                result,
                folder,
                secrets,
                ..
            },
        ) = (root, &self.stage)
        else {
            return;
        };
        if self.busy {
            return;
        }
        let rel = folder_rel(&folder.read(cx).value());
        let secrets_env = secrets.read(cx).value().trim().to_string();
        let secrets_env = if secrets_env.is_empty() {
            "imported".to_string()
        } else {
            secrets_env
        };
        let result = (**result).clone();
        self.busy = true;
        self.writing = true;
        self.error = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let warnings = result.warnings.clone();
            let outcome = cx
                .background_executor()
                .spawn(async move { write_into(&root, &rel, &result, &secrets_env) })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.busy = false;
                this.writing = false;
                match outcome {
                    Ok(report) => this.stage = Stage::Done { report, warnings },
                    Err(e) => this.error = Some(e.into()),
                }
                window.focus(&this.focus, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn lines(
        id: &'static str,
        lines: Vec<String>,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let muted = cx.theme().muted_foreground;
        div()
            .id(id)
            .max_h(px(180.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1()
            .text_xs()
            .text_color(muted)
            .children(lines)
    }

    fn close_button(&self, cx: &mut Context<Self>) -> Button {
        Button::new("import-close")
            .label(if self.writing {
                "Importing..."
            } else {
                "Close"
            })
            .disabled(self.writing)
            .on_click(cx.listener(|this, _, window, cx| this.dismiss(window, cx)))
    }
}

impl Render for ImportView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_root = self.current_root(cx).is_some();
        let mut body = v_flex().gap_3().child(div().text_lg().child("Import"));
        if !has_root {
            body = body
                .child("Open a collection first")
                .child(self.close_button(cx));
        } else {
            match &self.stage {
                Stage::Source => {
                    body = body
                        .child("Paste a curl command, or choose a Postman or OpenAPI file.")
                        .child(Editor::new(&self.paste).h(px(160.)))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_2()
                                .child(
                                    Button::new("import-paste")
                                        .label("Preview curl")
                                        .disabled(self.busy)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.preview_paste(window, cx)
                                        })),
                                )
                                .child(
                                    Button::new("import-file")
                                        .label("Choose file...")
                                        .disabled(self.busy)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.choose_file(window, cx)
                                        })),
                                )
                                .child(self.close_button(cx)),
                        );
                }
                Stage::Preview {
                    source,
                    result,
                    folder,
                    secrets,
                    ..
                } => {
                    let secret_note = has_secrets(result);
                    body = body
                        .child(format!("Source: {source}"))
                        .child(summary_line(&summarize(result)))
                        .child(Self::lines(
                            "import-warnings",
                            warning_lines(&result.warnings),
                            cx,
                        ))
                        .child("Folder (empty for the collection root)")
                        .child(Input::new(folder));
                    if secret_note {
                        body = body
                            .child("Secrets environment")
                            .child(Input::new(secrets))
                            .child(
                                div().text_xs().text_color(cx.theme().muted_foreground).child(
                                    "Credentials and Postman collection variables are stored in \
                                     this environment as {{variables}}; select it to resolve them.",
                                ),
                            );
                    }
                    body = body.child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(
                                Button::new("import-go")
                                    .primary()
                                    .label("Import")
                                    .disabled(self.busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.run_import(window, cx)
                                    })),
                            )
                            .child(self.close_button(cx)),
                    );
                }
                Stage::Done { report, warnings } => {
                    body = body
                        .child(Self::lines("import-report", report_lines(report), cx))
                        .child(Self::lines("import-warnings", warning_lines(warnings), cx))
                        .child(self.close_button(cx));
                }
            }
        }
        if let Some(e) = &self.error {
            body = body.child(div().text_color(cx.theme().danger).child(e.clone()));
        }
        div()
            .id("import-dialog")
            .track_focus(&self.focus)
            .w(px(560.))
            .p_4()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .on_action(cx.listener(|this, _: &CancelRequest, window, cx| this.dismiss(window, cx)))
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_core::request::{Request, Var};
    use wisp_storage::import::ImportedEnv;

    #[test]
    fn dismiss_allowed_unless_writing() {
        // Idle, previewing, done and errored all have `writing == false`.
        assert!(can_dismiss(false));
        assert!(!can_dismiss(true));
    }

    fn req(name: &str) -> ImportItem {
        ImportItem::Request {
            name: name.into(),
            request: Request::default(),
        }
    }

    fn nested() -> ImportResult {
        ImportResult {
            name: "My API / v2".into(),
            items: vec![
                req("a"),
                ImportItem::Folder {
                    name: "f".into(),
                    items: vec![
                        req("b"),
                        ImportItem::Folder {
                            name: "g".into(),
                            items: vec![req("c")],
                        },
                    ],
                },
            ],
            environments: vec![ImportedEnv::default()],
            ..Default::default()
        }
    }

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../storage/tests/fixtures/import"
        ))
        .join(name)
    }

    fn collection() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("collection.toml"), "format = 1\n").unwrap();
        dir
    }

    fn entries(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn parse_file_counts_each_fixture() {
        let cases = [
            ("postman.json", Format::Postman, (5, 2, 1)),
            ("openapi.json", Format::OpenApi, (6, 1, 2)),
            ("openapi.yaml", Format::OpenApi, (6, 1, 2)),
        ];
        for (name, want, counts) in cases {
            let (format, result) = parse_file(&fixture(name)).unwrap();
            let s = summarize(&result);
            assert_eq!(format, want, "{name}");
            assert_eq!((s.requests, s.folders, s.environments), counts, "{name}");
        }
    }

    #[test]
    fn rejects_non_regular_and_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        let err = parse_file(dir.path()).unwrap_err();
        assert!(err.contains("not a regular file"), "{err}");
        let big = dir.path().join("big.json");
        let f = std::fs::File::create(&big).unwrap();
        f.set_len(import::MAX_INPUT_BYTES as u64 + 1).unwrap();
        let err = parse_file(&big).unwrap_err();
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn imports_postman_into_a_new_folder() {
        let dir = collection();
        let (_, result) = parse_file(&fixture("postman.json")).unwrap();
        let rel = folder_rel(&default_folder(&result, Format::Postman));
        let report = write_into(dir.path(), &rel, &result, "imported").unwrap();
        assert!(report.requests > 0);
        assert!(dir.path().join(&rel).is_dir());
        assert!(!entries(&dir.path().join(&rel)).is_empty());
    }

    #[test]
    fn imports_into_an_existing_folder() {
        let dir = collection();
        std::fs::create_dir(dir.path().join("api")).unwrap();
        let (_, result) = parse_file(&fixture("postman.json")).unwrap();
        let report = write_into(dir.path(), Path::new("api"), &result, "imported").unwrap();
        assert!(report.requests > 0);
        assert!(!entries(&dir.path().join("api")).is_empty());
        assert!(
            !entries(&dir.path().join("api"))
                .iter()
                .any(|n| n.contains("probe"))
        );
        assert!(!dir.path().join("api-2").exists());
    }

    #[test]
    fn reserved_target_is_refused_with_nothing_written() {
        let (_, result) = parse_file(&fixture("postman.json")).unwrap();
        for existing in [false, true] {
            let dir = collection();
            if existing {
                std::fs::create_dir(dir.path().join("environments")).unwrap();
            }
            let before = entries(dir.path());
            let err =
                write_into(dir.path(), Path::new("environments"), &result, "imported").unwrap_err();
            assert!(!err.is_empty());
            assert_eq!(entries(dir.path()), before);
            if existing {
                assert!(entries(&dir.path().join("environments")).is_empty());
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_target_is_refused() {
        let dir = collection();
        let other = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(other.path(), dir.path().join("link")).unwrap();
        let (_, result) = parse_file(&fixture("postman.json")).unwrap();
        assert!(write_into(dir.path(), Path::new("link"), &result, "imported").is_err());
        assert!(entries(other.path()).is_empty());
    }

    #[test]
    fn write_failure_returns_an_error() {
        let dir = collection();
        std::fs::write(dir.path().join("api"), "not a folder").unwrap();
        let (_, result) = parse_file(&fixture("postman.json")).unwrap();
        assert!(write_into(dir.path(), Path::new("api"), &result, "imported").is_err());
    }

    #[test]
    fn counts_nested_result() {
        let s = summarize(&nested());
        assert_eq!((s.requests, s.folders, s.environments), (3, 2, 1));
        assert_eq!(summary_line(&s), "3 requests, 2 folders, 1 environment");
    }

    #[test]
    fn warnings_listed_in_order() {
        let w = |at: &str, m: &str| ImportWarning {
            at: at.into(),
            message: m.into(),
        };
        let lines = warning_lines(&[w("a / b", "first"), w("", "second"), w("c", "third")]);
        assert_eq!(lines, vec!["a / b: first", "second", "c: third"]);
    }

    #[test]
    fn slug_of_name_with_spaces_and_slashes() {
        assert_eq!(default_folder(&nested(), Format::Postman), "my-api-v2");
        assert_eq!(folder_rel(" My API/../x "), PathBuf::from("my-api-x"));
    }

    #[test]
    fn curl_targets_the_root() {
        assert_eq!(default_folder(&nested(), Format::Curl), "");
        assert_eq!(folder_rel(""), PathBuf::new());
    }

    #[test]
    fn secrets_environment_only_when_secrets_exist() {
        let mut r = nested();
        assert!(!has_secrets(&r));
        r.environments[0].secrets.push(Var {
            name: "token".into(),
            value: "x".into(),
        });
        assert!(has_secrets(&r));
    }

    #[test]
    fn report_lists_counts_and_renames() {
        let r = WriteReport {
            requests: 1,
            folders: 0,
            environments: 1,
            renamed: vec![("a".into(), "a-2".into())],
        };
        let lines = report_lines(&r);
        assert_eq!(lines[0], "Created 1 request, 0 folders, 1 environment");
        assert!(lines[1].contains("\"a\"") && lines[1].contains("\"a-2\""));
    }
}
