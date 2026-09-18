//! Native profile ownership. IPC arguments capture a runtime before execution.
use crate::{
    context::ServiceContext, database::DatabaseRuntime, secret_store::shared_secret_store,
};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;
use wealthfolio_core::profiles::{ProfileError, ProfileRegistry, ProfileSession, ProfileSummary};
use zeroize::Zeroizing;

pub const NATIVE_OWNER: &str = "main";
pub const PROFILE_CHANGED: &str = "profile-session-changed";

pub struct NativeProfiles {
    pub registry: Arc<ProfileRegistry>,
    active: Mutex<Option<Arc<DatabaseRuntime>>>,
    transition: tokio::sync::Mutex<()>,
    starting: AtomicBool,
    lock_epoch: AtomicU64,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileState {
    pub profiles: Vec<ProfileSummary>,
    pub pending_deletions: Vec<ProfileSummary>,
    pub session: Option<ProfileSession>,
    pub starting: bool,
}

fn portfolio_history_backfill_needed(context: &Arc<ServiceContext>) -> bool {
    let accounts = match context.account_service().get_non_archived_accounts() {
        Ok(accounts) => accounts,
        Err(err) => {
            log::error!("Failed to inspect accounts for valuation backfill: {}", err);
            return false;
        }
    };
    let account_ids: Vec<String> = accounts.into_iter().map(|account| account.id).collect();
    if account_ids.is_empty() {
        return false;
    }

    let latest = match context
        .valuation_service()
        .get_latest_valuations(&account_ids)
    {
        Ok(latest) => latest,
        Err(err) => {
            log::error!("Failed to inspect valuation history for backfill: {}", err);
            return false;
        }
    };
    let accounts_with_valuations: std::collections::HashSet<_> = latest
        .into_iter()
        .map(|valuation| valuation.account_id)
        .collect();
    let missing_ids: Vec<String> = account_ids
        .into_iter()
        .filter(|account_id| !accounts_with_valuations.contains(account_id))
        .collect();
    if missing_ids.is_empty() {
        return false;
    }

    if matches!(
        context
            .activity_service()
            .get_first_activity_date(Some(&missing_ids)),
        Ok(Some(_))
    ) {
        return true;
    }

    missing_ids.iter().any(|account_id| {
        matches!(
            context
                .snapshot_service()
                .get_latest_holdings_snapshot(account_id),
            Ok(Some(_))
        )
    })
}

impl NativeProfiles {
    pub fn new(root: String) -> Result<Self, String> {
        let (root, legacy) =
            match crate::data_dir::development_override(std::env::var_os("WF_DATA_DIR"))? {
                Some(root) => {
                    let database = root.join("app.db");
                    (root, database)
                }
                None => {
                    let database = wealthfolio_storage_sqlite::db::get_db_path(&root);
                    (root.into(), database.into())
                }
            };
        let registry = ProfileRegistry::open(root, legacy, shared_secret_store())
            .map_err(|e| e.to_string())?;
        for id in registry.pending_deletions().map_err(|e| e.to_string())? {
            if registry.finish_delete(id).is_err() {
                log::warn!("Profile deletion cleanup needs a retry.");
            }
        }
        Ok(Self {
            registry: Arc::new(registry),
            active: Mutex::new(None),
            transition: tokio::sync::Mutex::new(()),
            starting: AtomicBool::new(true),
            lock_epoch: AtomicU64::new(0),
        })
    }

    pub fn active(&self) -> Result<Option<Arc<DatabaseRuntime>>, String> {
        self.active
            .lock()
            .map(|r| r.clone())
            .map_err(|_| "Profile runtime is unavailable.".into())
    }

    pub fn try_context(&self) -> Option<Arc<ServiceContext>> {
        let session = self.registry.sessions.current(NATIVE_OWNER).ok()??;
        let runtime = self.active().ok()??;
        (runtime.profile_id == session.profile_id && runtime.generation() == session.generation)
            .then(|| runtime.try_context())
            .flatten()
    }

    /// Capture the original grant before checking context identity. A queued
    /// event keeps this scope even if a switch happens immediately afterward.
    pub fn event_scope(&self, context: &ServiceContext) -> Option<Uuid> {
        let session = self.registry.sessions.current(NATIVE_OWNER).ok()??;
        let runtime = self.admit(session.scope_id).ok()?;
        let current = runtime.try_context()?;
        if context.profile_id != session.profile_id
            || !context.is_active()
            || !std::ptr::eq(current.as_ref(), context)
        {
            return None;
        }
        Some(session.scope_id)
    }

    pub async fn startup(&self, handle: &AppHandle) -> Result<Option<Arc<ServiceContext>>, String> {
        let result = async {
            let list = self.registry.list().map_err(|e| e.to_string())?;
            if list.len() == 1 && !list[0].lock_enabled {
                match self.unlock(handle, list[0].id, None).await {
                    Ok(_) => return Ok(self.try_context()),
                    Err(error) if error.starts_with("PROFILE_LOCKED") => return Ok(None),
                    Err(error) => return Err(error),
                }
            }
            Ok(None)
        }
        .await;
        self.starting.store(false, Ordering::SeqCst);
        result
    }

    pub fn state(&self) -> Result<ProfileState, String> {
        let mut session = self
            .registry
            .sessions
            .current(NATIVE_OWNER)
            .map_err(|e| e.to_string())?;
        // A successful recovery/rebuild invalidates old requests. Only an already
        // authorized session may receive the replacement scope.
        if let (Some(current), Some(runtime)) = (&session, self.active()?) {
            let status = runtime.startup_status();
            if current.profile_id == runtime.profile_id
                && !status.maintenance
                && (status.ready || status.can_recover)
                && current.generation != status.generation
            {
                session = Some(
                    self.registry
                        .sessions
                        .rotate(NATIVE_OWNER, current.scope_id, status.generation)
                        .map_err(|e| e.to_string())?,
                );
            }
        }
        Ok(ProfileState {
            profiles: self.registry.list().map_err(|e| e.to_string())?,
            pending_deletions: self
                .registry
                .pending_profiles()
                .map_err(|e| e.to_string())?,
            session,
            starting: self.starting.load(Ordering::SeqCst),
        })
    }

    pub async fn unlock(
        &self,
        handle: &AppHandle,
        id: Uuid,
        proof: Option<String>,
    ) -> Result<ProfileSession, String> {
        let epoch = self.lock_epoch.load(Ordering::SeqCst);
        let _transition = self.transition.lock().await;
        let registry = self.registry.clone();
        let protected = tauri::async_runtime::spawn_blocking(move || {
            let proof = proof.map(Zeroizing::new);
            registry.verify(id, proof.as_deref().map(String::as_str))
        })
        .await
        .map_err(|_| "Profile verification failed.".to_string())?
        .map_err(|e| e.to_string())?;
        self.registry
            .sessions
            .revoke(NATIVE_OWNER)
            .map_err(|e| e.to_string())?;
        if let Some(old) = self.active()? {
            old.shutdown(handle).await?;
        }
        self.registry
            .auth_flows
            .select(NATIVE_OWNER, id)
            .map_err(|e| e.to_string())?;
        let profile = self.registry.profile(id).map_err(|e| e.to_string())?;
        let mut runtime = DatabaseRuntime::for_profile(
            id,
            self.registry.paths(&profile),
            self.registry.secret_store(&profile),
        );
        runtime.profile_registry = Some(self.registry.clone());
        let runtime = Arc::new(runtime);
        *self
            .active
            .lock()
            .map_err(|_| "Profile runtime is unavailable.")? = Some(runtime.clone());
        // Failed database startup still gets an authorized recovery session.
        if let Err(error) = runtime.initialize(handle).await {
            log::warn!("Profile database startup failed: {error}");
        }
        if epoch != self.lock_epoch.load(Ordering::SeqCst) {
            runtime.shutdown(handle).await?;
            return Err(ProfileError::Locked.to_string());
        }
        let session = self
            .registry
            .sessions
            .issue(NATIVE_OWNER, id, protected, runtime.generation())
            .map_err(|e| e.to_string())?;
        // Run this for every activated profile, including one opened from the
        // chooser after startup. Issue its scope first so job events are scoped.
        if let Some(context) = runtime.try_context() {
            if portfolio_history_backfill_needed(&context) {
                crate::events::emit_portfolio_trigger_recalculate(
                    handle,
                    crate::events::PortfolioRequestPayload::builder().build(),
                    &context,
                );
            }
        }
        Ok(session)
    }

    pub async fn lock(&self, handle: &AppHandle) -> Result<(), String> {
        self.lock_epoch.fetch_add(1, Ordering::SeqCst);
        self.registry
            .sessions
            .revoke(NATIVE_OWNER)
            .map_err(|e| e.to_string())?;
        if let Some(runtime) = self.active()? {
            runtime.suspend();
        }
        let _ = handle.emit(PROFILE_CHANGED, ());
        let _transition = self.transition.lock().await;
        self.registry
            .sessions
            .revoke(NATIVE_OWNER)
            .map_err(|e| e.to_string())?;
        if let Some(runtime) = self.active()? {
            runtime.shutdown(handle).await?;
        }
        *self
            .active
            .lock()
            .map_err(|_| "Profile runtime is unavailable.")? = None;
        Ok(())
    }

    pub fn admit(&self, scope: Uuid) -> Result<Arc<DatabaseRuntime>, String> {
        let session = self
            .registry
            .sessions
            .admit(NATIVE_OWNER, scope)
            .map_err(|e| e.to_string())?;
        let runtime = self
            .active()?
            .ok_or_else(|| ProfileError::Locked.to_string())?;
        if runtime.profile_id != session.profile_id || runtime.generation() != session.generation {
            return Err(ProfileError::Stale.to_string());
        }
        Ok(runtime)
    }
}

/// Command-boundary access to the runtime admitted for the caller's profile session.
/// Extraction checks window identity, scope and runtime generation once. The captured
/// runtime never changes when another profile opens; this is not a revocable lease
/// and does not cancel work already admitted. Database operations separately enforce
/// suspension and maintenance gates. Internal helpers should accept only the service,
/// runtime or path they need, retaining runtime/file leases for database work.
pub struct ProfileAccess(
    pub Arc<DatabaseRuntime>,
    #[allow(dead_code)] Option<tokio::sync::OwnedRwLockReadGuard<()>>,
);

impl std::ops::Deref for ProfileAccess {
    type Target = Arc<DatabaseRuntime>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de, R: tauri::Runtime> tauri::ipc::CommandArg<'de, R> for ProfileAccess {
    fn from_command(
        command: tauri::ipc::CommandItem<'de, R>,
    ) -> Result<Self, tauri::ipc::InvokeError> {
        let webview = command.message.webview();
        if webview.label() != NATIVE_OWNER {
            return Err(ProfileError::Locked.to_string().into());
        }
        let scope = match command.message.payload() {
            tauri::ipc::InvokeBody::Json(body) => body
                .get("scopeId")
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok()),
            _ => None,
        }
        .ok_or_else(|| tauri::ipc::InvokeError::from(ProfileError::Locked.to_string()))?;
        let profiles = webview.state::<NativeProfiles>();
        let runtime = profiles
            .admit(scope)
            .map_err(tauri::ipc::InvokeError::from)?;
        if runtime.generation().is_none()
            && ![
                "get_database_startup_status",
                "retry_database_startup",
                "get_database_encryption_status",
                "list_database_backups",
                "inspect_database_backup",
                "inspect_saved_database_backup",
                "discard_database_backup_import",
                "recover_database_from_import",
                "profile_transfer_file",
                "export_database_backup",
            ]
            .contains(&command.message.command())
        {
            return Err("Profile database is unavailable; open recovery first".into());
        }
        let admission = if command.message.command() == "store_sync_session" {
            None
        } else {
            Some(
                runtime
                    .connect_transition
                    .clone()
                    .try_read_owned()
                    .map_err(|_| {
                        tauri::ipc::InvokeError::from(
                            "Connect account change is in progress. Try again.",
                        )
                    })?,
            )
        };
        Ok(Self(runtime, admission))
    }
}

#[tauri::command]
pub async fn delete_profile(
    handle: AppHandle,
    profile_id: Uuid,
    scope_id: Option<Uuid>,
    confirmation: String,
    proof: Option<String>,
) -> Result<(), String> {
    // Keep cleanup running even if the renderer reloads or drops its IPC future.
    tauri::async_runtime::spawn(async move {
        let state = handle.state::<NativeProfiles>();
        let _transition = state.transition.lock().await;
        if !state.registry.is_deleting(profile_id) {
            let session = state
                .registry
                .sessions
                .admit(
                    NATIVE_OWNER,
                    scope_id.ok_or_else(|| ProfileError::Locked.to_string())?,
                )
                .map_err(|e| e.to_string())?;
            if session.profile_id != profile_id {
                return Err(ProfileError::Stale.to_string());
            }
            let registry = state.registry.clone();
            tauri::async_runtime::spawn_blocking(move || {
                registry.begin_delete(profile_id, &confirmation, proof.as_deref())
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        }
        if let Some(runtime) = state.active()? {
            if runtime.profile_id == profile_id {
                runtime.shutdown(&handle).await?;
            }
        }
        if state.active()?.is_some_and(|r| r.profile_id == profile_id) {
            *state
                .active
                .lock()
                .map_err(|_| "Profile runtime is unavailable.")? = None;
        }
        let registry = state.registry.clone();
        let result =
            tauri::async_runtime::spawn_blocking(move || registry.finish_delete(profile_id))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string());
        let _ = handle.emit(PROFILE_CHANGED, ());
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn get_profile_state(state: tauri::State<'_, NativeProfiles>) -> Result<ProfileState, String> {
    state.state()
}

#[tauri::command]
pub async fn create_profile(
    state: tauri::State<'_, NativeProfiles>,
    name: String,
    avatar_id: String,
    password: Option<String>,
) -> Result<serde_json::Value, String> {
    let registry = state.registry.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let password = password.map(Zeroizing::new);
        let (profile, recovery_code) = registry
            .create_with_password(&name, &avatar_id, password.as_deref().map(String::as_str))
            .map_err(|e| e.to_string())?;
        let mut result = serde_json::to_value(profile).map_err(|e| e.to_string())?;
        result["recoveryCode"] = serde_json::json!(recovery_code);
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unlock_profile(
    handle: AppHandle,
    state: tauri::State<'_, NativeProfiles>,
    profile_id: Uuid,
    proof: Option<String>,
) -> Result<ProfileSession, String> {
    state.unlock(&handle, profile_id, proof).await
}

#[tauri::command]
pub async fn lock_profile(
    handle: AppHandle,
    state: tauri::State<'_, NativeProfiles>,
    preserve_auth: Option<bool>,
) -> Result<(), String> {
    if preserve_auth != Some(true) {
        state
            .registry
            .auth_flows
            .cancel(NATIVE_OWNER)
            .map_err(|e| e.to_string())?;
    }
    state.lock(&handle).await
}

#[tauri::command]
pub fn profile_activity(
    state: tauri::State<'_, NativeProfiles>,
    scope_id: Uuid,
) -> Result<(), String> {
    state
        .registry
        .sessions
        .activity(NATIVE_OWNER, scope_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_profile(
    handle: AppHandle,
    state: ProfileAccess,
    name: String,
    avatar_id: String,
) -> Result<(), String> {
    let registry = handle.state::<NativeProfiles>().registry.clone();
    let id = state.profile_id;
    tauri::async_runtime::spawn_blocking(move || registry.update(id, &name, &avatar_id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_profile_password(
    handle: AppHandle,
    state: ProfileAccess,
    proof: Option<String>,
    password: Option<String>,
) -> Result<Option<String>, String> {
    let registry = handle.state::<NativeProfiles>().registry.clone();
    let id = state.profile_id;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let proof = proof.map(Zeroizing::new);
        let password = password.map(Zeroizing::new);
        registry.set_password(
            id,
            proof.as_deref().map(String::as_str),
            password.as_deref().map(String::as_str),
        )
    })
    .await
    .map_err(|_| "Profile password update failed.".to_string())?
    .map_err(|e| e.to_string())?;
    drop(state);
    let _ = handle.state::<NativeProfiles>().lock(&handle).await;
    Ok(result)
}

#[tauri::command]
pub async fn recover_profile_password(
    handle: AppHandle,
    state: tauri::State<'_, NativeProfiles>,
    profile_id: Uuid,
    recovery_code: String,
    password: String,
) -> Result<String, String> {
    // Recovery is an authentication operation; initial setup still requires an
    // admitted session and cannot be reached through this unauthenticated route.
    let registry = state.registry.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let recovery = Zeroizing::new(recovery_code);
        let password = Zeroizing::new(password);
        let protected = registry.verify(profile_id, Some(&recovery))?;
        if !protected {
            return Err(ProfileError::Locked);
        }
        registry.set_password(profile_id, Some(&recovery), Some(&password))
    })
    .await
    .map_err(|_| "Profile recovery failed.".to_string())?
    .map_err(|e| e.to_string())?;
    let _ = state.lock(&handle).await;
    result.ok_or_else(|| "Profile recovery failed.".into())
}

// Backend time is authoritative; network polling never counts as user activity.
pub fn start_lock_monitor(handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let profiles = handle.state::<NativeProfiles>();
            if profiles.transition.try_lock().is_ok()
                && !profiles.starting.load(Ordering::SeqCst)
                && profiles.active().ok().flatten().is_some()
                && profiles
                    .registry
                    .sessions
                    .current(NATIVE_OWNER)
                    .ok()
                    .flatten()
                    .is_none()
            {
                if let Err(error) = profiles.lock(&handle).await {
                    log::warn!("Profile remains locked: {error}");
                }
            }
        }
    });
}

#[tauri::command]
pub fn profile_auth_storage(
    handle: AppHandle,
    state: ProfileAccess,
    operation: String,
    key: Option<String>,
    value: Option<String>,
    flow_id: Option<Uuid>,
) -> Result<serde_json::Value, String> {
    let root = handle.state::<NativeProfiles>();
    let flows = &root.registry.auth_flows;
    let result = match operation.as_str() {
        "set" => serde_json::json!(flows
            .set_with_id(
                NATIVE_OWNER,
                state.profile_id,
                &key.ok_or("Missing key")?,
                &value.ok_or("Missing verifier")?,
                flow_id.unwrap_or_else(Uuid::new_v4)
            )
            .map_err(|e: wealthfolio_core::profiles::ProfileError| e.to_string())?),
        "validate" => serde_json::json!(flow_id.is_some_and(|id| flows
            .is_current(NATIVE_OWNER, state.profile_id, id)
            .unwrap_or(false))),
        "get" => serde_json::json!(flows
            .get_scoped(
                NATIVE_OWNER,
                state.profile_id,
                &key.ok_or("Missing key")?,
                flow_id
            )
            .map_err(|e| e.to_string())?),
        "callback" => serde_json::json!(flows
            .callback(NATIVE_OWNER, state.profile_id)
            .map_err(|e| e.to_string())?),
        "remove" => {
            flows
                .remove(NATIVE_OWNER, state.profile_id, flow_id)
                .map_err(|e| e.to_string())?;
            serde_json::Value::Null
        }
        _ => return Err("Invalid authentication operation".into()),
    };
    Ok(result)
}
#[tauri::command]
pub fn capture_profile_auth_callback(
    state: tauri::State<'_, NativeProfiles>,
    flow_id: Uuid,
    callback: String,
) -> Result<(), String> {
    state
        .registry
        .auth_flows
        .capture(NATIVE_OWNER, flow_id, &callback)
        .map_err(|e| e.to_string())
}
