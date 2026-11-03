//! Auth sub-tab: type select plus the fields of the selected scheme.

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled,
    Subscription, Window, div,
};
use gpui_component::ActiveTheme as _;
use gpui_component::IndexPath;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::select::{Select, SelectEvent, SelectState};
use wisp_core::request::{ApiKeyLocation, Auth};
use wisp_core::resolve::Scope;

use crate::tab::{RequestTab, TabEvent};
use crate::workspace::Workspace;

type LabelSelect = Entity<SelectState<Vec<&'static str>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthKind {
    None,
    Inherit,
    Basic,
    Bearer,
    ApiKey,
}

const KINDS: [AuthKind; 5] = [
    AuthKind::None,
    AuthKind::Inherit,
    AuthKind::Basic,
    AuthKind::Bearer,
    AuthKind::ApiKey,
];

impl AuthKind {
    fn of(auth: &Auth) -> Self {
        match auth {
            Auth::None => Self::None,
            Auth::Inherit => Self::Inherit,
            Auth::Basic { .. } => Self::Basic,
            Auth::Bearer { .. } => Self::Bearer,
            Auth::ApiKey { .. } => Self::ApiKey,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Inherit => "Inherit",
            Self::Basic => "Basic",
            Self::Bearer => "Bearer",
            Self::ApiKey => "API key",
        }
    }

    fn from_label(label: &str) -> Option<Self> {
        KINDS.into_iter().find(|k| k.label() == label)
    }
}

const LOCATIONS: [ApiKeyLocation; 2] = [ApiKeyLocation::Header, ApiKeyLocation::Query];

fn location_label(location: ApiKeyLocation) -> &'static str {
    match location {
        ApiKeyLocation::Header => "Header",
        ApiKeyLocation::Query => "Query",
    }
}

fn type_name(auth: &Auth) -> &'static str {
    AuthKind::of(auth).label()
}

/// "Inherits <type> from <folder|collection>", walking nearest-first past
/// `Inherit`. `folders` holds display names, outermost first, parallel to
/// `scope.folders`. `None` when everything inherits. Never includes values.
pub fn inherited_line(scope: &Scope, folders: &[String]) -> Option<String> {
    for (i, f) in scope.folders.iter().enumerate().rev() {
        if f.auth != Auth::Inherit {
            let name = folders.get(i).map(String::as_str).unwrap_or("folder");
            return Some(format!("Inherits {} from {name}", type_name(&f.auth)));
        }
    }
    let auth = &scope.collection.auth;
    (*auth != Auth::Inherit).then(|| format!("Inherits {} from collection", type_name(auth)))
}

pub struct AuthEditor {
    workspace: Entity<Workspace>,
    tab: Entity<RequestTab>,
    kind: AuthKind,
    location: ApiKeyLocation,
    kind_select: LabelSelect,
    loc_select: LabelSelect,
    username: Entity<InputState>,
    password: Entity<InputState>,
    token: Entity<InputState>,
    key_name: Entity<InputState>,
    key_value: Entity<InputState>,
    _subs: Vec<Subscription>,
}

impl AuthEditor {
    pub fn new(
        workspace: Entity<Workspace>,
        tab: Entity<RequestTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let kind_select = cx.new(|cx| {
            let items: Vec<&'static str> = KINDS.iter().map(|k| k.label()).collect();
            SelectState::new(items, Some(IndexPath::new(0)), window, cx)
        });
        let loc_select = cx.new(|cx| {
            let items: Vec<&'static str> = LOCATIONS.iter().map(|l| location_label(*l)).collect();
            SelectState::new(items, Some(IndexPath::new(0)), window, cx)
        });
        let plain = |ph: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(ph))
        };
        let secret = |ph: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(ph).masked(true))
        };
        let username = plain("Username", window, cx);
        let password = secret("Password", window, cx);
        let token = secret("Token", window, cx);
        let key_name = plain("Name", window, cx);
        let key_value = secret("Value", window, cx);

        let mut subs = Vec::new();
        for input in [&username, &password, &token, &key_name, &key_value] {
            subs.push(cx.subscribe_in(input, window, Self::on_input));
        }
        subs.push(cx.subscribe_in(&kind_select, window, Self::on_kind));
        subs.push(cx.subscribe_in(&loc_select, window, Self::on_location));
        subs.push(cx.subscribe_in(&tab, window, Self::on_tab));
        subs.push(cx.observe(&workspace, |_, _, cx| cx.notify()));

        let mut this = Self {
            workspace,
            tab,
            kind: AuthKind::Inherit,
            location: ApiKeyLocation::Header,
            kind_select,
            loc_select,
            username,
            password,
            token,
            key_name,
            key_value,
            _subs: subs,
        };
        this.resync(window, cx);
        this
    }

    fn text(input: &Entity<InputState>, cx: &Context<Self>) -> String {
        input.read(cx).value().to_string()
    }

    /// The draft value for the selected scheme, built from the field memory.
    fn current_auth(&self, cx: &Context<Self>) -> Auth {
        match self.kind {
            AuthKind::None => Auth::None,
            AuthKind::Inherit => Auth::Inherit,
            AuthKind::Basic => Auth::Basic {
                username: Self::text(&self.username, cx),
                password: Self::text(&self.password, cx),
            },
            AuthKind::Bearer => Auth::Bearer {
                token: Self::text(&self.token, cx),
            },
            AuthKind::ApiKey => Auth::ApiKey {
                name: Self::text(&self.key_name, cx),
                value: Self::text(&self.key_value, cx),
                location: self.location,
            },
        }
    }

    /// One `tab.edit` per real change; echoes of a resync compare equal.
    fn commit(&mut self, cx: &mut Context<Self>) {
        let auth = self.current_auth(cx);
        let tab = self.tab.clone();
        tab.update(cx, |t, cx| {
            if t.draft.auth != auth {
                t.edit(|r| r.auth = auth, cx);
            }
        });
    }

    fn on_input(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::Change) {
            self.commit(cx);
        }
    }

    fn on_kind(
        &mut self,
        _: &LabelSelect,
        event: &SelectEvent<Vec<&'static str>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let SelectEvent::Confirm(Some(label)) = event else {
            return;
        };
        if let Some(kind) = AuthKind::from_label(label) {
            self.kind = kind;
            self.commit(cx);
            cx.notify();
        }
    }

    fn on_location(
        &mut self,
        _: &LabelSelect,
        event: &SelectEvent<Vec<&'static str>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let SelectEvent::Confirm(Some(label)) = event else {
            return;
        };
        if let Some(loc) = LOCATIONS.into_iter().find(|l| location_label(*l) == *label) {
            self.location = loc;
            self.commit(cx);
            cx.notify();
        }
    }

    fn on_tab(
        &mut self,
        _: &Entity<RequestTab>,
        event: &TabEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, TabEvent::DraftReplaced) {
            self.resync(window, cx);
        }
    }

    /// Loads the draft's scheme; other schemes keep their remembered values.
    fn resync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let auth = self.tab.read(cx).draft.auth.clone();
        self.kind = AuthKind::of(&auth);
        let set = |input: &Entity<InputState>,
                   text: String,
                   window: &mut Window,
                   cx: &mut Context<Self>| {
            if input.read(cx).value().as_ref() != text {
                input.update(cx, |s, cx| s.set_value(text, window, cx));
            }
        };
        match auth {
            Auth::None | Auth::Inherit => {}
            Auth::Basic { username, password } => {
                set(&self.username, username, window, cx);
                set(&self.password, password, window, cx);
            }
            Auth::Bearer { token } => set(&self.token, token, window, cx),
            Auth::ApiKey {
                name,
                value,
                location,
            } => {
                set(&self.key_name, name, window, cx);
                set(&self.key_value, value, window, cx);
                self.location = location;
                self.loc_select.update(cx, |s, cx| {
                    s.set_selected_value(&location_label(location), window, cx)
                });
            }
        }
        let label = self.kind.label();
        self.kind_select
            .update(cx, |s, cx| s.set_selected_value(&label, window, cx));
        cx.notify();
    }

    fn inherit_line(&self, cx: &Context<Self>) -> String {
        let tab = self.tab.read(cx);
        let rel = tab.path.clone();
        let ws = self.workspace.read(cx);
        let scope = ws.scope_for(rel.as_deref());
        // The same filter `scope_for` applies: ancestors without defaults are skipped.
        let mut names: Vec<String> = match (rel.as_deref().and_then(|p| p.parent()), &ws.collection)
        {
            (Some(parent), Some(c)) => parent
                .ancestors()
                .filter(|p| !p.as_os_str().is_empty())
                .filter(|p| c.tree.folders.contains_key(*p))
                .filter_map(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .collect(),
            _ => Vec::new(),
        };
        names.reverse();
        inherited_line(&scope, &names).unwrap_or_else(|| "No auth is inherited".to_string())
    }

    fn row(label: &'static str, body: impl IntoElement) -> gpui::Div {
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(div().w(gpui::px(90.)).child(label))
            .child(div().flex_1().child(body))
    }
}

impl Render for AuthEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut col = div().flex().flex_col().gap_2().p_2().child(Self::row(
            "Type",
            Select::new(&self.kind_select).id("auth-kind"),
        ));
        col = match self.kind {
            AuthKind::None => col,
            AuthKind::Inherit => {
                let line: SharedString = self.inherit_line(cx).into();
                col.child(div().text_color(cx.theme().muted_foreground).child(line))
            }
            AuthKind::Basic => col
                .child(Self::row("Username", Input::new(&self.username)))
                .child(Self::row(
                    "Password",
                    Input::new(&self.password).mask_toggle(),
                )),
            AuthKind::Bearer => {
                col.child(Self::row("Token", Input::new(&self.token).mask_toggle()))
            }
            AuthKind::ApiKey => col
                .child(Self::row("Name", Input::new(&self.key_name)))
                .child(Self::row(
                    "Value",
                    Input::new(&self.key_value).mask_toggle(),
                ))
                .child(Self::row(
                    "Add to",
                    Select::new(&self.loc_select).id("auth-location"),
                )),
        };
        div()
            .size_full()
            .child(col.size_full().overflow_y_scrollbar())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_core::request::{CollectionDefaults, FolderDefaults};

    fn folder(auth: Auth) -> FolderDefaults {
        FolderDefaults {
            vars: Vec::new(),
            headers: Vec::new(),
            auth,
            order: Vec::new(),
        }
    }

    fn collection(auth: Auth) -> CollectionDefaults {
        CollectionDefaults {
            auth,
            ..CollectionDefaults::default()
        }
    }

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn nearest_non_inherit_folder_wins() {
        let c = collection(Auth::None);
        let outer = folder(Auth::Basic {
            username: "u".into(),
            password: "p".into(),
        });
        let inner = folder(Auth::Bearer { token: "t".into() });
        let skip = folder(Auth::Inherit);
        let scope = Scope {
            collection: &c,
            folders: vec![&outer, &inner, &skip],
            environment: None,
        };
        let line = inherited_line(&scope, &names(&["a", "b", "c"])).unwrap();
        assert_eq!(line, "Inherits Bearer from b");
    }

    #[test]
    fn falls_back_to_collection() {
        let c = collection(Auth::ApiKey {
            name: "k".into(),
            value: "secret".into(),
            location: ApiKeyLocation::Header,
        });
        let f = folder(Auth::Inherit);
        let scope = Scope {
            collection: &c,
            folders: vec![&f],
            environment: None,
        };
        let line = inherited_line(&scope, &names(&["a"])).unwrap();
        assert_eq!(line, "Inherits API key from collection");
        assert!(!line.contains("secret"));
    }

    #[test]
    fn all_inherit_is_none() {
        let c = collection(Auth::Inherit);
        let f = folder(Auth::Inherit);
        let scope = Scope {
            collection: &c,
            folders: vec![&f],
            environment: None,
        };
        assert_eq!(inherited_line(&scope, &names(&["a"])), None);
    }
}
