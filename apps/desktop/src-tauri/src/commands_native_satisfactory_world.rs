//! Opt-in native acceptance prelude, never a product startup default.
//! Claim/create/load use the same owned HTTPS commands as the world workbench.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use app_core::{InstanceDetails, InstanceStatus, satisfactory_world::*};
use app_storage::read_instance_details;
use keyring::{Entry, Error as CredentialError};

use crate::state::DesktopState;

const CREDENTIAL_KINDS: [&str; 2] = ["admin-password", "api-token"];
const LOAD_TIMEOUT: Duration = Duration::from_secs(120);
const API_READY_TIMEOUT: Duration = Duration::from_secs(90);

fn native_api_still_loading(error: &str) -> bool {
    matches!(
        error,
        "The Satisfactory server-name query timed out."
            | "The Satisfactory HTTPS API is unavailable or still loading. Refresh when the server is ready."
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Fresh,
    InitialAttempted,
    InitialReady,
    RestartAttempted,
    RestartReady,
}

impl Phase {
    fn admit(&mut self, cycle: &str) -> Result<(), String> {
        *self = match (cycle, *self) {
            ("initial", Self::Fresh) => Self::InitialAttempted,
            ("restart", Self::InitialReady) => Self::RestartAttempted,
            _ => return Err("Satisfactory fixture preparation was already attempted or has an invalid cycle; no mutation was retried.".into()),
        };
        Ok(())
    }
    fn ready(&mut self, cycle: &str) -> Result<(), String> {
        *self = match (cycle, *self) {
            ("initial", Self::InitialAttempted) => Self::InitialReady,
            ("restart", Self::RestartAttempted) => Self::RestartReady,
            _ => {
                return Err(
                    "Satisfactory fixture preparation changed lifecycle unexpectedly.".into(),
                );
            }
        };
        Ok(())
    }
}

fn selected(manual_save: bool, module_id: &str) -> bool {
    manual_save && module_id == "satisfactory"
}

// No Debug implementation: this guard owns a newly allocated credential namespace.
pub(crate) struct NativeWorld {
    instance_id: String,
    fixture_root: PathBuf,
    session_name: String,
    credential_identity: String,
    cleanup_allowed: Arc<AtomicBool>,
    credential_cleanup_attempted: bool,
    phase: Mutex<Phase>,
}

impl NativeWorld {
    pub(crate) fn new(
        details: &InstanceDetails,
        fixture_root: &Path,
        cleanup_allowed: Arc<AtomicBool>,
    ) -> Result<Option<Self>, String> {
        if !selected(
            std::env::var("LANGAME_NATIVE_MANUAL_SAVE").as_deref() == Ok("true"),
            &details.summary.module_id,
        ) {
            return Ok(None);
        }
        if !matches!(details.summary.status, InstanceStatus::Stopped)
            || details.active_run.is_some()
            || details.summary.active_process_count != 0
        {
            return Err(
                "Satisfactory native fixture preparation requires a new stopped instance.".into(),
            );
        }
        let fixture_root = fixture_root
            .canonicalize()
            .map_err(|_| "The native fixture directory is unavailable.")?;
        let config = Path::new(&details.config_file_path)
            .canonicalize()
            .map_err(|_| "The native fixture configuration is unavailable.")?;
        if !config.starts_with(&fixture_root) || config == fixture_root {
            return Err("Satisfactory native preparation cannot touch an instance outside its disposable fixture.".into());
        }
        let settings: serde_json::Value = serde_json::from_str(&details.settings_json)
            .map_err(|_| "The native fixture settings are malformed.")?;
        if settings
            .get("allow_insecure_local_api")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            return Err(
                "The native fixture must use normal authenticated Satisfactory management.".into(),
            );
        }
        let credential_identity = super::context::credential_identity(details)?;
        // Acquire deletion ownership only after both exact new accounts are absent.
        for kind in CREDENTIAL_KINDS {
            let account = format!("{credential_identity}/{kind}");
            let entry = Entry::new(super::credentials::SERVICE, &account)
                .map_err(|_| "The native fixture credential store is unavailable.")?;
            match entry.get_password() {
                Err(CredentialError::NoEntry) => {}
                Ok(_) => {
                    return Err(
                        "The native fixture credential namespace already exists; it was preserved."
                            .into(),
                    );
                }
                Err(_) => {
                    return Err(
                        "The native fixture credential namespace could not be checked.".into(),
                    );
                }
            }
        }
        Ok(Some(Self {
            instance_id: details.summary.id.clone(),
            fixture_root,
            session_name: format!("LGSM_Native_Save_{}", uuid::Uuid::new_v4().simple()),
            credential_identity,
            cleanup_allowed,
            credential_cleanup_attempted: false,
            phase: Mutex::new(Phase::Fresh),
        }))
    }

    async fn current(
        &self,
        state: &tauri::State<'_, DesktopState>,
        expected: &InstanceDetails,
    ) -> Result<(), String> {
        let storage = state.bootstrap_storage()?;
        let current = read_instance_details(&storage.paths, &self.instance_id)
            .await
            .map_err(|_| "The native Satisfactory run could not be reread.")?;
        let expected_run = expected
            .active_run
            .as_ref()
            .ok_or("The native fixture has no active Satisfactory run.")?;
        let run = current
            .active_run
            .as_ref()
            .ok_or("The native Satisfactory run has ended.")?;
        if current.summary.module_id != "satisfactory"
            || !matches!(current.summary.status, InstanceStatus::Running)
            || run.run_id != expected_run.run_id
            || run.session_id != expected_run.session_id
            || run.pid != expected_run.pid
            || run.processes.len() != 1
            || expected_run.processes.len() != 1
            || run.processes[0].process_identity != expected_run.processes[0].process_identity
            || run.processes[0].process_key != expected_run.processes[0].process_key
        {
            return Err("The Satisfactory native fixture changed its managed run.".into());
        }
        let image = run.processes[0]
            .process_identity
            .as_ref()
            .ok_or("The native Satisfactory process identity is missing.")?;
        let image = Path::new(&image.image_path)
            .canonicalize()
            .map_err(|_| "The native Satisfactory image is unavailable.")?;
        if !image.starts_with(&self.fixture_root) {
            return Err("The native Satisfactory process escaped its disposable fixture.".into());
        }
        Ok(())
    }

    async fn read(
        &self,
        state: &tauri::State<'_, DesktopState>,
        expected: &InstanceDetails,
    ) -> Result<SatisfactoryWorldSnapshot, String> {
        self.current(state, expected).await?;
        let result =
            super::read_satisfactory_world_settings(state.clone(), self.instance_id.clone()).await;
        self.current(state, expected).await?;
        let snapshot = result?;
        if snapshot.instance_id != self.instance_id {
            return Err("The native Satisfactory API returned another fixture identity.".into());
        }
        Ok(snapshot)
    }

    async fn available(
        &self,
        state: &tauri::State<'_, DesktopState>,
        expected: &InstanceDetails,
    ) -> Result<SatisfactoryWorldSnapshot, String> {
        // Ports can listen before the native lightweight query/API is ready.
        // Poll only closed loading failures; no world mutation is retried.
        tokio::time::timeout(API_READY_TIMEOUT, async {
            loop {
                match self.read(state, expected).await {
                    Ok(snapshot) => return Ok(snapshot),
                    Err(error) if native_api_still_loading(&error) => {}
                    Err(error) => return Err(error),
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }).await.map_err(|_| "Satisfactory native management queries did not become ready within the bounded startup observation.".to_owned())?
    }

    async fn loaded(
        &self,
        state: &tauri::State<'_, DesktopState>,
        expected: &InstanceDetails,
    ) -> Result<(), String> {
        tokio::time::timeout(LOAD_TIMEOUT, async {
            loop {
                match self.read(state, expected).await {
                    Ok(snapshot) => {
                        if loaded_world(&snapshot, &self.instance_id, &self.session_name) { return Ok(()); }
                        if snapshot.is_game_running && snapshot.active_session_name != self.session_name {
                            return Err("Satisfactory loaded an unexpected fixture world.".into());
                        }
                    }
                    Err(error) if native_api_still_loading(&error) => {}
                    Err(error) => return Err(error),
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }).await.map_err(|_| "Satisfactory did not report the owned fixture world loaded within its bounded observation window.".to_owned())?
    }

    pub(crate) async fn prepare(
        &self,
        state: &tauri::State<'_, DesktopState>,
        details: &InstanceDetails,
        cycle: &str,
    ) -> Result<(), String> {
        self.phase
            .lock()
            .map_err(|_| "The native Satisfactory fixture lifecycle lock failed.")?
            .admit(cycle)?;
        let _operation = state
            .begin_storage_context_operation("isolated Satisfactory native world preparation")?;
        let snapshot = self.available(state, details).await?;
        if cycle == "initial" {
            require_fresh(&snapshot, &self.instance_id)?;
            let claimed = super::setup_satisfactory_server(
                state.clone(),
                SetupSatisfactoryServerInput {
                    instance_id: self.instance_id.clone(),
                    server_name: "LanGame isolated native save acceptance".into(),
                    admin_password: None,
                },
            )
            .await?;
            if claimed.instance_id != self.instance_id
                || claimed.connection_status != SatisfactoryConnectionStatus::Ready
                || claimed.is_game_running
                || !claimed.sessions.is_empty()
                || claimed.connected_players != 0
            {
                return Err(
                    "The freshly claimed native fixture is not an empty authenticated server."
                        .into(),
                );
            }
            self.current(state, details).await?;
            let result = super::create_satisfactory_world(
                state.clone(),
                CreateSatisfactoryWorldInput {
                    instance_id: self.instance_id.clone(),
                    expected_revision: claimed.revision,
                    session_name: self.session_name.clone(),
                    starting_location: "Grass Fields".into(),
                    skip_onboarding: true,
                    acknowledge_enable_advanced_settings: false,
                    game_mode_settings: BTreeMap::new(),
                    advanced_game_settings: BTreeMap::new(),
                },
            )
            .await?;
            self.accepted(&result)?;
        } else if let Some(save_name) =
            restart_save(&snapshot, &self.instance_id, &self.session_name)?
        {
            self.current(state, details).await?;
            let result = super::load_satisfactory_save(
                state.clone(),
                LoadSatisfactorySaveInput {
                    instance_id: self.instance_id.clone(),
                    expected_revision: snapshot.revision,
                    save_name,
                },
            )
            .await?;
            self.accepted(&result)?;
        }
        self.loaded(state, details).await?;
        self.phase
            .lock()
            .map_err(|_| "The native Satisfactory fixture lifecycle lock failed.")?
            .ready(cycle)?;
        println!(
            "NATIVE_SATISFACTORY_WORLD cycle={cycle} fixture_only=true native_world_loaded=true credentials=owned_system_store content_readback=not_claimed"
        );
        Ok(())
    }

    fn accepted(&self, result: &SatisfactoryWorldOperationResult) -> Result<(), String> {
        if !result.accepted
            || result.instance_id != self.instance_id
            || result.session_name != self.session_name
        {
            return Err("The native Satisfactory fixture world request was not acknowledged; it was not retried.".into());
        }
        Ok(())
    }

    // Call after NativeCleanup has stopped and verified the owned process tree.
    // Deletion and absence confirmation are part of acceptance, not only a log.
    pub(crate) fn finish(&mut self) -> Result<(), String> {
        if self.credential_cleanup_attempted {
            return Err("Satisfactory fixture credential cleanup was already attempted.".into());
        }
        self.credential_cleanup_attempted = true;
        if !self.cleanup_allowed.load(Ordering::SeqCst) {
            return Err("Satisfactory fixture credentials were retained because owned process cleanup is unconfirmed.".into());
        }
        let mut failed = false;
        for kind in CREDENTIAL_KINDS {
            let account = format!("{}/{kind}", self.credential_identity);
            let Ok(entry) = Entry::new(super::credentials::SERVICE, &account) else {
                failed = true;
                continue;
            };
            if let Err(error) = entry.delete_credential()
                && !matches!(error, CredentialError::NoEntry)
            {
                failed = true;
            }
            if !matches!(entry.get_password(), Err(CredentialError::NoEntry)) {
                failed = true;
            }
        }
        if failed {
            return Err("Satisfactory fixture credential deletion could not be confirmed for both owned accounts.".into());
        }
        eprintln!(
            "NATIVE_SATISFACTORY_WORLD phase=credential_cleanup outcome=passed absence_confirmed=true"
        );
        Ok(())
    }
}

impl Drop for NativeWorld {
    fn drop(&mut self) {
        if self.credential_cleanup_attempted {
            return;
        }
        if self.finish().is_err() {
            eprintln!(
                "NATIVE_SATISFACTORY_WORLD phase=credential_cleanup outcome=unconfirmed fallback=true"
            );
        }
    }
}

fn require_fresh(snapshot: &SatisfactoryWorldSnapshot, instance: &str) -> Result<(), String> {
    if snapshot.instance_id != instance
        || snapshot.connection_status != SatisfactoryConnectionStatus::Unclaimed
        || snapshot.is_game_running
        || !snapshot.active_session_name.is_empty()
        || !snapshot.sessions.is_empty()
        || snapshot.connected_players != 0
    {
        return Err("Only an empty unclaimed disposable Satisfactory server may be initialized by this native test.".into());
    }
    Ok(())
}

fn loaded_world(snapshot: &SatisfactoryWorldSnapshot, instance: &str, world: &str) -> bool {
    snapshot.instance_id == instance
        && snapshot.connection_status == SatisfactoryConnectionStatus::Ready
        && snapshot.is_game_running
        && snapshot.active_session_name == world
        && snapshot.connected_players == 0
}

fn restart_save(
    snapshot: &SatisfactoryWorldSnapshot,
    instance: &str,
    world: &str,
) -> Result<Option<String>, String> {
    if snapshot.instance_id != instance
        || snapshot.connection_status != SatisfactoryConnectionStatus::Ready
        || snapshot.connected_players != 0
        || snapshot.revision.is_empty()
        || snapshot.sessions.len() != 1
        || snapshot.sessions[0].session_name != world
        || (snapshot.is_game_running && snapshot.active_session_name != world)
    {
        return Err("The restarted Satisfactory fixture is not the unique previously created world; it was not recreated.".into());
    }
    let mut saves = snapshot.sessions[0]
        .saves
        .iter()
        .filter(|save| save.save_name.starts_with("LGSM_manual_save_"));
    let save = saves
        .next()
        .ok_or("The previous formal Satisfactory manual save is absent; no world was recreated.")?;
    if saves.next().is_some() {
        return Err(
            "The previous Satisfactory manual-save checkpoint is ambiguous; no load was attempted."
                .into(),
        );
    }
    Ok(if loaded_world(snapshot, instance, world) {
        None
    } else {
        Some(save.save_name.clone())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_startup_observation_polls_only_known_loading_reads() {
        assert!(native_api_still_loading(
            "The Satisfactory server-name query timed out."
        ));
        assert!(native_api_still_loading(
            "The Satisfactory HTTPS API is unavailable or still loading. Refresh when the server is ready."
        ));
        for error in [
            "Authorize Satisfactory server management before continuing.",
            "The Satisfactory API listener is no longer owned by this instance. Refresh the server state.",
            "The Satisfactory HTTPS API returned an incomplete response.",
            "The Satisfactory request outcome could not be confirmed. Refresh or reconnect before retrying; the request was not resent.",
        ] {
            assert!(!native_api_still_loading(error));
        }
    }
    fn snapshot() -> SatisfactoryWorldSnapshot {
        serde_json::from_value(serde_json::json!({
            "instance_id":"fixture", "connection_status":"ready", "revision":"revision",
            "server_name":null, "active_session_name":"OwnedWorld", "auto_load_session_name":"",
            "is_game_running":true, "connected_players":0, "creative_mode_enabled":false,
            "advanced_game_settings":{}, "server_options":{}, "pending_server_options":{},
            "sessions":[{"session_name":"OwnedWorld", "saves":[{"save_name":"LGSM_manual_save_once", "save_date_time":"2026-10-19", "play_duration_seconds":1, "is_creative_mode_enabled":false}]}],
            "rule_definitions":[], "starting_locations":[]
        })).unwrap()
    }
    #[test]
    fn prelude_is_limited_to_opted_in_satisfactory_and_never_retries_mutation_admission() {
        assert!(selected(true, "satisfactory"));
        assert!(!selected(false, "satisfactory"));
        assert!(!selected(true, "minecraft"));
        let mut phase = Phase::Fresh;
        assert!(phase.admit("restart").is_err());
        phase.admit("initial").unwrap();
        assert!(phase.admit("initial").is_err());
        phase.ready("initial").unwrap();
        phase.admit("restart").unwrap();
        assert!(phase.admit("restart").is_err());
        phase.ready("restart").unwrap();
        assert!(phase.admit("restart").is_err());
    }
    #[test]
    fn initial_claim_requires_a_truly_empty_unclaimed_server() {
        let mut fresh = snapshot();
        assert!(require_fresh(&fresh, "fixture").is_err());
        fresh.connection_status = SatisfactoryConnectionStatus::Unclaimed;
        fresh.active_session_name.clear();
        fresh.is_game_running = false;
        fresh.sessions.clear();
        assert!(require_fresh(&fresh, "fixture").is_ok());
        fresh.connected_players = 1;
        assert!(require_fresh(&fresh, "fixture").is_err());
    }
    #[test]
    fn restart_uses_only_the_existing_unique_owned_world_and_formal_checkpoint() {
        let mut current = snapshot();
        assert_eq!(
            restart_save(&current, "fixture", "OwnedWorld").unwrap(),
            None
        );
        current.is_game_running = false;
        assert_eq!(
            restart_save(&current, "fixture", "OwnedWorld").unwrap(),
            Some("LGSM_manual_save_once".into())
        );
        assert!(restart_save(&current, "other", "OwnedWorld").is_err());
        assert!(restart_save(&current, "fixture", "OtherWorld").is_err());
        current.sessions[0].saves.clear();
        assert!(restart_save(&current, "fixture", "OwnedWorld").is_err());
        let mut duplicate = snapshot();
        let duplicate_save = duplicate.sessions[0].saves[0].clone();
        duplicate.sessions[0].saves.push(duplicate_save);
        assert!(restart_save(&duplicate, "fixture", "OwnedWorld").is_err());
        let mut foreign = snapshot();
        let foreign_session = foreign.sessions[0].clone();
        foreign.sessions.push(foreign_session);
        assert!(restart_save(&foreign, "fixture", "OwnedWorld").is_err());
    }
}
