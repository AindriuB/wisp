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
                roots.iter().any(|r| r == root || Some(r) == canon.as_ref())
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
    let (mut keychain_ops, mut file_env, mut share, mut file_present) =
        (false, false, false, false);
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
    Bind {
        retry: Option<Box<EnvAction>>,
    },
    MoveToKeychain,
    SetValue {
        var: String,
        value: SecretValue,
    },
    DeleteValue {
        var: String,
    },
    MoveToFile {
        allow_drop: bool,
    },
    ImportFile,
    /// Look up the bound roots for the rebind confirmation.
    PrepareRebind,
    Rebind,
    BindAdditional,
    NewId,
    Share {
        new_root: PathBuf,
    },
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
    fn yes_cancel(
        root: &Path,
        env: &str,
        title: String,
        detail: String,
        yes: &str,
        a: EnvAction,
    ) -> Self {
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
            "Its values are stored in the keychain and the file is deleted.".to_string(),
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
        format!(
            "Use keychain secrets for this collection at {}?",
            root.display()
        ),
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
    let verb = if bound.len() == 1 {
        "still exists"
    } else {
        "still exist"
    };
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
pub fn prompt_for(
    outcome: &Outcome,
    action: &EnvAction,
    env: &str,
    root: &Path,
) -> Option<PromptSpec> {
    match outcome {
        Outcome::NeedBind => {
            let retry = match action {
                EnvAction::Bind { retry } => retry.as_deref().cloned(),
                a => Some(a.clone()),
            };
            Some(bind_prompt(
                root,
                env,
                retry.unwrap_or(EnvAction::Bind { retry: None }),
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
            format!(
                "Not saved (bug: secrets for a keychain environment must be set one at a time): {e}"
            )
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
fn op_error(prefix: &str, e: &SecretsError, data: &DataDir, id: &CollectionId) -> Option<Outcome> {
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
        return Outcome::Notice(
            "The data directory is unknown: keychain secrets are unavailable".into(),
        );
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
                Err(err) => op_error("Not enabled", &err, data, &id)
                    .unwrap_or_else(|| failed("Not enabled", err)),
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
                Err(err) => {
                    op_error("Not moved to keychain", &err, data, &id).unwrap_or(Outcome::NeedBind)
                }
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
                            new.iter()
                                .map(|n| n.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
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
                "This copy now has its own collection id; keychain secrets are not enabled for it"
                    .into(),
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

#[cfg(test)]
mod keychain_tests {
    use super::*;
    use wisp_storage::collection::assign_new_collection_id;
    use wisp_storage::vault::{MemoryStore, secret_account};

    const SENTINEL: &str = "s3ntinel-token-xyz";
    const ID: &str = "abcdef0123456789";

    fn data() -> (tempfile::TempDir, DataDir) {
        let d = tempfile::tempdir().unwrap();
        let data = DataDir::at(d.path().join("data"));
        (d, data)
    }

    /// A collection root with `id` (when given) and a keychain-mode `dev` environment.
    fn coll(parent: &Path, name: &str, id: Option<&str>) -> PathBuf {
        let root = parent.join(name);
        std::fs::create_dir_all(root.join("environments")).unwrap();
        let head = match id {
            Some(id) => format!("format = 1\nid = \"{id}\"\n"),
            None => "format = 1\n".to_string(),
        };
        std::fs::write(root.join("collection.toml"), head).unwrap();
        std::fs::write(
            root.join("environments/dev.toml"),
            "secrets = \"keychain\"\nsecret_names = [\"token\"]\n[vars]\nhost = \"h\"\n",
        )
        .unwrap();
        root
    }

    fn file_coll(parent: &Path, name: &str, id: &str) -> PathBuf {
        let root = coll(parent, name, Some(id));
        std::fs::write(root.join("environments/dev.toml"), "[vars]\nhost = \"h\"\n").unwrap();
        std::fs::write(
            root.join("environments/dev.secrets.toml"),
            "[vars]\ntoken = \"t1\"\n",
        )
        .unwrap();
        root
    }

    fn cid(s: &str) -> CollectionId {
        CollectionId::parse(s).unwrap()
    }

    fn status(root: &Path, data: &DataDir, store: &MemoryStore) -> LoadStatus {
        load_job(root, "dev", Some(data), store)
            .loaded
            .unwrap()
            .status
    }

    fn env_op<'a>(root: &'a Path, data: &'a DataDir, store: &'a MemoryStore) -> OpEnv<'a> {
        OpEnv {
            root,
            env: "dev",
            data: Some(data),
            stores: store,
        }
    }

    fn set(var: &str, value: &str) -> EnvAction {
        EnvAction::SetValue {
            var: var.into(),
            value: SecretValue::new(value),
        }
    }

    fn key(env: Option<&str>, col: Option<&str>) -> TokenKey {
        TokenKey {
            grant: Grant::AuthCode,
            token_url: "https://idp/t".into(),
            client_id: "cid".into(),
            scope: "r".into(),
            collection: col.map(str::to_string),
            environment: env.map(str::to_string),
        }
    }

    fn files_under(dir: &Path) -> usize {
        std::fs::read_dir(dir).map(|d| d.count()).unwrap_or(0)
    }

    #[test]
    fn keychain_service_is_the_variable_when_set_and_non_empty() {
        assert_eq!(service_name(None), "wisp");
        assert_eq!(service_name(Some("")), "wisp");
        assert_eq!(service_name(Some("wisp-test-1")), "wisp-test-1");
    }

    #[test]
    fn keychain_refresh_persists_only_for_a_keychain_status() {
        let st = |state| EnvStatus {
            env: "dev".into(),
            state,
        };
        let loaded = |status| EnvState::Loaded {
            status,
            unresolved: vec![],
            can_share: false,
            secrets_file: false,
        };
        let kc = st(loaded(LoadStatus::Store {
            kind: StoreKind::Keychain,
            file_ignored: false,
        }));
        assert!(kc.persist_refresh("dev"));
        assert!(!kc.persist_refresh("prod"));
        assert!(!st(loaded(LoadStatus::File)).persist_refresh("dev"));
        assert!(
            !st(loaded(LoadStatus::FellBackToFile {
                reason: UnavailableReason::NoService
            }))
            .persist_refresh("dev")
        );
        assert!(
            !st(loaded(LoadStatus::Failed {
                reason: FailReason::Unbound
            }))
            .persist_refresh("dev")
        );
        assert!(!st(EnvState::Loading).persist_refresh("dev"));
    }

    #[test]
    fn keychain_refresh_store_round_trips_and_refuses_incomplete_keys() {
        let m = Arc::new(MemoryStore::new());
        let a = KeychainRefreshStore::new(m.clone());
        let k = key(Some("dev"), Some(ID));
        a.save(&k, SENTINEL).unwrap();
        assert_eq!(a.load(&k).unwrap().as_deref(), Some(SENTINEL));
        assert_eq!(m.accounts().len(), 1);
        a.forget(&k).unwrap();
        assert_eq!(a.load(&k).unwrap(), None);
        for bad in [key(None, Some(ID)), key(Some("dev"), None)] {
            assert!(a.save(&bad, SENTINEL).is_err());
            assert!(a.load(&bad).is_err());
        }
        assert!(m.accounts().is_empty());
    }

    #[test]
    fn keychain_refresh_save_failure_is_an_error_without_the_token() {
        let (dir, _) = data();
        for err in [
            SecretStoreError::Denied,
            SecretStoreError::Locked,
            SecretStoreError::Other { code: Some(7) },
            SecretStoreError::Unavailable {
                reason: UnavailableReason::TimedOut,
            },
        ] {
            let m = Arc::new(MemoryStore::new());
            let a = KeychainRefreshStore::new(m.clone());
            m.fail_next(err);
            let e = a.save(&key(Some("dev"), Some(ID)), SENTINEL).unwrap_err();
            assert!(!e.to_string().contains(SENTINEL));
            assert!(a.take_banners().is_empty());
            assert_eq!(a.load(&key(Some("dev"), Some(ID))).unwrap(), None);
        }
        assert_eq!(files_under(dir.path()), 0);
    }

    #[test]
    fn keychain_too_large_refresh_token_lives_in_the_session_only() {
        let (dir, _) = data();
        let m = Arc::new(MemoryStore::new());
        let a = KeychainRefreshStore::new(m.clone());
        let k = key(Some("dev"), Some(ID));
        m.fail_next(SecretStoreError::TooLarge);
        a.save(&k, SENTINEL).unwrap();
        assert_eq!(a.load(&k).unwrap().as_deref(), Some(SENTINEL));
        assert!(m.accounts().is_empty());
        let banners = a.take_banners();
        assert_eq!(banners.len(), 1);
        assert!(banners[0].contains("dev") && banners[0].contains("kept for this session only"));
        assert!(!banners[0].contains(SENTINEL));
        // Once per account.
        m.fail_next(SecretStoreError::TooLarge);
        a.save(&k, SENTINEL).unwrap();
        assert!(a.take_banners().is_empty());
        // A new adapter has nothing, and nothing reached a file.
        let b = KeychainRefreshStore::new(m.clone());
        assert_eq!(b.load(&k).unwrap(), None);
        assert_eq!(files_under(dir.path()), 0);
        // A later save that fits removes the session entry.
        a.save(&k, "small").unwrap();
        assert_eq!(b.load(&k).unwrap().as_deref(), Some("small"));
        a.forget(&k).unwrap();
        assert_eq!(a.load(&k).unwrap(), None);
    }

    #[test]
    fn keychain_banners_for_each_status() {
        let b = |status| {
            EnvStatus {
                env: "dev".into(),
                state: EnvState::Loaded {
                    status,
                    unresolved: vec![],
                    can_share: false,
                    secrets_file: false,
                },
            }
            .banners()
        };
        let fail = |reason| b(LoadStatus::Failed { reason });
        assert_eq!(
            b(LoadStatus::FellBackToFile {
                reason: UnavailableReason::NoService
            })[0]
                .text,
            "Keychain unavailable (secret store service not running): using dev.secrets.toml"
        );
        assert!(
            fail(FailReason::Store(SecretStoreError::Denied))[0]
                .text
                .starts_with("Keychain error: secrets for dev not loaded (")
        );
        assert_eq!(
            fail(FailReason::Store(SecretStoreError::Unavailable {
                reason: UnavailableReason::TimedOut
            }))[0]
                .text,
            "Keychain did not answer: secrets for dev not loaded"
        );
        assert_eq!(
            fail(FailReason::UnknownStoreKind("kp".into()))[0].text,
            "Unknown secret store \"kp\" for dev: secrets not loaded"
        );
        assert!(
            b(LoadStatus::Store {
                kind: StoreKind::Keychain,
                file_ignored: true
            })[0]
                .text
                .contains("ignored")
        );
        assert!(
            b(LoadStatus::Store {
                kind: StoreKind::Keychain,
                file_ignored: false
            })
            .is_empty()
        );
    }

    #[test]
    fn keychain_bind_and_rebind_banners_are_distinct() {
        let unbound = EnvStatus {
            env: "dev".into(),
            state: EnvState::Loaded {
                status: LoadStatus::Failed {
                    reason: FailReason::Unbound,
                },
                unresolved: vec![],
                can_share: false,
                secrets_file: false,
            },
        };
        let b = &unbound.banners()[0];
        assert_eq!(
            b.text,
            "Keychain secrets for dev are not enabled for this collection"
        );
        assert_eq!(b.action, Some(BannerAction::UseKeychain));
        let elsewhere = EnvStatus {
            state: EnvState::Loaded {
                status: LoadStatus::Failed {
                    reason: FailReason::IdBoundElsewhere {
                        bound_root: PathBuf::from("/x/y"),
                    },
                },
                unresolved: vec![],
                can_share: false,
                secrets_file: false,
            },
            ..unbound
        };
        let b = &elsewhere.banners()[0];
        assert!(b.text.starts_with("This collection's id is bound to "));
        assert!(b.text.ends_with(": secrets not loaded"));
        assert_eq!(b.action, Some(BannerAction::SameCollectionMoved));
    }

    #[test]
    fn keychain_missing_or_wrong_marker_shows_the_bind_banner() {
        let (d, data) = data();
        let m = MemoryStore::new();
        let root = coll(d.path(), "a", Some(ID));
        bind(&data, &cid(ID), &root).unwrap();
        assert!(matches!(status(&root, &data, &m), LoadStatus::Store { .. }));
        let marker = root.join("environments/.wisp-binding");
        std::fs::write(&marker, "wrong\n").unwrap();
        let unbound = LoadStatus::Failed {
            reason: FailReason::Unbound,
        };
        assert_eq!(status(&root, &data, &m), unbound);
        std::fs::remove_file(&marker).unwrap();
        assert_eq!(status(&root, &data, &m), unbound);
    }

    #[test]
    fn keychain_two_collections_load_under_their_own_ids() {
        let (d, data) = data();
        let m = MemoryStore::new();
        let a = coll(d.path(), "a", Some(ID));
        let b = coll(d.path(), "b", Some("fedcba9876543210"));
        bind(&data, &cid(ID), &a).unwrap();
        m.set(
            &secret_account(&cid(ID), "dev", "token"),
            &SecretValue::new("A"),
        )
        .unwrap();
        let la = load_job(&a, "dev", Some(&data), &m).loaded.unwrap();
        let lb = load_job(&b, "dev", Some(&data), &m).loaded.unwrap();
        assert!(matches!(la.status, LoadStatus::Store { .. }));
        assert_eq!(la.env.secrets.len(), 1);
        // `b` is not bound and its id has no entries: nothing of `a` shows on it.
        assert_eq!(
            lb.status,
            LoadStatus::Failed {
                reason: FailReason::Unbound
            }
        );
        assert!(lb.env.secrets.is_empty());
    }

    #[test]
    fn keychain_set_value_on_an_unbound_id_cancel_changes_nothing() {
        let (d, data) = data();
        let m = MemoryStore::new();
        let root = coll(d.path(), "a", Some(ID));
        let out = run(&set("token", SENTINEL), &env_op(&root, &data, &m));
        assert_eq!(out, Outcome::NeedBind);
        let spec = prompt_for(&out, &set("token", SENTINEL), "dev", &root).unwrap();
        assert!(
            spec.title
                .starts_with("Use keychain secrets for this collection at ")
        );
        // Cancel is the last button and runs nothing.
        assert_eq!(spec.answer(spec.buttons.len() - 1), None);
        assert!(m.accounts().is_empty());
        assert_eq!(
            status(&root, &data, &m),
            LoadStatus::Failed {
                reason: FailReason::Unbound
            }
        );
    }

    #[test]
    fn keychain_bind_confirm_then_the_value_is_stored_and_resolves() {
        let (d, data) = data();
        let m = MemoryStore::new();
        let root = coll(d.path(), "a", Some(ID));
        let a = set("token", SENTINEL);
        let out = run(&a, &env_op(&root, &data, &m));
        let spec = prompt_for(&out, &a, "dev", &root).unwrap();
        let confirmed = spec.answer(0).unwrap();
        let done = run(&confirmed, &env_op(&root, &data, &m));
        assert!(matches!(&done, Outcome::Done(t) if !t.contains(SENTINEL)));
        let l = load_job(&root, "dev", Some(&data), &m).loaded.unwrap();
        assert!(matches!(l.status, LoadStatus::Store { .. }));
        assert_eq!(l.env.secrets[0].value, SENTINEL);
    }

    #[test]
    fn keychain_banner_bind_action_binds_and_the_reload_succeeds() {
        let (d, data) = data();
        let m = MemoryStore::new();
        let root = coll(d.path(), "a", None);
        // No id yet: unbound, loads as such.
        assert_eq!(
            status(&root, &data, &m),
            LoadStatus::Failed {
                reason: FailReason::NoCollectionId
            }
        );
        let spec = first_prompt(&EnvAction::Bind { retry: None }, "dev", &root).unwrap();
        assert_eq!(spec.answer(1), None);
        assert_eq!(read_collection_id(&root).unwrap(), None);
        run(&spec.answer(0).unwrap(), &env_op(&root, &data, &m));
        assert!(matches!(status(&root, &data, &m), LoadStatus::Store { .. }));
    }

    #[test]
    fn keychain_rebind_cancel_leaves_binding_and_confirm_moves_it() {
        let (d, data) = data();
        let m = MemoryStore::new();
        let old = coll(d.path(), "old", Some(ID));
        bind(&data, &cid(ID), &old).unwrap();
        let new = coll(d.path(), "new", Some(ID));
        let elsewhere = LoadStatus::Failed {
            reason: FailReason::IdBoundElsewhere {
                bound_root: std::fs::canonicalize(&old).unwrap(),
            },
        };
        // `old` still exists: the original folder is on disk, but the new one is unbound here.
        assert_eq!(status(&new, &data, &m), elsewhere);
        let out = run(&EnvAction::PrepareRebind, &env_op(&new, &data, &m));
        let Outcome::ConfirmRebind(roots) = &out else {
            panic!("{out:?}")
        };
        assert_eq!(roots.len(), 1);
        let spec = prompt_for(&out, &EnvAction::PrepareRebind, "dev", &new).unwrap();
        assert_eq!(spec.answer(1), None);
        assert_eq!(bound_roots(&data, &cid(ID)).unwrap().len(), 1);
        assert_eq!(status(&new, &data, &m), elsewhere);
        // The old folder moved away: confirm rebinds and the reload resolves.
        std::fs::remove_dir_all(&old).unwrap();
        m.set(
            &secret_account(&cid(ID), "dev", "token"),
            &SecretValue::new("v"),
        )
        .unwrap();
        let done = run(&spec.answer(0).unwrap(), &env_op(&new, &data, &m));
        assert!(matches!(done, Outcome::Done(_)), "{done:?}");
        let l = load_job(&new, "dev", Some(&data), &m).loaded.unwrap();
        assert_eq!(l.env.secrets[0].value, "v");
    }

    #[test]
    fn keychain_looks_like_copy_offers_three_ways_and_cancel_changes_nothing() {
        let (d, data) = data();
        let m = MemoryStore::new();
        let a = coll(d.path(), "a", Some(ID));
        let b = coll(d.path(), "b", Some(ID));
        bind(&data, &cid(ID), &a).unwrap();
        let out = run(&EnvAction::Rebind, &env_op(&b, &data, &m));
        let Outcome::LooksLikeCopy(roots) = &out else {
            panic!("{out:?}")
        };
        assert_eq!(roots, &bound_roots(&data, &cid(ID)).unwrap());
        let spec = prompt_for(&out, &EnvAction::Rebind, "dev", &b).unwrap();
        assert_eq!(spec.buttons.len(), 3);
        assert_eq!(spec.answer(0), Some(EnvAction::BindAdditional));
        assert_eq!(spec.answer(1), Some(EnvAction::NewId));
        assert_eq!(spec.answer(2), None);
        assert!(spec.detail.contains(&roots[0].display().to_string()));
        assert_eq!(read_collection_id(&b).unwrap(), Some(cid(ID)));
        assert_eq!(bound_roots(&data, &cid(ID)).unwrap().len(), 1);
    }

    #[test]
    fn keychain_new_id_for_a_copy_leaves_the_original_and_loads_unbound() {
        let (d, data) = data();
        let m = MemoryStore::new();
        let a = coll(d.path(), "a", Some(ID));
        let b = coll(d.path(), "b", Some(ID));
        bind(&data, &cid(ID), &a).unwrap();
        m.set(
            &secret_account(&cid(ID), "dev", "token"),
            &SecretValue::new("v"),
        )
        .unwrap();
        let out = run(&EnvAction::NewId, &env_op(&b, &data, &m));
        assert!(matches!(out, Outcome::Done(_)));
        assert_ne!(read_collection_id(&b).unwrap(), Some(cid(ID)));
        assert_eq!(
            status(&b, &data, &m),
            LoadStatus::Failed {
                reason: FailReason::Unbound
            }
        );
        assert_eq!(m.accounts().len(), 1);
        assert_eq!(bound_roots(&data, &cid(ID)).unwrap().len(), 1);
        assert!(matches!(status(&a, &data, &m), LoadStatus::Store { .. }));
    }

    fn make_worktree(main: &Path, wt: &Path, name: &str) {
        std::fs::create_dir_all(main.join(".git")).unwrap();
        let main = std::fs::canonicalize(main).unwrap();
        let wt = std::fs::canonicalize(wt).unwrap();
        let gd = main.join(".git/worktrees").join(name);
        std::fs::create_dir_all(&gd).unwrap();
        std::fs::write(gd.join("commondir"), "../..\n").unwrap();
        std::fs::write(
            gd.join("gitdir"),
            format!("{}\n", wt.join(".git").display()),
        )
        .unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", gd.display())).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn keychain_same_secrets_here_for_a_verified_worktree_and_not_an_unverified_folder() {
        let (d, data) = data();
        let m = MemoryStore::new();
        let a = coll(d.path(), "a", Some(ID));
        let w = coll(d.path(), "w", Some(ID));
        let stray = coll(d.path(), "stray", Some(ID));
        bind(&data, &cid(ID), &a).unwrap();
        m.set(
            &secret_account(&cid(ID), "dev", "token"),
            &SecretValue::new("orig"),
        )
        .unwrap();
        // Unverified: explained, every bound root listed, nothing bound.
        let out = run(&EnvAction::BindAdditional, &env_op(&stray, &data, &m));
        let Outcome::Notice(t) = &out else {
            panic!("{out:?}")
        };
        assert!(t.contains("can't be verified"));
        assert!(
            t.contains(
                &bound_roots(&data, &cid(ID)).unwrap()[0]
                    .display()
                    .to_string()
            )
        );
        assert_eq!(bound_roots(&data, &cid(ID)).unwrap().len(), 1);
        assert!(matches!(
            status(&stray, &data, &m),
            LoadStatus::Failed {
                reason: FailReason::IdBoundElsewhere { .. }
            }
        ));
        // Verified worktree: both roots bound, id unchanged, the original's value resolves.
        make_worktree(&a, &w, "w1");
        let out = run(&EnvAction::BindAdditional, &env_op(&w, &data, &m));
        assert!(matches!(out, Outcome::Done(_)), "{out:?}");
        assert_eq!(bound_roots(&data, &cid(ID)).unwrap().len(), 2);
        assert_eq!(read_collection_id(&w).unwrap(), Some(cid(ID)));
        let l = load_job(&w, "dev", Some(&data), &m).loaded.unwrap();
        assert_eq!(l.env.secrets[0].value, "orig");
    }

    #[test]
    fn keychain_share_confirm_binds_the_new_folder_and_cancel_does_not() {
        let (d, data) = data();
        let m = MemoryStore::new();
        let a = coll(d.path(), "a", Some(ID));
        let n = coll(d.path(), "n", Some(ID));
        bind(&data, &cid(ID), &a).unwrap();
        m.set(
            &secret_account(&cid(ID), "dev", "token"),
            &SecretValue::new("shared"),
        )
        .unwrap();
        // The panel offers it from a bound root only.
        let r = load_job(&a, "dev", Some(&data), &m);
        assert!(r.can_share);
        assert!(!load_job(&n, "dev", Some(&data), &m).can_share);
        let action = EnvAction::Share {
            new_root: n.clone(),
        };
        let spec = first_prompt(&action, "dev", &a).unwrap();
        assert!(spec.detail.contains(&a.display().to_string()));
        assert!(spec.detail.contains(&n.display().to_string()));
        assert_eq!(spec.answer(1), None);
        assert_eq!(bound_roots(&data, &cid(ID)).unwrap().len(), 1);
        let out = run(&spec.answer(0).unwrap(), &env_op(&a, &data, &m));
        assert!(matches!(&out, Outcome::Done(t) if t.contains("shared") || t.contains("Shared")));
        assert_eq!(bound_roots(&data, &cid(ID)).unwrap().len(), 2);
        let l = load_job(&n, "dev", Some(&data), &m).loaded.unwrap();
        assert_eq!(l.env.secrets[0].value, "shared");
        // Sharing from an unbound root is refused by storage too.
        let u = coll(d.path(), "u", Some("0123456789abcdef"));
        let out = run(
            &EnvAction::Share {
                new_root: a.clone(),
            },
            &env_op(&u, &data, &m),
        );
        assert!(matches!(out, Outcome::Notice(t) if t.starts_with("Not shared: ")));
    }

    #[test]
    fn keychain_import_is_offered_only_for_store_mode_with_a_secrets_file() {
        let (d, data) = data();
        let m = MemoryStore::new();
        let root = coll(d.path(), "a", Some(ID));
        bind(&data, &cid(ID), &root).unwrap();
        let model = |root: &Path| {
            let r = load_job(root, "dev", Some(&data), &m);
            let st = EnvStatus {
                env: "dev".into(),
                state: r.state(),
            };
            panel_model(Some(&r.loaded.as_ref().unwrap().env), Some(&st))
        };
        assert!(!model(&root).import);
        std::fs::write(
            root.join("environments/dev.secrets.toml"),
            "[vars]\ntoken = \"fromfile\"\nextra = \"e\"\n",
        )
        .unwrap();
        assert!(model(&root).import);
        let out = run(&EnvAction::ImportFile, &env_op(&root, &data, &m));
        let Outcome::Done(t) = &out else {
            panic!("{out:?}")
        };
        assert!(t.contains("extra") && !t.contains("fromfile"));
        assert!(!root.join("environments/dev.secrets.toml").exists());
        // File mode never offers it, even with a file.
        let f = file_coll(d.path(), "f", "11112222333344445555");
        assert!(!model(&f).import);
        assert!(model(&f).move_to_keychain);
    }

    #[test]
    fn keychain_move_to_file_drops_unresolved_names_only_after_confirm() {
        let (d, data) = data();
        let m = MemoryStore::new();
        let root = coll(d.path(), "a", Some(ID));
        bind(&data, &cid(ID), &root).unwrap();
        // `token` is listed but has no value in the store.
        let out = run(
            &EnvAction::MoveToFile { allow_drop: false },
            &env_op(&root, &data, &m),
        );
        assert_eq!(out, Outcome::ConfirmDrop(vec!["token".into()]));
        let spec = prompt_for(
            &out,
            &EnvAction::MoveToFile { allow_drop: false },
            "dev",
            &root,
        )
        .unwrap();
        assert!(spec.detail.contains("token"));
        assert_eq!(spec.answer(1), None);
        // Cancelled: still keychain mode.
        assert!(matches!(status(&root, &data, &m), LoadStatus::Store { .. }));
        let done = run(&spec.answer(0).unwrap(), &env_op(&root, &data, &m));
        assert!(matches!(done, Outcome::Done(_)), "{done:?}");
        let l = load_job(&root, "dev", Some(&data), &m).loaded.unwrap();
        assert_eq!(l.status, LoadStatus::File);
        assert!(l.env.secrets.is_empty());
    }

    #[test]
    fn keychain_too_large_values_are_never_stored() {
        let (d, data) = data();
        let m = MemoryStore::new();
        let root = coll(d.path(), "a", Some(ID));
        bind(&data, &cid(ID), &root).unwrap();
        let big = "x".repeat(keychain::MAX_SECRET_BYTES + 1);
        let out = run(&set("token", &big), &env_op(&root, &data, &m));
        assert_eq!(
            out,
            Outcome::Notice("Not stored: value is 2561 B, over the 2,560 B keychain limit".into())
        );
        assert!(m.accounts().is_empty());
        // Move to keychain aborts with nothing changed and names the variable and size.
        let f = file_coll(d.path(), "f", "11112222333344445555");
        let fid = cid("11112222333344445555");
        bind(&data, &fid, &f).unwrap();
        std::fs::write(
            f.join("environments/dev.secrets.toml"),
            format!("[vars]\ntoken = \"{big}\"\n"),
        )
        .unwrap();
        // The store refuses a value over its cap.
        m.fail_next(SecretStoreError::TooLarge);
        let out = run(&EnvAction::MoveToKeychain, &env_op(&f, &data, &m));
        let Outcome::Notice(t) = &out else {
            panic!("{out:?}")
        };
        assert!(t.contains("token (2561 B)"), "{t}");
        assert!(f.join("environments/dev.secrets.toml").exists());
        assert!(m.accounts().is_empty());
        assert_eq!(status(&f, &data, &m), LoadStatus::File);
    }

    #[cfg(unix)]
    #[test]
    fn keychain_invalid_symlinked_targets_show_notices_and_change_nothing() {
        use wisp_storage::environment::save_environment_locked;
        use wisp_storage::vault::VaultCtx;
        let (d, data) = data();
        let m = MemoryStore::new();
        // assign_new_collection_id on a symlinked collection.toml.
        let root = coll(d.path(), "a", Some(ID));
        let real = d.path().join("real.toml");
        std::fs::rename(root.join("collection.toml"), &real).unwrap();
        std::os::unix::fs::symlink(&real, root.join("collection.toml")).unwrap();
        let before = std::fs::read_to_string(&real).unwrap();
        let out = run(&EnvAction::NewId, &env_op(&root, &data, &m));
        assert!(
            matches!(&out, Outcome::Notice(t) if t.starts_with("Not changed: ")),
            "{out:?}"
        );
        assert_eq!(std::fs::read_to_string(&real).unwrap(), before);
        assert!(assign_new_collection_id(&root).is_err());
        // save_environment_locked on a symlinked environment file.
        let s = coll(d.path(), "s", Some("99998888777766665555"));
        let target = d.path().join("outside.toml");
        std::fs::write(&target, "[vars]\n").unwrap();
        std::fs::remove_file(s.join("environments/dev.toml")).unwrap();
        std::os::unix::fs::symlink(&target, s.join("environments/dev.toml")).unwrap();
        let env = Environment {
            name: "dev".into(),
            vars: vec![],
            secrets: vec![],
        };
        let ctx = VaultCtx::new(data.clone());
        let r = save_environment_locked(&s, &env, &ctx);
        if let Err(e) = r {
            let text = save_failure_notice(&e);
            assert!(text.starts_with("Not saved"));
        }
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "[vars]\n");
    }
}
