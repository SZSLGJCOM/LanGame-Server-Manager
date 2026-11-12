use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use super::*;
use crate::InstanceProgramMode;
use crate::atomic_file::{compare_and_swap_optional_file_atomically, create_file_atomically};
use crate::instance_settings_lock::InstanceSettingsLock;

#[path = "dragonwilds_bootstrap_ini.rs"]
mod ini;

pub(super) const JOURNAL: &str = ".langame-dragonwilds-bootstrap.json";
const CONFIG: &str = "RSDragonwilds/Saved/Config/WindowsServer/DedicatedServer.ini";

#[derive(Debug, Clone)]
pub struct DragonwildsBootstrapObservation {
    pub world_ready: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Commit {
    world_file: String,
    original_revision: String,
    replacement_revision: String,
    backup_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    instance_id: String,
    install_root: PathBuf,
    world_name: String,
    pending: Vec<u8>,
    original_config: Vec<u8>,
    temporary_config: Vec<u8>,
    observed_guid: Option<[u32; 4]>,
    commit: Option<Commit>,
}

struct StoredJournal {
    value: Journal,
    bytes: Vec<u8>,
}

struct Session {
    paths: StoragePaths,
    instance_id: String,
    lock: InstanceSettingsLock,
    context: WorldContext,
    draft: pending::Pending,
    journal: Mutex<StoredJournal>,
}

/// Owns the instance mutation lease until initialization and cleanup finish.
/// The caller must confirm every owned native process has stopped before either
/// finish or abort. Dropping the session preserves all recovery information.
pub struct DragonwildsBootstrap {
    inner: Arc<Session>,
}

pub async fn prepare_dragonwilds_bootstrap(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<Option<DragonwildsBootstrap>, StorageError> {
    let lock = acquire_instance_settings_mutation_lock(paths, instance_id)?;
    let paths = paths.clone();
    let id = instance_id.to_owned();
    let worker_lock = lock.clone();
    lock.complete_mutation("preparing Dragonwilds native bootstrap", async move {
        let context = load_context(&paths, &id, false).await?;
        require_inactive(&context)?;
        let session_lock = worker_lock.clone();
        worker_lock
            .spawn_blocking(move || prepare(paths, id, context, session_lock))
            .await
            .map_err(|error| invalid(Path::new(JOURNAL), error.to_string()))?
    })
    .await
}

fn require_inactive(context: &WorldContext) -> Result<(), StorageError> {
    if !context.inactive {
        return Err(invalid(
            &context.config,
            "Stop the server before initializing world rules.",
        ));
    }
    Ok(())
}

fn prepare(
    paths: StoragePaths,
    instance_id: String,
    context: WorldContext,
    lock: InstanceSettingsLock,
) -> Result<Option<DragonwildsBootstrap>, StorageError> {
    let journal_path = context.config.join(JOURNAL);
    let stored = pending::load(&context, &instance_id)?;
    let existing = pending::read_document(&journal_path)?;
    if stored.is_none() && existing.is_none() {
        return Ok(None);
    }
    let instance = context
        .config
        .parent()
        .ok_or_else(|| invalid(&context.config, "Missing instance directory."))?;
    if crate::instance_program_mode(instance)? != InstanceProgramMode::Independent {
        return Err(invalid(
            instance,
            "World initialization requires an independent instance runtime.",
        ));
    }
    let canonical = fs::canonicalize(&context.install)
        .map_err(|error| invalid(&context.install, error.to_string()))?;
    let config_path = context.install.join(CONFIG);
    let current = pending::read_document(&config_path)?.ok_or_else(|| {
        invalid(
            &config_path,
            "Materialize DedicatedServer.ini before initializing the world.",
        )
    })?;
    let files = io::world_files(&context.saves)?;
    let mut journal = if let Some(bytes) = existing {
        let value: Journal = serde_json::from_slice(&bytes)
            .map_err(|_| invalid(&journal_path, "World initialization journal is malformed."))?;
        if value.version != 1
            || value.instance_id != instance_id
            || value.install_root != canonical
            || value.world_name != context.default_world_name
        {
            return Err(invalid(
                &journal_path,
                "Initialization belongs to a different runtime or world name. Restore the original world selection before retrying; the generated world was preserved.",
            ));
        }
        if stored
            .as_ref()
            .is_some_and(|stored| stored.bytes != value.pending)
            || (stored.is_none() && value.commit.is_none())
        {
            return Err(invalid(
                &journal_path,
                "The staged world rules changed during initialization.",
            ));
        }
        if value.observed_guid.is_some() && files.is_empty() {
            return Err(invalid(
                &context.saves,
                "The previously recorded world is missing. Restore it before retrying; initialization will not generate a replacement world.",
            ));
        }
        if value.observed_guid.is_none() && !files.is_empty() {
            return Err(invalid(
                &context.saves,
                "A world appeared before its identity was recorded. It may be an imported save; automatic initialization cannot claim it. The world and staged rules were preserved.",
            ));
        }
        StoredJournal { value, bytes }
    } else {
        if !files.is_empty() {
            return Err(invalid(
                &context.saves,
                "Existing or imported world saves cannot be initialized as a new world. The saves and staged rules were preserved.",
            ));
        }
        let draft = stored.as_ref().expect("a draft or journal exists");
        let temporary = ini::replace_password(
            &current,
            Some(&uuid::Uuid::new_v4().simple().to_string()),
            &config_path,
        )?;
        let value = Journal {
            version: 1,
            instance_id: instance_id.clone(),
            install_root: canonical,
            world_name: context.default_world_name.clone(),
            pending: draft.bytes.clone(),
            original_config: current.clone(),
            temporary_config: temporary,
            observed_guid: None,
            commit: None,
        };
        let bytes = journal_bytes(&value, &journal_path)?;
        create_file_atomically(&journal_path, &bytes)
            .map_err(|error| invalid(&journal_path, error.to_string()))?;
        StoredJournal { value, bytes }
    };
    let draft: pending::Pending = serde_json::from_slice(&journal.value.pending).map_err(|_| {
        invalid(
            &journal_path,
            "Initialization contains an invalid world draft.",
        )
    })?;
    pending::validate(&draft, &instance_id, &journal_path)?;
    if ini::value(&current, "DefaultWorldName", &config_path)?.as_deref()
        != Some(context.default_world_name.as_str())
    {
        return Err(invalid(
            &config_path,
            "Rendered world name no longer matches this initialization.",
        ));
    }
    let temporary_password = ini::value(
        &journal.value.temporary_config,
        "WorldPassword",
        &config_path,
    )?
    .filter(|value| value.len() == 32 && value.bytes().all(|value| value.is_ascii_hexdigit()))
    .ok_or_else(|| {
        invalid(
            &journal_path,
            "Initialization temporary password is invalid.",
        )
    })?;
    let old_password = ini::value(
        &journal.value.original_config,
        "WorldPassword",
        &config_path,
    )?;
    let current_password = ini::value(&current, "WorldPassword", &config_path)?;
    if current_password.as_deref() != Some(temporary_password.as_str()) {
        if current_password != old_password
            && current_password.as_deref() != Some(context.world_password.as_str())
        {
            return Err(invalid(
                &config_path,
                "World password changed outside this initialization. Its current value was preserved.",
            ));
        }
        // Normal pre-start rendering may already have restored the operator's
        // password and retained native ServerGuid. Keep that new exact baseline.
        journal.value.original_config = current.clone();
        journal.value.temporary_config =
            ini::replace_password(&current, Some(&temporary_password), &config_path)?;
        publish_journal(&journal_path, &mut journal)?;
    }
    let temporary = ini::replace_password(&current, Some(&temporary_password), &config_path)?;
    if current != temporary
        && !compare_and_swap_file_atomically(&config_path, &current, &temporary)
            .map_err(|error| invalid(&config_path, error.to_string()))?
    {
        return Err(invalid(
            &config_path,
            "DedicatedServer.ini changed while isolating world initialization.",
        ));
    }
    Ok(Some(DragonwildsBootstrap {
        inner: Arc::new(Session {
            paths,
            instance_id,
            lock,
            context,
            draft,
            journal: Mutex::new(journal),
        }),
    }))
}

fn journal_bytes(value: &Journal, path: &Path) -> Result<Vec<u8>, StorageError> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > pending::MAX_DOCUMENT {
        return Err(invalid(
            path,
            "World initialization recovery record exceeds 256 KiB.",
        ));
    }
    Ok(bytes)
}

fn publish_journal(path: &Path, journal: &mut StoredJournal) -> Result<(), StorageError> {
    let result = (|| {
        let bytes = journal_bytes(&journal.value, path)?;
        if !compare_and_swap_file_atomically(path, &journal.bytes, &bytes)
            .map_err(|error| invalid(path, error.to_string()))?
        {
            return Err(invalid(
                path,
                "World initialization recovery record changed.",
            ));
        }
        Ok(bytes)
    })();
    match result {
        Ok(bytes) => {
            journal.bytes = bytes;
            Ok(())
        }
        Err(error) => {
            // Never acknowledge a GUID or commit before its durable publication.
            journal.value = serde_json::from_slice(&journal.bytes)?;
            Err(error)
        }
    }
}

struct NativeWorld {
    path: PathBuf,
    bytes: Vec<u8>,
    guid: [u32; 4],
}

impl Session {
    fn journal(&self) -> Result<MutexGuard<'_, StoredJournal>, StorageError> {
        self.journal.lock().map_err(|_| {
            invalid(
                &self.context.config,
                "World initialization state is unavailable.",
            )
        })
    }

    fn verify(&self, journal: &StoredJournal) -> Result<(), StorageError> {
        if pending::read_document(&self.context.config.join(JOURNAL))?.as_deref()
            != Some(journal.bytes.as_slice())
        {
            return Err(invalid(
                &self.context.config,
                "World initialization recovery record changed.",
            ));
        }
        let draft = pending::read_document(&self.context.config.join(pending::FILE))?;
        if draft.as_deref() != Some(journal.value.pending.as_slice())
            && !(draft.is_none() && journal.value.commit.is_some())
        {
            return Err(invalid(
                &self.context.config,
                "Staged world rules changed during initialization.",
            ));
        }
        Ok(())
    }

    fn native_world(&self, journal: &Journal) -> Result<Option<NativeWorld>, StorageError> {
        let files = io::world_files(&self.context.saves)?;
        if files.is_empty() {
            return Ok(None);
        }
        let mut selected: Option<(i64, NativeWorld)> = None;
        let mut ambiguous = false;
        let mut guid = journal.observed_guid;
        for path in files {
            let bytes = read_world(&path)?;
            let Ok(native) = decode_world_settings(&bytes) else {
                return Ok(None);
            };
            let Ok((metadata, native_guid)) = io::read_bootstrap_world_metadata(&path) else {
                return Ok(None);
            };
            if !native.world_name.eq_ignore_ascii_case(&journal.world_name)
                || guid.is_some_and(|guid| guid != native_guid)
            {
                return Err(invalid(
                    &path,
                    "A different world appeared during initialization. No existing save was replaced.",
                ));
            }
            guid = Some(native_guid);
            if selected
                .as_ref()
                .is_some_and(|(time, _)| *time == metadata.saved_at_ticks)
            {
                ambiguous = true;
                continue;
            }
            if selected
                .as_ref()
                .is_none_or(|(time, _)| *time < metadata.saved_at_ticks)
            {
                ambiguous = false;
                selected = Some((
                    metadata.saved_at_ticks,
                    NativeWorld {
                        path,
                        bytes,
                        guid: native_guid,
                    },
                ));
            }
        }
        if ambiguous {
            return Err(invalid(
                &self.context.saves,
                "Several generated worlds have the same latest save timestamp; their selection is ambiguous.",
            ));
        }
        let world = selected.map(|(_, world)| world);
        if let (Some(commit), Some(world)) = (&journal.commit, &world) {
            let hash = sha256(&world.bytes);
            if world_file_name(&world.path)? != commit.world_file
                || (hash != commit.original_revision && hash != commit.replacement_revision)
            {
                return Err(invalid(
                    &world.path,
                    "The generated world changed after its rules were prepared. Its current data and backup were preserved.",
                ));
            }
        }
        Ok(world)
    }

    fn observe(&self) -> Result<DragonwildsBootstrapObservation, StorageError> {
        let mut journal = self.journal()?;
        self.verify(&journal)?;
        let Some(world) = self.native_world(&journal.value)? else {
            return Ok(DragonwildsBootstrapObservation { world_ready: false });
        };
        if journal.value.observed_guid.is_none() {
            journal.value.observed_guid = Some(world.guid);
            publish_journal(&self.context.config.join(JOURNAL), &mut journal)?;
        }
        Ok(DragonwildsBootstrapObservation { world_ready: true })
    }

    fn prepare_update(&self) -> Result<PreparedUpdate, StorageError> {
        let journal = self.journal()?;
        self.verify(&journal)?;
        if journal.value.observed_guid.is_none() {
            return Err(invalid(
                &self.context.saves,
                "The completed native world must be observed before applying its rules.",
            ));
        }
        let world = self.native_world(&journal.value)?.ok_or_else(|| invalid(&self.context.saves, "The native world is not complete. Its partial output and staged rules were preserved."))?;
        if journal
            .value
            .commit
            .as_ref()
            .is_some_and(|commit| sha256(&world.bytes) == commit.replacement_revision)
        {
            return Ok(PreparedUpdate {
                path: world.path,
                original: world.bytes.clone(),
                replacement: world.bytes,
            });
        }
        let mut native =
            decode_world_settings(&world.bytes).map_err(|error| invalid(&world.path, error))?;
        native.overrides.extend(self.draft.overrides.clone());
        let replacement = patch_world_settings(
            &world.bytes,
            self.draft.world_mode.native_value(),
            &native.overrides,
        )
        .map_err(|error| invalid(&world.path, error))?;
        if replacement.len() > io::MAX_WORLD_BYTES {
            return Err(invalid(
                &world.path,
                "Updated world exceeds the 128 MiB safety limit.",
            ));
        }
        if journal
            .value
            .commit
            .as_ref()
            .is_some_and(|commit| sha256(&replacement) != commit.replacement_revision)
        {
            return Err(invalid(
                &world.path,
                "The staged rule result no longer matches the recovery record.",
            ));
        }
        Ok(PreparedUpdate {
            path: world.path,
            original: world.bytes,
            replacement,
        })
    }

    fn apply_update(
        &self,
        prepared: PreparedUpdate,
        backup_id: Option<String>,
    ) -> Result<(), StorageError> {
        let mut journal = self.journal()?;
        self.verify(&journal)?;
        let current = self.native_world(&journal.value)?.ok_or_else(|| {
            invalid(
                &self.context.saves,
                "The generated world became incomplete.",
            )
        })?;
        if current.path != prepared.path || current.bytes != prepared.original {
            return Err(invalid(
                &prepared.path,
                "The generated world changed while its backup was created.",
            ));
        }
        if journal.value.commit.is_none() {
            journal.value.commit = Some(Commit {
                world_file: world_file_name(&prepared.path)?,
                original_revision: sha256(&prepared.original),
                replacement_revision: sha256(&prepared.replacement),
                backup_id: backup_id.ok_or_else(|| {
                    invalid(&prepared.path, "A verified pre-change backup is required.")
                })?,
            });
            publish_journal(&self.context.config.join(JOURNAL), &mut journal)?;
        }
        let _guards = guard_directories(&self.context.saves)?;
        if prepared.original != prepared.replacement
            && !compare_and_swap_file_atomically(
                &prepared.path,
                &prepared.original,
                &prepared.replacement,
            )
            .map_err(|error| invalid(&prepared.path, error.to_string()))?
        {
            return Err(invalid(
                &prepared.path,
                "The generated world changed while applying its rules.",
            ));
        }
        if read_world(&prepared.path)? != prepared.replacement {
            return Err(invalid(
                &prepared.path,
                "The world rule write could not be confirmed; its backup was preserved.",
            ));
        }
        Ok(())
    }

    fn restore_password(&self) -> Result<(), StorageError> {
        let journal = self.journal()?;
        self.verify(&journal)?;
        let path = self.context.install.join(CONFIG);
        let current = pending::read_document(&path)?.ok_or_else(|| {
            invalid(
                &path,
                "DedicatedServer.ini disappeared during initialization.",
            )
        })?;
        let temporary = ini::value(&journal.value.temporary_config, "WorldPassword", &path)?;
        let original = ini::value(&journal.value.original_config, "WorldPassword", &path)?;
        let password = ini::value(&current, "WorldPassword", &path)?;
        if password == original || password.as_deref() == Some(self.context.world_password.as_str())
        {
            return Ok(());
        }
        if password != temporary {
            return Err(invalid(
                &path,
                "World password changed outside this initialization; its current value was preserved.",
            ));
        }
        let raw_original = ini::raw_value(&journal.value.original_config, "WorldPassword", &path)?;
        let replacement = ini::replace_password(&current, raw_original.as_deref(), &path)?;
        if !compare_and_swap_file_atomically(&path, &current, &replacement)
            .map_err(|error| invalid(&path, error.to_string()))?
        {
            return Err(invalid(
                &path,
                "DedicatedServer.ini changed while restoring the operator's password.",
            ));
        }
        Ok(())
    }

    fn clear(&self) -> Result<(), StorageError> {
        let journal = self.journal()?;
        self.verify(&journal)?;
        let world = self
            .native_world(&journal.value)?
            .ok_or_else(|| invalid(&self.context.saves, "The completed world is missing."))?;
        if journal
            .value
            .commit
            .as_ref()
            .is_none_or(|commit| sha256(&world.bytes) != commit.replacement_revision)
        {
            return Err(invalid(
                &world.path,
                "The completed world does not match its recorded rule result.",
            ));
        }
        let path = self.context.config.join(pending::FILE);
        if let Some(current) = pending::read_document(&path)?
            && (current != journal.value.pending
                || !compare_and_swap_optional_file_atomically(&path, Some(&current), None)
                    .map_err(|error| invalid(&path, error.to_string()))?)
        {
            return Err(invalid(
                &path,
                "World draft changed before initialization could complete.",
            ));
        }
        let path = self.context.config.join(JOURNAL);
        if !compare_and_swap_optional_file_atomically(&path, Some(&journal.bytes), None)
            .map_err(|error| invalid(&path, error.to_string()))?
        {
            return Err(invalid(
                &path,
                "Initialization recovery record changed during completion.",
            ));
        }
        Ok(())
    }
}

impl DragonwildsBootstrap {
    pub fn install_root(&self) -> &Path {
        &self.inner.context.install
    }
    pub fn world_name(&self) -> &str {
        &self.inner.context.default_world_name
    }
    pub fn inspect_native_state(&self) -> Result<DragonwildsBootstrapObservation, StorageError> {
        self.inner.observe()
    }

    async fn worker<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&Session) -> Result<T, StorageError> + Send + 'static,
    ) -> Result<T, StorageError> {
        let session = Arc::clone(&self.inner);
        self.inner
            .lock
            .spawn_blocking(move || operation(&session))
            .await
            .map_err(|error| invalid(Path::new(JOURNAL), error.to_string()))?
    }

    async fn stopped(&self) -> Result<(), StorageError> {
        let context = load_context(&self.inner.paths, &self.inner.instance_id, false).await?;
        require_inactive(&context)?;
        if context.install != self.inner.context.install
            || context.saves != self.inner.context.saves
            || context.default_world_name != self.inner.context.default_world_name
        {
            return Err(invalid(
                &context.config,
                "The initialization target changed while the native server was running.",
            ));
        }
        Ok(())
    }

    async fn finish_rules(&self) -> Result<(), StorageError> {
        let prepared = self.worker(Session::prepare_update).await?;
        let needs_backup = self.inner.journal()?.value.commit.is_none();
        let backup_id = if needs_backup {
            Some(
                crate::backups::create_instance_backup(&self.inner.paths, &self.inner.instance_id)
                    .await?
                    .backup_id,
            )
        } else {
            None
        };
        self.worker(move |session| session.apply_update(prepared, backup_id))
            .await
    }

    pub async fn finish_after_stopped(self) -> Result<(), StorageError> {
        let lease = self.inner.lock.clone();
        lease
            .complete_mutation("finishing Dragonwilds native bootstrap", async move {
                self.stopped().await?;
                let rules = self.finish_rules().await;
                let restore = self.worker(Session::restore_password).await;
                match (rules, restore) {
                    (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
                    (Err(error), Err(restore)) => Err(invalid(
                        &self.inner.context.config,
                        format!("{error}; temporary password recovery also failed: {restore}"),
                    )),
                    (Ok(()), Ok(())) => self.worker(Session::clear).await,
                }
            })
            .await
    }

    pub async fn abort_after_stopped(self) -> Result<(), StorageError> {
        let lease = self.inner.lock.clone();
        lease
            .complete_mutation("recovering Dragonwilds native bootstrap", async move {
                self.stopped().await?;
                self.worker(Session::restore_password).await
            })
            .await
    }
}
