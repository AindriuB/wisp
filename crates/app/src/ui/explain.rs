//! Read-only explanation of the effective request.

use gpui::{
    Context, Entity, InteractiveElement as _, IntoElement, ParentElement, Render,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div,
};
use gpui_component::{ActiveTheme as _, StyledExt as _};
use wisp_core::resolve::{
    AuthApplication, HideReason, Origin, ParamKind, ResolutionReport, Setting, SettingSource,
    Shown, ShownPart, Synthetic, explain,
};

use crate::tab::{RequestTab, ResponseSubTab, TabEvent};
use crate::ui::url_bar::{folder_names, location_text, problem_text};
use crate::workspace::{Workspace, WorkspaceEvent, scope_path_for};

pub(crate) fn tab_folders(
    workspace: &Workspace,
    tab: &Entity<RequestTab>,
    cx: &gpui::App,
) -> Vec<String> {
    let Some(collection) = workspace
        .collections
        .iter()
        .find(|c| c.group.tabs.contains(tab))
    else {
        return Vec::new();
    };
    let t = tab.read(cx);
    let (path, _, _) = scope_path_for(
        &collection.tree,
        t.path.as_deref(),
        t.from_history.as_ref(),
        &t.draft.url,
    );
    folder_names(path.as_deref())
}

pub(crate) fn part_text(part: &ShownPart, _folders: &[String]) -> String {
    match part {
        ShownPart::Text(text) => {
            let mut chars = text.chars();
            let short: String = chars.by_ref().take(200).collect();
            if chars.next().is_some() {
                format!("{short}…")
            } else {
                short
            }
        }
        ShownPart::Secret { name, resolved } => format!(
            "secret {name} ({})",
            if *resolved { "resolved" } else { "not set" }
        ),
        ShownPart::Dynamic { name } => format!(
            "{}, generated at send",
            if name.starts_with('$') {
                name.clone()
            } else {
                format!("${name}")
            }
        ),
        ShownPart::Hidden(HideReason::Credential) => "hidden: credential name".into(),
        ShownPart::Hidden(HideReason::Userinfo) => "hidden: URL userinfo".into(),
        ShownPart::Hidden(HideReason::TooLong) => "hidden: over 200 characters".into(),
    }
}

pub(crate) fn origin_text(origin: &Origin, folders: &[String]) -> String {
    match origin {
        Origin::Request => "request".into(),
        Origin::Folder(i) => format!(
            "folder {}",
            folders.get(*i).map_or("(unknown)", String::as_str)
        ),
        Origin::Collection => "collection".into(),
        Origin::Environment => "environment".into(),
        Origin::Secret => "secret".into(),
        Origin::Dynamic => "dynamic".into(),
        Origin::BodyType => "body type".into(),
        Origin::Auth => "auth".into(),
    }
}

fn shown_text(shown: &Shown, folders: &[String]) -> String {
    shown.0.iter().map(|p| part_text(p, folders)).collect()
}

fn setting_text(name: &str, setting: &Setting) -> String {
    let source = match setting.source {
        SettingSource::Request => "request",
        SettingSource::Default => "default",
    };
    let note = if name == "Timeout" && setting.source == SettingSource::Default {
        " (engine default: 30s)"
    } else {
        ""
    };
    format!("{name}: {} ({source}){note}", setting.value)
}

fn report_sections(
    report: &ResolutionReport,
    folders: &[String],
) -> Vec<(&'static str, Vec<String>)> {
    let mut variables = Vec::new();
    for v in &report.variables {
        let shadowed = if v.shadowed.is_empty() {
            String::new()
        } else {
            format!(
                "; shadowed: {}",
                v.shadowed
                    .iter()
                    .map(|o| origin_text(o, folders))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let source = match (&v.origin, &report.environment) {
            (Origin::Environment, Some(env)) => format!("environment {}", env.name),
            (Origin::Secret, _) => format!("secret {}", v.name),
            _ => origin_text(&v.origin, folders),
        };
        let locations = v
            .locations
            .iter()
            .map(location_text)
            .collect::<Vec<_>>()
            .join(", ");
        variables.push(format!(
            "{}: {} (from {source}){shadowed}; used in {locations}",
            v.name,
            shown_text(&v.value, folders),
        ));
    }
    let mut url = vec![format!(
        "{:?} {}",
        report.method,
        shown_text(&report.url, folders)
    )];
    for p in &report.params {
        let kind = match p.kind {
            ParamKind::Query => "query",
            ParamKind::Path => "path",
        };
        url.push(format!(
            "{kind}: {} = {} (from {})",
            shown_text(&p.name, folders),
            shown_text(&p.value, folders),
            origin_text(&p.origin, folders)
        ));
    }
    let headers = report
        .headers
        .iter()
        .map(|h| {
            let source = match h.synthetic {
                Some(Synthetic::BodyType(k)) => format!("body type {}", k.label()),
                Some(Synthetic::Auth(k)) => format!("auth {}", k.label()),
                None => origin_text(&h.origin, folders),
            };
            let overridden = if h.overridden.is_empty() {
                String::new()
            } else {
                format!(
                    "; overrides {}",
                    h.overridden
                        .iter()
                        .map(|o| origin_text(o, folders))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            format!(
                "{}: {} (from {source}){overridden}",
                shown_text(&h.name, folders),
                shown_text(&h.value, folders)
            )
        })
        .collect();
    let a = &report.auth;
    let source = a
        .origin
        .as_ref()
        .map_or_else(|| "none".into(), |o| origin_text(o, folders));
    let application = match &a.application {
        AuthApplication::None => "none".into(),
        AuthApplication::Header(n) => format!("header {}", shown_text(n, folders)),
        AuthApplication::Query(n) => format!("query {}", shown_text(n, folders)),
        AuthApplication::AtSend => "at send by the engine".into(),
    };
    let mut auth = vec![format!(
        "{} (from {source}); applied as {application}",
        a.kind.label()
    )];
    auth.extend(a.fields.iter().map(|f| format!("field: {}", f.name)));
    let settings = vec![
        setting_text("Timeout", &report.settings.timeout),
        setting_text("Follow redirects", &report.settings.follow_redirects),
        setting_text("Verify TLS", &report.settings.verify_tls),
    ];
    let bytes = report
        .body
        .bytes
        .map_or_else(|| "unknown".into(), |n| format!("{n} bytes"));
    let body = vec![format!(
        "{}; {bytes}; variables: {}",
        report.body.kind.label(),
        if report.body.vars.is_empty() {
            "none".into()
        } else {
            report.body.vars.join(", ")
        }
    )];
    let mut findings: Vec<_> = report.problems.iter().map(problem_text).collect();
    findings.extend(report.warnings.iter().map(|w| match w {
        wisp_core::resolve::ReportWarning::PatternMismatch { name, pattern } => {
            format!("{name} does not match {pattern}")
        }
        wisp_core::resolve::ReportWarning::InvalidPattern {
            name,
            pattern,
            message,
        } => format!("{name}: invalid pattern {pattern}: {message}"),
    }));
    vec![
        ("Variables", variables),
        ("URL and params", url),
        ("Headers", headers),
        ("Auth", auth),
        ("Settings", settings),
        ("Body", body),
        ("Problems and warnings", findings),
    ]
}

pub struct ExplainPane {
    workspace: Entity<Workspace>,
    tab: Entity<RequestTab>,
    sections: Vec<(&'static str, Vec<String>)>,
    _subs: Vec<Subscription>,
}
impl ExplainPane {
    pub fn new(
        workspace: Entity<Workspace>,
        tab: Entity<RequestTab>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subs = vec![
            cx.subscribe(&tab, |this, _, event: &TabEvent, cx| {
                if matches!(
                    event,
                    TabEvent::DraftEdited
                        | TabEvent::DraftReplaced
                        | TabEvent::ResponseChanged
                        | TabEvent::Saved
                ) {
                    this.refresh(cx);
                }
            }),
            cx.subscribe(&workspace, |this, _, event: &WorkspaceEvent, cx| {
                if matches!(
                    event,
                    WorkspaceEvent::EnvironmentChanged
                        | WorkspaceEvent::SecretsChanged
                        | WorkspaceEvent::CollectionChanged
                ) {
                    this.refresh(cx);
                }
            }),
        ];
        Self {
            workspace,
            tab,
            sections: Vec::new(),
            _subs: subs,
        }
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.tab.read(cx).response_tab != ResponseSubTab::Explain {
            return;
        }
        let ws = self.workspace.read(cx);
        let folders = tab_folders(ws, &self.tab, cx);
        let report = explain(&self.tab.read(cx).draft, &ws.send_scope(&self.tab, cx));
        self.sections = report_sections(&report, &folders);
        cx.notify();
    }
}
impl Render for ExplainPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div()
            .id("explain-pane")
            .size_full()
            .overflow_y_scroll()
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .text_sm();
        for (title, lines) in &self.sections {
            let mut section = div().flex().flex_col().child(
                div()
                    .font_bold()
                    .text_color(cx.theme().foreground)
                    .child(*title),
            );
            if lines.is_empty() {
                section =
                    section.child(div().text_color(cx.theme().muted_foreground).child("None"));
            }
            for line in lines {
                section = section.child(div().child(line.clone()));
            }
            root = root.child(section);
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::url_bar::strip_text;
    use crate::workspace::HistoryScope;
    use wisp_core::request::{Auth, CollectionDefaults, Environment, KeyValue, Request, Var};
    use wisp_core::resolve::{Scope, explain_auth};

    #[test]
    fn explain_parts_and_origins_are_safe_and_readable() {
        let folders = vec!["users".to_string()];
        assert_eq!(origin_text(&Origin::Folder(0), &folders), "folder users");
        assert_eq!(
            part_text(
                &ShownPart::Secret {
                    name: "KEY".into(),
                    resolved: true
                },
                &folders
            ),
            "secret KEY (resolved)"
        );
        assert_eq!(
            part_text(
                &ShownPart::Secret {
                    name: "KEY".into(),
                    resolved: false
                },
                &folders
            ),
            "secret KEY (not set)"
        );
        assert_eq!(
            part_text(
                &ShownPart::Dynamic {
                    name: "$uuid".into()
                },
                &folders
            ),
            "$uuid, generated at send"
        );
        assert_eq!(
            part_text(&ShownPart::Hidden(HideReason::Credential), &folders),
            "hidden: credential name"
        );
        assert_eq!(
            part_text(&ShownPart::Text("é".repeat(201)), &folders)
                .chars()
                .count(),
            201
        );
    }

    #[test]
    fn explain_canary_never_renders_secret_value() {
        let marker = "CANARY_731d52a_unique_secret";
        let env = Environment {
            name: "dev".into(),
            vars: vec![],
            secrets: vec![Var {
                name: "key".into(),
                value: marker.into(),
            }],
        };
        let req = Request {
            url: "https://example.test/{{key}}".into(),
            headers: vec![KeyValue {
                name: "X-Test".into(),
                value: "{{key}}".into(),
                enabled: true,
            }],
            auth: Auth::Bearer {
                token: "{{key}}".into(),
            },
            ..Default::default()
        };
        let defaults = CollectionDefaults::default();
        let scope = Scope {
            collection: &defaults,
            folders: vec![],
            environment: Some(&env),
        };
        let report = explain(&req, &scope);
        let sections = report_sections(&report, &[]);
        for (title, lines) in sections {
            assert!(!title.contains(marker));
            for line in lines {
                assert!(!line.contains(marker), "{line}");
            }
        }
        let strip = strip_text(
            Some(("dev", None)),
            &explain_auth(&req, &scope),
            &HistoryScope::Scratch,
            &[],
        );
        assert!(!strip.contains(marker));
    }
}
