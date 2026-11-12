//! Auth sub-tab: type select plus the fields of the selected scheme.

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled,
    Subscription, Window, div,
};
use gpui_component::ActiveTheme as _;
use gpui_component::IndexPath;
use gpui_component::Sizable as _;
use gpui_component::button::Button;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::select::{Select, SelectEvent, SelectState};
use wisp_core::request::{ApiKeyLocation, Auth, ClientAuth};
use wisp_core::resolve::Scope;

use crate::tab::{AuthorizeState, RequestTab, TabEvent};
use crate::ui::oauth;
use crate::workspace::Workspace;

type LabelSelect = Entity<SelectState<Vec<&'static str>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthKind {
    None,
    Inherit,
    Basic,
    Bearer,
    ApiKey,
    Digest,
    AwsSigV4,
    OAuth2ClientCredentials,
    OAuth2AuthCode,
}

const KINDS: [AuthKind; 9] = [
    AuthKind::None,
    AuthKind::Inherit,
    AuthKind::Basic,
    AuthKind::Bearer,
    AuthKind::ApiKey,
    AuthKind::Digest,
    AuthKind::AwsSigV4,
    AuthKind::OAuth2ClientCredentials,
    AuthKind::OAuth2AuthCode,
];

impl AuthKind {
    fn of(auth: &Auth) -> Self {
        match auth {
            Auth::None => Self::None,
            Auth::Inherit => Self::Inherit,
            Auth::Basic { .. } => Self::Basic,
            Auth::Bearer { .. } => Self::Bearer,
            Auth::ApiKey { .. } => Self::ApiKey,
            Auth::Digest { .. } => Self::Digest,
            Auth::AwsSigV4 { .. } => Self::AwsSigV4,
            Auth::OAuth2ClientCredentials { .. } => Self::OAuth2ClientCredentials,
            Auth::OAuth2AuthCode { .. } => Self::OAuth2AuthCode,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Inherit => "Inherit",
            Self::Basic => "Basic",
            Self::Bearer => "Bearer",
            Self::ApiKey => "API key",
            Self::Digest => "Digest",
            Self::AwsSigV4 => "AWS Signature",
            Self::OAuth2ClientCredentials => "OAuth2 client credentials",
            Self::OAuth2AuthCode => "OAuth2 authorization code",
        }
    }

    fn from_label(label: &str) -> Option<Self> {
        KINDS.into_iter().find(|k| k.label() == label)
    }
}

/// One text field of the editor. Schemes that share a meaning share a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Username,
    Password,
    Token,
    KeyName,
    KeyValue,
    AccessKey,
    SecretKey,
    SessionToken,
    Region,
    Service,
    AuthUrl,
    TokenUrl,
    ClientId,
    ClientSecret,
    Scope,
    RedirectPort,
}

/// The auth value for `kind`, reading each field through `text`.
fn build_auth(
    kind: AuthKind,
    location: ApiKeyLocation,
    client_auth: ClientAuth,
    text: &dyn Fn(Slot) -> String,
) -> Auth {
    match kind {
        AuthKind::None => Auth::None,
        AuthKind::Inherit => Auth::Inherit,
        AuthKind::Basic => Auth::Basic {
            username: text(Slot::Username),
            password: text(Slot::Password),
        },
        AuthKind::Bearer => Auth::Bearer {
            token: text(Slot::Token),
        },
        AuthKind::ApiKey => Auth::ApiKey {
            name: text(Slot::KeyName),
            value: text(Slot::KeyValue),
            location,
        },
        AuthKind::Digest => Auth::Digest {
            username: text(Slot::Username),
            password: text(Slot::Password),
        },
        AuthKind::AwsSigV4 => Auth::AwsSigV4 {
            access_key: text(Slot::AccessKey),
            secret_key: text(Slot::SecretKey),
            session_token: text(Slot::SessionToken),
            region: text(Slot::Region),
            service: text(Slot::Service),
        },
        AuthKind::OAuth2ClientCredentials => Auth::OAuth2ClientCredentials {
            token_url: text(Slot::TokenUrl),
            client_id: text(Slot::ClientId),
            client_secret: text(Slot::ClientSecret),
            scope: text(Slot::Scope),
            client_auth,
        },
        AuthKind::OAuth2AuthCode => Auth::OAuth2AuthCode {
            auth_url: text(Slot::AuthUrl),
            token_url: text(Slot::TokenUrl),
            client_id: text(Slot::ClientId),
            client_secret: text(Slot::ClientSecret),
            scope: text(Slot::Scope),
            client_auth,
            redirect_port: parse_port(&text(Slot::RedirectPort)),
        },
    }
}

/// A redirect port from its field: blank or anything that is not a port means 0, any free port.
fn parse_port(text: &str) -> u16 {
    text.trim().parse().unwrap_or(0)
}

/// The field values an auth value carries.
fn slots_of(auth: &Auth) -> Vec<(Slot, String)> {
    match auth {
        Auth::None | Auth::Inherit => Vec::new(),
        Auth::Basic { username, password } | Auth::Digest { username, password } => vec![
            (Slot::Username, username.clone()),
            (Slot::Password, password.clone()),
        ],
        Auth::Bearer { token } => vec![(Slot::Token, token.clone())],
        Auth::ApiKey { name, value, .. } => vec![
            (Slot::KeyName, name.clone()),
            (Slot::KeyValue, value.clone()),
        ],
        Auth::AwsSigV4 {
            access_key,
            secret_key,
            session_token,
            region,
            service,
        } => vec![
            (Slot::AccessKey, access_key.clone()),
            (Slot::SecretKey, secret_key.clone()),
            (Slot::SessionToken, session_token.clone()),
            (Slot::Region, region.clone()),
            (Slot::Service, service.clone()),
        ],
        Auth::OAuth2ClientCredentials {
            token_url,
            client_id,
            client_secret,
            scope,
            ..
        } => vec![
            (Slot::TokenUrl, token_url.clone()),
            (Slot::ClientId, client_id.clone()),
            (Slot::ClientSecret, client_secret.clone()),
            (Slot::Scope, scope.clone()),
        ],
        Auth::OAuth2AuthCode {
            auth_url,
            token_url,
            client_id,
            client_secret,
            scope,
            redirect_port,
            ..
        } => vec![
            (Slot::AuthUrl, auth_url.clone()),
            (Slot::TokenUrl, token_url.clone()),
            (Slot::ClientId, client_id.clone()),
            (Slot::ClientSecret, client_secret.clone()),
            (Slot::Scope, scope.clone()),
            (
                Slot::RedirectPort,
                if *redirect_port == 0 {
                    String::new()
                } else {
                    redirect_port.to_string()
                },
            ),
        ],
    }
}

const CLIENT_AUTHS: [ClientAuth; 2] = [ClientAuth::Basic, ClientAuth::Body];

fn client_auth_label(c: ClientAuth) -> &'static str {
    match c {
        ClientAuth::Basic => "Basic",
        ClientAuth::Body => "Body",
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
    client_auth: ClientAuth,
    client_auth_select: LabelSelect,
    username: Entity<InputState>,
    password: Entity<InputState>,
    token: Entity<InputState>,
    key_name: Entity<InputState>,
    key_value: Entity<InputState>,
    access_key: Entity<InputState>,
    secret_key: Entity<InputState>,
    session_token: Entity<InputState>,
    region: Entity<InputState>,
    service: Entity<InputState>,
    auth_url: Entity<InputState>,
    token_url: Entity<InputState>,
    client_id: Entity<InputState>,
    client_secret: Entity<InputState>,
    scope: Entity<InputState>,
    redirect_port: Entity<InputState>,
    /// Shows the authorization URL as selectable text; never read back.
    shown_url: Entity<InputState>,
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
        let client_auth_select = cx.new(|cx| {
            let items: Vec<&'static str> =
                CLIENT_AUTHS.iter().map(|c| client_auth_label(*c)).collect();
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
        let access_key = plain("Access key", window, cx);
        let secret_key = secret("Secret key", window, cx);
        let session_token = secret("Session token (optional)", window, cx);
        let region = plain("us-east-1", window, cx);
        let service = plain("s3", window, cx);
        let auth_url = plain("https://idp.example/authorize", window, cx);
        let token_url = plain("https://idp.example/token", window, cx);
        let client_id = plain("Client ID", window, cx);
        let client_secret = secret("Client secret (optional)", window, cx);
        let scope = plain("Scope", window, cx);
        let redirect_port = plain("Blank or 0 = any free port", window, cx);
        let shown_url = plain("", window, cx);

        let mut subs = Vec::new();
        for input in [
            &username,
            &password,
            &token,
            &key_name,
            &key_value,
            &access_key,
            &secret_key,
            &session_token,
            &region,
            &service,
            &auth_url,
            &token_url,
            &client_id,
            &client_secret,
            &scope,
            &redirect_port,
        ] {
            subs.push(cx.subscribe_in(input, window, Self::on_input));
        }
        subs.push(cx.subscribe_in(&kind_select, window, Self::on_kind));
        subs.push(cx.subscribe_in(&loc_select, window, Self::on_location));
        subs.push(cx.subscribe_in(&client_auth_select, window, Self::on_client_auth));
        subs.push(cx.subscribe_in(&tab, window, Self::on_tab));
        subs.push(cx.observe(&workspace, |_, _, cx| cx.notify()));

        let mut this = Self {
            workspace,
            tab,
            kind: AuthKind::Inherit,
            location: ApiKeyLocation::Header,
            kind_select,
            loc_select,
            client_auth: ClientAuth::Basic,
            client_auth_select,
            username,
            password,
            token,
            key_name,
            key_value,
            access_key,
            secret_key,
            session_token,
            region,
            service,
            auth_url,
            token_url,
            client_id,
            client_secret,
            scope,
            redirect_port,
            shown_url,
            _subs: subs,
        };
        this.resync(window, cx);
        this
    }

    fn text(input: &Entity<InputState>, cx: &Context<Self>) -> String {
        input.read(cx).value().to_string()
    }

    fn input(&self, slot: Slot) -> &Entity<InputState> {
        match slot {
            Slot::Username => &self.username,
            Slot::Password => &self.password,
            Slot::Token => &self.token,
            Slot::KeyName => &self.key_name,
            Slot::KeyValue => &self.key_value,
            Slot::AccessKey => &self.access_key,
            Slot::SecretKey => &self.secret_key,
            Slot::SessionToken => &self.session_token,
            Slot::Region => &self.region,
            Slot::Service => &self.service,
            Slot::AuthUrl => &self.auth_url,
            Slot::TokenUrl => &self.token_url,
            Slot::ClientId => &self.client_id,
            Slot::ClientSecret => &self.client_secret,
            Slot::Scope => &self.scope,
            Slot::RedirectPort => &self.redirect_port,
        }
    }

    /// The draft value for the selected scheme, built from the field memory.
    fn current_auth(&self, cx: &Context<Self>) -> Auth {
        let text = |slot: Slot| Self::text(self.input(slot), cx);
        build_auth(self.kind, self.location, self.client_auth, &text)
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

    fn on_client_auth(
        &mut self,
        _: &LabelSelect,
        event: &SelectEvent<Vec<&'static str>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let SelectEvent::Confirm(Some(label)) = event else {
            return;
        };
        if let Some(c) = CLIENT_AUTHS
            .into_iter()
            .find(|c| client_auth_label(*c) == *label)
        {
            self.client_auth = c;
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
        match event {
            TabEvent::DraftReplaced => self.resync(window, cx),
            TabEvent::AuthorizeChanged => {
                let url = match &self.tab.read(cx).authorize {
                    AuthorizeState::Waiting { url: Some(u), .. } => u.clone(),
                    _ => String::new(),
                };
                self.shown_url
                    .update(cx, |s, cx| s.set_value(url, window, cx));
                cx.notify();
            }
            _ => {}
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
        for (slot, text) in slots_of(&auth) {
            let input = self.input(slot);
            set(input, text, window, cx);
        }
        if let Auth::ApiKey { location, .. } = auth {
            self.location = location;
            self.loc_select.update(cx, |s, cx| {
                s.set_selected_value(&location_label(location), window, cx)
            });
        }
        if let Auth::OAuth2ClientCredentials { client_auth, .. }
        | Auth::OAuth2AuthCode { client_auth, .. } = auth
        {
            self.client_auth = client_auth;
            self.client_auth_select.update(cx, |s, cx| {
                s.set_selected_value(&client_auth_label(client_auth), window, cx)
            });
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
        let mut names: Vec<String> = match (
            rel.as_deref().and_then(|p| p.parent()),
            ws.active_collection(),
        ) {
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

    fn oauth2_fields(&self, col: gpui::Div, auth_code: bool, cx: &mut Context<Self>) -> gpui::Div {
        let mut col = col;
        if auth_code {
            col = col.child(Self::row("Auth URL", Input::new(&self.auth_url)));
        }
        col = col
            .child(Self::row("Token URL", Input::new(&self.token_url)))
            .child(Self::row("Client ID", Input::new(&self.client_id)))
            .child(Self::row(
                "Client secret",
                Input::new(&self.client_secret).mask_toggle(),
            ))
            .child(Self::row("Scope", Input::new(&self.scope)))
            .child(Self::row(
                "Client auth",
                Select::new(&self.client_auth_select).id("auth-client-auth"),
            ));
        if !auth_code {
            return col;
        }
        col = col.child(Self::row("Redirect port", Input::new(&self.redirect_port)));
        let state = self.tab.read(cx).authorize.clone();
        let (workspace, tab) = (self.workspace.clone(), self.tab.clone());
        if state.waiting_id().is_none() {
            col = col.child(
                div().child(
                    Button::new("oauth-authorize")
                        .label("Authorize")
                        .small()
                        .on_click(move |_, _, cx| {
                            workspace.update(cx, |w, cx| {
                                if let Err(e) = w.authorize(&tab, cx) {
                                    let text = format!(
                                        "Cannot authorize: {}",
                                        crate::workspace::describe_problems(&e)
                                    );
                                    cx.emit(crate::workspace::WorkspaceEvent::Notice(text.into()));
                                }
                            });
                        }),
                ),
            );
        }
        let (workspace, tab) = (self.workspace.clone(), self.tab.clone());
        let strip = oauth::strip(
            &state,
            &self.shown_url,
            move |_, _, cx| {
                workspace.update(cx, |w, cx| w.authorize_cancel(&tab, cx));
            },
            cx,
        );
        col.children(strip)
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
            AuthKind::Digest => col
                .child(Self::row("Username", Input::new(&self.username)))
                .child(Self::row(
                    "Password",
                    Input::new(&self.password).mask_toggle(),
                )),
            AuthKind::AwsSigV4 => col
                .child(Self::row("Access key", Input::new(&self.access_key)))
                .child(Self::row(
                    "Secret key",
                    Input::new(&self.secret_key).mask_toggle(),
                ))
                .child(Self::row(
                    "Session token",
                    Input::new(&self.session_token).mask_toggle(),
                ))
                .child(Self::row("Region", Input::new(&self.region)))
                .child(Self::row("Service", Input::new(&self.service))),
            AuthKind::OAuth2ClientCredentials => self.oauth2_fields(col, false, cx),
            AuthKind::OAuth2AuthCode => self.oauth2_fields(col, true, cx),
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
    fn oauth2_kinds_are_labelled_and_offered() {
        for a in oauth_samples() {
            let kind = AuthKind::of(&a);
            assert!(KINDS.contains(&kind));
            assert_eq!(type_name(&a), kind.label());
        }
        let labels: Vec<&str> = KINDS.iter().map(|k| k.label()).collect();
        assert!(labels.contains(&"OAuth2 client credentials"));
        assert!(labels.contains(&"OAuth2 authorization code"));
    }

    fn round_trip(a: &Auth) -> Auth {
        let slots = slots_of(a);
        let text = |slot: Slot| {
            slots
                .iter()
                .find(|(s, _)| *s == slot)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let (location, client_auth) = match a {
            Auth::ApiKey { location, .. } => (*location, ClientAuth::Basic),
            Auth::OAuth2ClientCredentials { client_auth, .. }
            | Auth::OAuth2AuthCode { client_auth, .. } => (ApiKeyLocation::Header, *client_auth),
            _ => (ApiKeyLocation::Header, ClientAuth::Basic),
        };
        build_auth(AuthKind::of(a), location, client_auth, &text)
    }

    #[test]
    fn oauth2_fields_round_trip_through_the_slots() {
        for a in oauth_samples() {
            assert_eq!(round_trip(&a), a);
        }
        // Blank or invalid redirect port means any.
        assert_eq!(parse_port(""), 0);
        assert_eq!(parse_port("0"), 0);
        assert_eq!(parse_port(" 8123 "), 8123);
        assert_eq!(parse_port("99999"), 0);
    }

    #[test]
    fn oauth2_secret_slot_is_the_masked_secret_only() {
        // The secret text is never part of any other slot.
        for a in oauth_samples() {
            let secret = match &a {
                Auth::OAuth2ClientCredentials { client_secret, .. }
                | Auth::OAuth2AuthCode { client_secret, .. } => client_secret.clone(),
                _ => unreachable!(),
            };
            for (slot, v) in slots_of(&a) {
                assert_eq!(v == secret, slot == Slot::ClientSecret, "{slot:?}");
            }
        }
    }

    #[test]
    fn oauth2_build_auth_maps_only_inherit_to_inherit() {
        let all = [
            AuthKind::None,
            AuthKind::Inherit,
            AuthKind::Basic,
            AuthKind::Bearer,
            AuthKind::ApiKey,
            AuthKind::Digest,
            AuthKind::AwsSigV4,
            AuthKind::OAuth2ClientCredentials,
            AuthKind::OAuth2AuthCode,
        ];
        assert_eq!(all.len(), KINDS.len());
        for kind in all {
            let built = build_auth(kind, ApiKeyLocation::Header, ClientAuth::Basic, &|_| {
                String::new()
            });
            assert_eq!(
                built == Auth::Inherit,
                kind == AuthKind::Inherit,
                "{kind:?}"
            );
            assert_eq!(AuthKind::of(&built), kind);
        }
    }

    #[test]
    fn kind_list_offers_digest_and_aws_signature() {
        let labels: Vec<&str> = KINDS.iter().map(|k| k.label()).collect();
        assert!(labels.contains(&"Digest"));
        assert!(labels.contains(&"AWS Signature"));
        for k in KINDS {
            assert_eq!(AuthKind::from_label(k.label()), Some(k));
        }
    }

    #[test]
    fn every_kind_round_trips_through_the_field_helpers() {
        let samples = [
            Auth::None,
            Auth::Inherit,
            Auth::Basic {
                username: "u".into(),
                password: "p".into(),
            },
            Auth::Bearer { token: "t".into() },
            Auth::ApiKey {
                name: "n".into(),
                value: "v".into(),
                location: ApiKeyLocation::Query,
            },
            Auth::Digest {
                username: "u".into(),
                password: "p".into(),
            },
            Auth::AwsSigV4 {
                access_key: "ak".into(),
                secret_key: "sk".into(),
                session_token: "st".into(),
                region: "r".into(),
                service: "s".into(),
            },
        ];
        for a in &samples {
            let slots = slots_of(a);
            let text = |slot: Slot| {
                slots
                    .iter()
                    .find(|(s, _)| *s == slot)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            };
            let location = match a {
                Auth::ApiKey { location, .. } => *location,
                _ => ApiKeyLocation::Header,
            };
            assert_eq!(
                &build_auth(AuthKind::of(a), location, ClientAuth::Basic, &text),
                a
            );
        }
    }

    #[test]
    fn switching_kind_keeps_remembered_fields() {
        // Digest and Basic share the username and password slots.
        let basic = Auth::Basic {
            username: "u".into(),
            password: "p".into(),
        };
        let slots = slots_of(&basic);
        let text = |slot: Slot| {
            slots
                .iter()
                .find(|(s, _)| *s == slot)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        assert_eq!(
            build_auth(
                AuthKind::Digest,
                ApiKeyLocation::Header,
                ClientAuth::Basic,
                &text
            ),
            Auth::Digest {
                username: "u".into(),
                password: "p".into()
            }
        );
    }

    fn oauth_samples() -> Vec<Auth> {
        let mut v = Vec::new();
        for secret in ["literal-secret", "{{client_secret}}"] {
            v.push(Auth::OAuth2ClientCredentials {
                token_url: "https://i/t".into(),
                client_id: "cid".into(),
                client_secret: secret.into(),
                scope: "r w".into(),
                client_auth: ClientAuth::Body,
            });
            v.push(Auth::OAuth2AuthCode {
                auth_url: "https://i/a".into(),
                token_url: "https://i/t".into(),
                client_id: "cid".into(),
                client_secret: secret.into(),
                scope: "r".into(),
                client_auth: ClientAuth::Basic,
                redirect_port: 8123,
            });
        }
        v
    }

    #[test]
    fn oauth2_save_leaves_the_auth_value_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        for (n, auth) in oauth_samples().into_iter().enumerate() {
            let mut req = crate::draft::blank_request();
            req.auth = auth.clone();
            let path = dir.path().join(format!("r{n}.toml"));
            wisp_storage::request::save_request(&path, &req).unwrap();
            let back = wisp_storage::request::load_request(&path).unwrap();
            assert_eq!(back.auth, auth);
        }
    }

    /// The editor's commit computation (`current_auth`) over slots built from the draft, then save/load.
    #[test]
    fn oauth2_commit_then_save_load_leaves_the_auth_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        for (n, auth) in oauth_samples().into_iter().enumerate() {
            let slots = slots_of(&auth);
            let text = |slot: Slot| {
                slots
                    .iter()
                    .find(|(s, _)| *s == slot)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            };
            let client_auth = match &auth {
                Auth::OAuth2ClientCredentials { client_auth, .. }
                | Auth::OAuth2AuthCode { client_auth, .. } => *client_auth,
                _ => unreachable!(),
            };
            let committed = build_auth(
                AuthKind::of(&auth),
                ApiKeyLocation::Header,
                client_auth,
                &text,
            );
            assert_eq!(committed, auth);
            let mut req = crate::draft::blank_request();
            req.auth = committed;
            let path = dir.path().join(format!("c{n}.toml"));
            wisp_storage::request::save_request(&path, &req).unwrap();
            let back = wisp_storage::request::load_request(&path).unwrap();
            assert_eq!(back.auth, auth);
        }
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
