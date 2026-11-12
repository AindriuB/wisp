//! Keychain wiring: store selection, the refresh-token adapter, the environment load job, the
//! confirmed secret actions and the status text shown for them.
//!
//! Everything here is blocking or pure; the workspace runs the blocking parts on a background
//! task. No function returns, logs or formats a secret value: notices and banners carry names,
//! sizes and paths only.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use wisp_core::auth::oauth2::{Grant, RefreshStoreError, RefreshTokenStore, TokenKey};
use wisp_core::request::Environment;
use wisp_storage::StorageError;
use wisp_storage::collection::{CollectionId, read_collection_id};
use wisp_storage::data_dir::DataDir;
use wisp_storage::keychain;
use wisp_storage::vault::{
    FailReason, LoadStatus, LoadedEnvironment, SecretStore, SecretStoreError, SecretStores,
    SecretValue, SecretsError, StoreKind, UnavailableReason, VaultCtx, bind,
    bind_additional_checkout, bound_roots, delete_secret, import_secrets_file,
    load_environment_with, migrate_to_file, migrate_to_store, rebind, refresh_token_account,
    set_secret, share_with_folder,
};

/// Environment variable that overrides the keychain service name.
pub const KEYCHAIN_SERVICE_ENV: &str = "WISP_KEYCHAIN_SERVICE";

/// The size cap of one keychain value, as shown to the user.
pub const KEYCHAIN_LIMIT_TEXT: &str = "2,560 B";

/// The stores the app hands to storage and the refresh-token adapter.
pub type SharedStores = Arc<dyn SecretStores + Send + Sync>;

/// The keychain service: `WISP_KEYCHAIN_SERVICE` when set and non-empty, else `wisp`.
pub fn service_name(var: Option<&str>) -> String {
    match var {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => "wisp".to_string(),
    }
}

/// True when a `portable` marker sits beside the executable (the rule `DataDir::resolve` uses).
pub fn portable_marker() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("portable").exists()))
        .unwrap_or(false)
}

/// The app's one [`SecretStores`]. A future store kind is added in `get` and nowhere else.
pub struct AppStores {
    keychain: Box<dyn SecretStore>,
}

impl AppStores {
    /// Open the OS keychain store (inert when `portable`).
    pub fn open(portable: bool) -> Self {
        let service = service_name(std::env::var(KEYCHAIN_SERVICE_ENV).ok().as_deref());
        Self {
            keychain: keychain::open(&service, portable),
        }
    }
}

impl SecretStores for AppStores {
    fn get(&self, kind: StoreKind) -> Option<&dyn SecretStore> {
        match kind {
            StoreKind::Keychain => Some(&*self.keychain),
            _ => None,
        }
    }
}

// ---- refresh tokens ----

fn grant_name(g: Grant) -> &'static str {
    match g {
        Grant::ClientCredentials => "client_credentials",
        Grant::AuthCode => "auth_code",
    }
}

/// Persists OAuth2 refresh tokens in the keychain store, under `refresh_token_account`.
///
/// A token over the keychain cap is kept in a session-only map in this adapter and nowhere else.
pub struct KeychainRefreshStore {
    stores: SharedStores,
    session: Mutex<HashMap<String, String>>,
    warned: Mutex<HashSet<String>>,
    banners: Mutex<Vec<String>>,
}

impl KeychainRefreshStore {
    pub fn new(stores: SharedStores) -> Self {
        Self {
            stores,
            session: Mutex::new(HashMap::new()),
            warned: Mutex::new(HashSet::new()),
            banners: Mutex::new(Vec::new()),
        }
    }

    /// Banners raised since the last call (a too-large token, once per account).
    pub fn take_banners(&self) -> Vec<String> {
        std::mem::take(&mut *self.banners.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// The store account and environment of `key`; a key with no collection or no environment
    /// is refused.
    fn account(key: &TokenKey) -> Result<(String, String), RefreshStoreError> {
        let col = key.collection.as_deref().ok_or_else(|| {
            RefreshStoreError::new("refresh token not stored: the request has no collection")
        })?;
        let env = key.environment.as_deref().ok_or_else(|| {
            RefreshStoreError::new("refresh token not stored: no environment is active")
        })?;
        let id = CollectionId::parse(col).ok_or_else(|| {
            RefreshStoreError::new("refresh token not stored: the collection has no usable id")
        })?;
        let account = refresh_token_account(
            &id,
            env,
            grant_name(key.grant),
            &key.token_url,
            &key.client_id,
            &key.scope,
        );
        Ok((account, env.to_string()))
    }

    fn store(&self) -> Result<&dyn SecretStore, RefreshStoreError> {
        self.stores
            .get(StoreKind::Keychain)
            .ok_or_else(|| RefreshStoreError::new("the keychain is not available"))
    }

    fn session(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn store_failure(what: &str, e: &SecretStoreError) -> RefreshStoreError {
    RefreshStoreError::new(format!("keychain {what} failed: {e}"))
}

impl RefreshTokenStore for KeychainRefreshStore {
    fn load(&self, key: &TokenKey) -> Result<Option<String>, RefreshStoreError> {
        let (account, _) = Self::account(key)?;
        if let Some(t) = self.session().get(&account) {
            return Ok(Some(t.clone()));
        }
        self.store()?
            .get(&account)
            .map(|v| v.map(|v| v.expose().to_string()))
            .map_err(|e| store_failure("read", &e))
    }

    fn save(&self, key: &TokenKey, refresh_token: &str) -> Result<(), RefreshStoreError> {
        let (account, env) = Self::account(key)?;
        match self
            .store()?
            .set(&account, &SecretValue::new(refresh_token))
        {
            Ok(()) => {
                self.session().remove(&account);
                Ok(())
            }
            Err(SecretStoreError::TooLarge) => {
                self.session()
                    .insert(account.clone(), refresh_token.to_string());
                let first = self
                    .warned
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(account);
                if first {
                    self.banners
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(too_large_token_banner(&env, refresh_token.len()));
                }
                Ok(())
            }
            Err(e) => Err(store_failure("write", &e)),
        }
    }

    fn forget(&self, key: &TokenKey) -> Result<(), RefreshStoreError> {
        let (account, _) = Self::account(key)?;
        self.session().remove(&account);
        self.store()?
            .delete(&account)
            .map_err(|e| store_failure("delete", &e))
    }
}

/// The banner for a refresh token the keychain refused for size. Holds no token.
pub fn too_large_token_banner(env: &str, bytes: usize) -> String {
    format!(
        "Refresh token for {env} is too large for the keychain ({bytes} B > \
         {KEYCHAIN_LIMIT_TEXT}): kept for this session only"
    )
}

// ---- load status ----

/// Where an environment's load stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvState {
    Loading,
    Loaded {
        status: LoadStatus,
        unresolved: Vec<String>,
        /// The open root is one of the collection's bound roots, so it may share the secrets.
        can_share: bool,
        /// `<env>.secrets.toml` exists beside the environment.
        secrets_file: bool,
    },
    Error(String),
}

/// The load state of one collection's environment. Never holds a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvStatus {
    pub env: String,
    pub state: EnvState,
}

/// An action a banner offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerAction {
    /// "Use keychain secrets…": bind this collection.
    UseKeychain,
    /// "This is the same collection, moved": rebind.
    SameCollectionMoved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Banner {
    pub text: String,
    pub action: Option<BannerAction>,
}

fn plain(text: String) -> Banner {
    Banner { text, action: None }
}

fn unbound_banner(env: &str) -> Banner {
    Banner {
        text: format!("Keychain secrets for {env} are not enabled for this collection"),
        action: Some(BannerAction::UseKeychain),
    }
}

impl EnvStatus {
    /// True only when this status is for `active` and it loaded from the keychain. Every other
    /// status, including a fallback to the file, keeps refresh tokens in memory.
    pub fn persist_refresh(&self, active: &str) -> bool {
        self.env == active
            && matches!(
                &self.state,
                EnvState::Loaded {
                    status: LoadStatus::Store {
                        kind: StoreKind::Keychain,
                        ..
                    },
                    ..
                }
            )
    }

    /// What to show about the load, for the banner strip.
    pub fn banners(&self) -> Vec<Banner> {
        let env = &self.env;
        match &self.state {
            EnvState::Loading => Vec::new(),
            EnvState::Error(e) => vec![plain(format!("Environment {env} not loaded: {e}"))],
            EnvState::Loaded { status, .. } => match status {
                LoadStatus::File => Vec::new(),
                LoadStatus::FellBackToFile { reason } => vec![plain(format!(
                    "Keychain unavailable ({reason}): using {env}.secrets.toml"
                ))],
                LoadStatus::Store { file_ignored, .. } => {
                    if *file_ignored {
                        vec![plain(format!(
                            "{env}.secrets.toml is ignored: {env} keeps its secrets in the keychain"
                        ))]
                    } else {
                        Vec::new()
                    }
                }
                LoadStatus::Failed { reason } => vec![fail_banner(env, reason)],
            },
        }
    }

    /// The mode shown in the Environment secrets panel.
    pub fn mode_label(&self) -> &'static str {
        match &self.state {
            EnvState::Loading => "loading",
            EnvState::Error(_) => "not loaded",
            EnvState::Loaded { status, .. } => match status {
                LoadStatus::File => "file",
                LoadStatus::FellBackToFile { .. } => "file (keychain unavailable)",
                LoadStatus::Store { .. } => "keychain",
                LoadStatus::Failed { reason } => match reason {
                    FailReason::Unbound | FailReason::NoCollectionId => "keychain (not enabled)",
                    _ => "keychain (secrets not loaded)",
                },
            },
        }
    }
}

fn fail_banner(env: &str, reason: &FailReason) -> Banner {
    match reason {
        FailReason::UnknownStoreKind(kind) => plain(format!(
            "Unknown secret store \"{kind}\" for {env}: secrets not loaded"
        )),
        FailReason::Store(SecretStoreError::Unavailable {
            reason: UnavailableReason::TimedOut,
        }) => plain(format!(
            "Keychain did not answer: secrets for {env} not loaded"
        )),
        FailReason::Store(e) => plain(format!(
            "Keychain error: secrets for {env} not loaded ({e})"
        )),
        FailReason::NoCollectionId | FailReason::Unbound => unbound_banner(env),
        FailReason::IdBoundElsewhere { bound_root } => Banner {
            text: format!(
                "This collection's id is bound to {}: secrets not loaded",
                bound_root.display()
            ),
            action: Some(BannerAction::SameCollectionMoved),
        },
    }
}

// ---- the load job ----

/// The result of [`load_job`].
pub struct LoadResult {
    pub loaded: Result<LoadedEnvironment, String>,
    pub can_share: bool,
    pub secrets_file: bool,
}

impl LoadResult {
    /// The state this result puts the collection's environment in.
    pub fn state(&self) -> EnvState {
        match &self.loaded {
            Ok(l) => EnvState::Loaded {
                status: l.status.clone(),
                unresolved: l.unresolved.clone(),
                can_share: self.can_share,
                secrets_file: self.secrets_file,
            },
            Err(e) => EnvState::Error(e.clone()),
        }
    }
}

fn load_err(e: impl std::fmt::Display) -> LoadResult {
    LoadResult {
        loaded: Err(e.to_string()),
        can_share: false,
        secrets_file: false,
    }
}

/// Load `name` through the fail-closed loader with the collection's own id (read, never
/// written). Blocking: runs on a background task.
pub fn load_job(
    root: &Path,
    name: &str,
    data: Option<&DataDir>,
    stores: &dyn SecretStores,
) -> LoadResult {
    // Without a data dir nothing can be bound, so a store-mode environment loads as unbound.
    let none;
    let data = match data {
        Some(d) => d,
        None => {
            none = DataDir::at(std::env::temp_dir().join("wisp-no-data-dir"));
            &none
        }
    };
    let id = match read_collection_id(root) {
        Ok(id) => id,
        Err(e) => return load_err(e),
    };
    let ctx = VaultCtx::new(data.clone());
    let loaded = match load_environment_with(root, name, id.as_ref(), stores, &ctx) {
        Ok(l) => l,
        Err(e) => return load_err(e),
    };
    let can_share = match (&loaded.status, &id) {
        (LoadStatus::Store { .. }, Some(id)) => bound_roots(data, id)
            .map(|roots| {
                let canon = std::fs::canonicalize(root).ok();
                roots
                    .iter()
                    .any(|r| r == root || Some(r) == canon.as_ref())
            })
            .unwrap_or(false),
        _ => false,
    };
    let secrets_file = root
        .join("environments")
        .join(format!("{name}.secrets.toml"))
        .exists();
    LoadResult {
        loaded: Ok(loaded),
        can_share,
        secrets_file,
    }
}

// ---- the panel ----

/// What the Environment secrets panel shows and offers. Never holds a value.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PanelModel {
    pub env: Option<String>,
    pub mode: String,
    /// Secret names, resolved or not.
    pub names: Vec<String>,
    pub move_to_keychain: bool,
    pub move_to_file: bool,
    pub set_and_delete: bool,
    pub import: bool,
    pub share: bool,
}

pub fn panel_model(env: Option<&Environment>, status: Option<&EnvStatus>) -> PanelModel {
    let (Some(env), Some(status)) = (env, status) else {
        return PanelModel {
            mode: "no environment selected".into(),
            ..PanelModel::default()
        };
    };
    let mut names: Vec<String> = env.secrets.iter().map(|s| s.name.clone()).collect();
    let (mut keychain_ops, mut file_env, mut share, mut file_present) = (false, false, false, false);
    if let EnvState::Loaded {
        status: s,
        unresolved,
        can_share,
        secrets_file,
    } = &status.state
    {
        names.extend(unresolved.iter().cloned());
        file_env = matches!(s, LoadStatus::File);
        keychain_ops = matches!(
            s,
            LoadStatus::Store { .. }
                | LoadStatus::Failed {
                    reason: FailReason::Unbound | FailReason::NoCollectionId
                }
        );
        share = *can_share && matches!(s, LoadStatus::Store { .. });
        file_present = *secrets_file;
    }
    names.sort();
    names.dedup();
    PanelModel {
        env: Some(env.name.clone()),
        mode: status.mode_label().to_string(),
        names,
        move_to_keychain: file_env,
        move_to_file: keychain_ops,
        set_and_delete: keychain_ops,
        import: keychain_ops && file_present,
        share,
    }
}

// ---- actions ----

/// What the user asked for. Each runs only after the confirmation its prompt shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvAction {
    /// Bind this collection to its root, then retry `retry`.
    Bind { retry: Option<Box<EnvAction>> },
    MoveToKeychain,
    SetValue { var: String, value: SecretValue },
    DeleteValue { var: String },
    MoveToFile { allow_drop: bool },
    ImportFile,
    /// Look up the bound roots for the rebind confirmation.
    PrepareRebind,
    Rebind,
    BindAdditional,
    NewId,
    Share { new_root: PathBuf },
}

/// A confirmation: the buttons, and what each one runs (`None` changes nothing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSpec {
    pub root: PathBuf,
    pub env: String,
    pub title: String,
    pub detail: String,
    pub buttons: Vec<String>,
    pub on: Vec<Option<EnvAction>>,
}

impl PromptSpec {
    fn yes_cancel(root: &Path, env: &str, title: String, detail: String, yes: &str, a: EnvAction) -> Self {
        Self {
            root: root.to_path_buf(),
            env: env.to_string(),
            title,
            detail,
            buttons: vec![yes.to_string(), "Cancel".to_string()],
            on: vec![Some(a), None],
        }
    }

    /// The action behind button `i`, if any.
    pub fn answer(&self, i: usize) -> Option<EnvAction> {
        self.on.get(i).cloned().flatten()
    }
}

fn paths(roots: &[PathBuf]) -> String {
    roots
        .iter()
        .map(|r| r.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The confirmation to show before running `action`, if it has one.
pub fn first_prompt(action: &EnvAction, env: &str, root: &Path) -> Option<PromptSpec> {
    let p = |title: String, detail: String, yes: &str| {
        PromptSpec::yes_cancel(root, env, title, detail, yes, action.clone())
    };
    match action {
        EnvAction::Bind { .. } => Some(bind_prompt(root, env, action.clone())),
        EnvAction::MoveToKeychain => Some(p(
            format!("Move {env} secrets to the keychain?"),
            format!(
                "The values in {env}.secrets.toml are stored in the keychain and the file is deleted."
            ),
            "Move to keychain",
        )),
        EnvAction::MoveToFile { .. } => Some(p(
            format!("Move {env} secrets to a file?"),
            format!(
                "The values are written to {env}.secrets.toml in plain text and removed from the keychain."
            ),
            "Move to file",
        )),
        EnvAction::ImportFile => Some(p(
            format!("Import {env}.secrets.toml?"),
            format!("Its values are stored in the keychain and the file is deleted."),
            "Import",
        )),
        EnvAction::DeleteValue { var } => Some(p(
            format!("Delete {var}?"),
            format!("The value of {var} is removed from the keychain for {env}."),
            "Delete",
        )),
        EnvAction::Share { new_root } => Some(p(
            "Share keychain secrets with another folder?".to_string(),
            format!(
                "Share this collection's keychain secrets from {} with {}?",
                root.display(),
                new_root.display()
            ),
            "Share",
        )),
        _ => None,
    }
}

/// "Use keychain secrets for this collection at <path>?", retrying `retry` after binding when it
/// is not the bare banner action.
pub fn bind_prompt(root: &Path, env: &str, retry: EnvAction) -> PromptSpec {
    let retry = match retry {
        EnvAction::Bind { retry } => retry,
        other => Some(Box::new(other)),
    };
    PromptSpec::yes_cancel(
        root,
        env,
        format!("Use keychain secrets for this collection at {}?", root.display()),
        "Secrets are kept in the keychain under this collection's id.".to_string(),
        "Use keychain secrets",
        EnvAction::Bind { retry },
    )
}

fn drop_prompt(root: &Path, env: &str, names: &[String]) -> PromptSpec {
    PromptSpec::yes_cancel(
        root,
        env,
        "Some secrets have no value and will be dropped".to_string(),
        format!(
            "These names have no value in the keychain and will be dropped: {}",
            names.join(", ")
        ),
        "Move and drop them",
        EnvAction::MoveToFile { allow_drop: true },
    )
}

fn rebind_prompt(root: &Path, env: &str, bound: &[PathBuf]) -> PromptSpec {
    PromptSpec::yes_cancel(
        root,
        env,
        "Is this the same collection, moved?".to_string(),
        format!(
            "Its keychain secrets are bound to {}. Move the binding to {}?",
            paths(bound),
            root.display()
        ),
        "Move the binding here",
        EnvAction::Rebind,
    )
}

fn copy_prompt(root: &Path, env: &str, bound: &[PathBuf]) -> PromptSpec {
    let verb = if bound.len() == 1 { "still exists" } else { "still exist" };
    PromptSpec {
        root: root.to_path_buf(),
        env: env.to_string(),
        title: "This looks like a copy".to_string(),
        detail: format!("{} {verb} with this id.", paths(bound)),
        buttons: vec![
            "Use the same keychain secrets here (another checkout of this collection)".into(),
            "Give this copy its own id".into(),
            "Cancel".into(),
        ],
        on: vec![
            Some(EnvAction::BindAdditional),
            Some(EnvAction::NewId),
            None,
        ],
    }
}

/// What running an action produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It worked; the notice names no value.
    Done(String),
    /// It did not; the notice says why. State is unchanged.
    Notice(String),
    /// The id is unbound: confirm, bind, then retry.
    NeedBind,
    ConfirmDrop(Vec<String>),
    ConfirmRebind(Vec<PathBuf>),
    LooksLikeCopy(Vec<PathBuf>),
}

/// The confirmation an outcome calls for; `action` is what was run.
pub fn prompt_for(outcome: &Outcome, action: &EnvAction, env: &str, root: &Path) -> Option<PromptSpec> {
    match outcome {
        Outcome::NeedBind => {
            let retry = match action {
                EnvAction::Bind { retry } => retry.as_deref().cloned(),
                a => Some(a.clone()),
            };
            Some(bind_prompt(
                root,
                env,
                retry.map_or(EnvAction::Bind { retry: None }, |r| r),
            ))
        }
        Outcome::ConfirmDrop(names) => Some(drop_prompt(root, env, names)),
        Outcome::ConfirmRebind(bound) => Some(rebind_prompt(root, env, bound)),
        Outcome::LooksLikeCopy(bound) => Some(copy_prompt(root, env, bound)),
        Outcome::Done(_) | Outcome::Notice(_) => None,
    }
}

/// Where an action runs.
pub struct OpEnv<'a> {
    pub root: &'a Path,
    pub env: &'a str,
    pub data: Option<&'a DataDir>,
    pub stores: &'a dyn SecretStores,
}

fn failed(prefix: &str, e: impl std::fmt::Display) -> Outcome {
    Outcome::Notice(format!("{prefix}: {e}"))
}

/// A failure to save an environment, as a notice. A non-file-mode environment refusing secrets is
/// a bug in the caller: it is shown and never retried through the file.
#[allow(dead_code)]
pub fn save_failure_notice(e: &StorageError) -> String {
    match e {
        StorageError::SecretsNeedStore { .. } => {
            format!("Not saved (bug: secrets for a keychain environment must be set one at a time): {e}")
        }
        _ => format!("Not saved: {e}"),
    }
}

fn too_large_notice(bytes: usize) -> String {
    format!("Not stored: value is {bytes} B, over the {KEYCHAIN_LIMIT_TEXT} keychain limit")
}

fn bound_elsewhere_notice(data: &DataDir, id: &CollectionId, first: &Path) -> String {
    let roots = bound_roots(data, id).unwrap_or_default();
    let list = if roots.is_empty() {
        first.display().to_string()
    } else {
        paths(&roots)
    };
    format!("This collection's id is bound to {list}: nothing was changed")
}

/// Map a [`SecretsError`] from a store operation. `Unbound` is `None`: the caller asks to bind.
fn op_error(
    prefix: &str,
    e: &SecretsError,
    data: &DataDir,
    id: &CollectionId,
) -> Option<Outcome> {
    Some(match e {
        SecretsError::Unbound => return None,
        SecretsError::IdBoundElsewhere { bound_root } => {
            Outcome::Notice(bound_elsewhere_notice(data, id, bound_root))
        }
        other => failed(prefix, other),
    })
}

/// Run `action` against `e`. Blocking; the workspace calls it off the UI thread. A keychain
/// action never runs before the id is bound; an unbound id returns [`Outcome::NeedBind`] with
/// nothing written.
pub fn run(action: &EnvAction, e: &OpEnv) -> Outcome {
    let Some(data) = e.data else {
        return Outcome::Notice("The data directory is unknown: keychain secrets are unavailable".into());
    };
    let ctx = VaultCtx::new(data.clone());
    let store = e.stores.get(StoreKind::Keychain);
    let id = || read_collection_id(e.root);
    match action {
        EnvAction::Bind { retry } => {
            let id = match crate::workspace::resolve_collection_id(e.root) {
                Ok(id) => id,
                Err(r) => return failed("Not enabled", r),
            };
            match bind(data, &id, e.root) {
                Ok(()) => match retry {
                    Some(r) => run(r, e),
                    None => Outcome::Done(format!(
                        "Keychain secrets enabled for this collection at {}",
                        e.root.display()
                    )),
                },
                Err(err) => {
                    op_error("Not enabled", &err, data, &id).unwrap_or_else(|| failed("Not enabled", err))
                }
            }
        }
        EnvAction::MoveToKeychain => {
            let (Some(store), id) = (store, id()) else {
                return no_keychain();
            };
            let id = match id {
                Ok(Some(id)) => id,
                Ok(None) => return Outcome::NeedBind,
                Err(err) => return failed("Not moved to keychain", err),
            };
            match migrate_to_store(e.root, e.env, &id, StoreKind::Keychain, store, &ctx) {
                Ok(leftover) => Outcome::Done(match leftover {
                    None => format!("Moved {} secrets to the keychain", e.env),
                    Some(w) => format!(
                        "Moved {} secrets to the keychain, but {} could not be deleted",
                        e.env,
                        w.path.display()
                    ),
                }),
                Err(SecretsError::Store(SecretStoreError::TooLarge)) => {
                    Outcome::Notice(oversized_notice(e, &id, data))
                }
                Err(err) => op_error("Not moved to keychain", &err, data, &id)
                    .unwrap_or(Outcome::NeedBind),
            }
        }
        EnvAction::SetValue { var, value } => {
            let bytes = value.expose().len();
            if bytes > keychain::MAX_SECRET_BYTES {
                return Outcome::Notice(too_large_notice(bytes));
            }
            if var.is_empty() {
                return Outcome::Notice("Not stored: enter a variable name".into());
            }
            let (Some(store), id) = (store, id()) else {
                return no_keychain();
            };
            let id = match id {
                Ok(Some(id)) => id,
                Ok(None) => return Outcome::NeedBind,
                Err(err) => return failed("Not stored", err),
            };
            match set_secret(e.root, e.env, &id, store, var, value, &ctx) {
                Ok(()) => Outcome::Done(format!("Set {var} for {}", e.env)),
                Err(SecretsError::Store(SecretStoreError::TooLarge)) => {
                    Outcome::Notice(too_large_notice(bytes))
                }
                Err(err) => op_error("Not stored", &err, data, &id).unwrap_or(Outcome::NeedBind),
            }
        }
        EnvAction::DeleteValue { var } => {
            let (Some(store), id) = (store, id()) else {
                return no_keychain();
            };
            let id = match id {
                Ok(Some(id)) => id,
                Ok(None) => return Outcome::NeedBind,
                Err(err) => return failed("Not deleted", err),
            };
            match delete_secret(e.root, e.env, &id, store, var, &ctx) {
                Ok(()) => Outcome::Done(format!("Deleted {var} from {}", e.env)),
                Err(err) => op_error("Not deleted", &err, data, &id).unwrap_or(Outcome::NeedBind),
            }
        }
        EnvAction::MoveToFile { allow_drop } => {
            let (Some(store), id) = (store, id()) else {
                return no_keychain();
            };
            let id = match id {
                Ok(Some(id)) => id,
                Ok(None) => return Outcome::NeedBind,
                Err(err) => return failed("Not moved", err),
            };
            match migrate_to_file(e.root, e.env, &id, store, *allow_drop, &ctx) {
                Ok(warnings) if warnings.is_empty() => {
                    Outcome::Done(format!("Moved {} secrets to {}.secrets.toml", e.env, e.env))
                }
                Ok(warnings) => Outcome::Done(format!(
                    "Moved {} secrets to {}.secrets.toml; the keychain entries for {} could not be deleted",
                    e.env,
                    e.env,
                    warnings
                        .iter()
                        .map(|w| w.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
                Err(SecretsError::UnresolvedNames { names }) => Outcome::ConfirmDrop(names),
                Err(err) => op_error("Not moved", &err, data, &id).unwrap_or(Outcome::NeedBind),
            }
        }
        EnvAction::ImportFile => {
            let (Some(store), id) = (store, id()) else {
                return no_keychain();
            };
            let id = match id {
                Ok(Some(id)) => id,
                Ok(None) => return Outcome::NeedBind,
                Err(err) => return failed("Not imported", err),
            };
            let before = secret_names(e, &id, &ctx);
            match import_secrets_file(e.root, e.env, &id, store, &ctx) {
                Ok(leftover) => {
                    let after = secret_names(e, &id, &ctx);
                    let new: Vec<&String> = after.iter().filter(|n| !before.contains(n)).collect();
                    let mut text = if new.is_empty() {
                        format!("Imported the secrets file for {}", e.env)
                    } else {
                        format!(
                            "Imported the secrets file for {}: {}",
                            e.env,
                            new.iter().map(|n| n.as_str()).collect::<Vec<_>>().join(", ")
                        )
                    };
                    if let Some(w) = leftover {
                        text.push_str(&format!("; {} could not be deleted", w.path.display()));
                    }
                    Outcome::Done(text)
                }
                Err(err) => op_error("Not imported", &err, data, &id).unwrap_or(Outcome::NeedBind),
            }
        }
        EnvAction::PrepareRebind => {
            let id = match id() {
                Ok(Some(id)) => id,
                Ok(None) => return Outcome::Notice("This collection has no id".into()),
                Err(err) => return failed("Not rebound", err),
            };
            match bound_roots(data, &id) {
                Ok(roots) => Outcome::ConfirmRebind(roots),
                Err(err) => failed("Not rebound", err),
            }
        }
        EnvAction::Rebind => {
            let id = match id() {
                Ok(Some(id)) => id,
                Ok(None) => return Outcome::Notice("This collection has no id".into()),
                Err(err) => return failed("Not rebound", err),
            };
            match rebind(data, &id, e.root) {
                Ok(()) => Outcome::Done(format!(
                    "The collection's keychain secrets now follow {}",
                    e.root.display()
                )),
                Err(SecretsError::LooksLikeCopy { bound_root }) => {
                    let roots = bound_roots(data, &id).unwrap_or_else(|_| vec![bound_root]);
                    Outcome::LooksLikeCopy(roots)
                }
                Err(err) => failed("Not rebound", err),
            }
        }
        EnvAction::BindAdditional => {
            let id = match id() {
                Ok(Some(id)) => id,
                Ok(None) => return Outcome::Notice("This collection has no id".into()),
                Err(err) => return failed("Not bound", err),
            };
            match bind_additional_checkout(data, &id, e.root) {
                Ok(()) => Outcome::Done(format!(
                    "{} now uses this collection's keychain secrets",
                    e.root.display()
                )),
                Err(SecretsError::UnverifiedCheckout { bound_roots }) => Outcome::Notice(format!(
                    "This folder can't be verified as another checkout of this collection, so it was not bound. \
                     Bound folders: {}. Open one of them and use \"Share keychain secrets with another folder…\".",
                    paths(&bound_roots)
                )),
                Err(err) => failed("Not bound", err),
            }
        }
        EnvAction::NewId => match crate::workspace::assign_collection_id(e.root) {
            Ok(_) => Outcome::Done(
                "This copy now has its own collection id; keychain secrets are not enabled for it".into(),
            ),
            Err(r) => failed("Not changed", r),
        },
        EnvAction::Share { new_root } => {
            let id = match id() {
                Ok(Some(id)) => id,
                Ok(None) => return Outcome::Notice("Not shared: this collection has no id".into()),
                Err(err) => return failed("Not shared", err),
            };
            match share_with_folder(data, &id, e.root, new_root) {
                Ok(()) => Outcome::Done(format!(
                    "Shared keychain secrets from {} with {}",
                    e.root.display(),
                    new_root.display()
                )),
                Err(err) => failed("Not shared", err),
            }
        }
    }
}

fn no_keychain() -> Outcome {
    Outcome::Notice("The keychain is not available".into())
}

/// Every secret name of the environment, resolved or not.
fn secret_names(e: &OpEnv, id: &CollectionId, ctx: &VaultCtx) -> Vec<String> {
    match load_environment_with(e.root, e.env, Some(id), e.stores, ctx) {
        Ok(l) => {
            let mut n: Vec<String> = l.env.secrets.iter().map(|s| s.name.clone()).collect();
            n.extend(l.unresolved);
            n
        }
        Err(_) => Vec::new(),
    }
}

/// "Move to keychain" hit the size cap: name the variable and its size. The environment is still
/// in file mode, so the loader returns the file's values; only their sizes are read.
fn oversized_notice(e: &OpEnv, id: &CollectionId, data: &DataDir) -> String {
    let ctx = VaultCtx::new(data.clone());
    let big: Vec<String> = load_environment_with(e.root, e.env, Some(id), e.stores, &ctx)
        .map(|l| {
            l.env
                .secrets
                .iter()
                .filter(|s| s.value.len() > keychain::MAX_SECRET_BYTES)
                .map(|s| format!("{} ({} B)", s.name, s.value.len()))
                .collect()
        })
        .unwrap_or_default();
    format!(
        "Not moved: {} over the {KEYCHAIN_LIMIT_TEXT} keychain limit; nothing was changed",
        if big.is_empty() {
            "a value is".to_string()
        } else {
            big.join(", ")
        }
    )
}

