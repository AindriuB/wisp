//! Request pane: sub-tab strip over the six editors.

pub mod auth;
pub mod body;
pub mod headers;
pub mod params;
pub mod path;
pub mod settings;

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription,
    Window, div,
};
use gpui_component::tab::{Tab, TabBar};

use crate::draft::{DirtySections, dirty_sections};
use crate::tab::{RequestSubTab, RequestTab};
use crate::workspace::Workspace;

use auth::AuthEditor;
use body::BodyEditor;
use headers::HeadersEditor;
use params::ParamsEditor;
use path::PathEditor;
use settings::SettingsEditor;

const SUB_TABS: [(RequestSubTab, &str); 6] = [
    (RequestSubTab::Params, "Params"),
    (RequestSubTab::Path, "Path"),
    (RequestSubTab::Headers, "Headers"),
    (RequestSubTab::Auth, "Auth"),
    (RequestSubTab::Body, "Body"),
    (RequestSubTab::Settings, "Settings"),
];

fn is_dirty(sub: RequestSubTab, d: &DirtySections) -> bool {
    match sub {
        RequestSubTab::Params => d.params,
        RequestSubTab::Path => d.path,
        RequestSubTab::Headers => d.headers,
        RequestSubTab::Auth => d.auth,
        RequestSubTab::Body => d.body,
        RequestSubTab::Settings => d.settings,
    }
}

/// Sub-tab strip over the six editors.
pub struct RequestPane {
    tab: Entity<RequestTab>,
    params: Entity<ParamsEditor>,
    path: Entity<PathEditor>,
    headers: Entity<HeadersEditor>,
    auth: Entity<AuthEditor>,
    body: Entity<BodyEditor>,
    settings: Entity<SettingsEditor>,
    _sub: Subscription,
}

impl RequestPane {
    pub fn new(
        workspace: Entity<Workspace>,
        tab: Entity<RequestTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (w, t) = (&workspace, &tab);
        Self {
            params: cx.new(|cx| ParamsEditor::new(w.clone(), t.clone(), window, cx)),
            path: cx.new(|cx| PathEditor::new(w.clone(), t.clone(), window, cx)),
            headers: cx.new(|cx| HeadersEditor::new(w.clone(), t.clone(), window, cx)),
            auth: cx.new(|cx| AuthEditor::new(w.clone(), t.clone(), window, cx)),
            body: cx.new(|cx| BodyEditor::new(w.clone(), t.clone(), window, cx)),
            settings: cx.new(|cx| SettingsEditor::new(w.clone(), t.clone(), window, cx)),
            _sub: cx.subscribe(&tab, |_, _, _, cx| cx.notify()),
            tab,
        }
    }
}

impl Render for RequestPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (active, dirty) = {
            let t = self.tab.read(cx);
            (t.request_tab, dirty_sections(t.saved.as_ref(), &t.draft))
        };
        let selected = SUB_TABS.iter().position(|(s, _)| *s == active).unwrap_or(0);
        let labels: Vec<Tab> = SUB_TABS
            .iter()
            .map(|(s, name)| {
                let dot = if is_dirty(*s, &dirty) { " ●" } else { "" };
                Tab::new().label(format!("{name}{dot}"))
            })
            .collect();
        let tab = self.tab.clone();
        let strip = TabBar::new("request-sub-tabs")
            .selected_index(selected)
            .on_click(move |ix, _, cx| {
                if let Some((sub, _)) = SUB_TABS.get(*ix) {
                    let sub = *sub;
                    tab.update(cx, |t, cx| {
                        t.request_tab = sub;
                        cx.notify();
                    });
                }
            })
            .children(labels);
        let editor = match active {
            RequestSubTab::Params => div().child(self.params.clone()),
            RequestSubTab::Path => div().child(self.path.clone()),
            RequestSubTab::Headers => div().child(self.headers.clone()),
            RequestSubTab::Auth => div().child(self.auth.clone()),
            RequestSubTab::Body => div().child(self.body.clone()),
            RequestSubTab::Settings => div().child(self.settings.clone()),
        };
        div().size_full().child(strip).child(editor)
    }
}
