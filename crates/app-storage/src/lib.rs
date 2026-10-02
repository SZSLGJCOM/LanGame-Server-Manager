use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use app_core::{
    ActiveInstanceRun, AppSettings, CreateInstanceInput, DEFAULT_LANGAME_DATA_ROOT,
    INSTANCE_CREATION_DEFAULT_AUTOSTART, INSTANCE_CREATION_DEFAULT_BIND_IP,
    InsertInstanceBroadcastEventInput, InstallState, InstanceBackupKind,
    InstanceBackupRestoreResult, InstanceBackupResult, InstanceBroadcastEvent,
    InstanceBroadcastPolicy, InstanceBroadcastRules, InstanceDetails, InstanceProcessState,
    InstanceProvisioning, InstanceRunRecord, InstanceRuntimeOverview, InstanceStatus,
    InstanceSummary, LogTailSnapshot, ModulePortGroupSpec, ModuleSummary, PortBinding,
    ProcessIdentity, StorageStatus, UpdateInstanceBroadcastPolicyInput, UpdateInstanceInput,
};
use app_modules::ModuleDescriptor;
use serde_json::{Map, Value, json};
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow};
use sqlx::{Executor, Row, Sqlite, SqlitePool, Transaction};
use thiserror::Error;
use uuid::Uuid;

mod a2s;
mod archived_programs;
pub use archived_programs::{ArchivedProgramSource, read_archived_program_sources};
mod assistant_session_archive;
pub use assistant_session_archive::{AssistantSessionArchive, assistant_session_binding_digest};
mod ark_cluster_backups;
mod ark_clusters;
mod ark_maps;
mod atomic_file;
mod backups;
mod broadcast;
mod install_save_paths;
mod instance_archive;
mod instance_archive_resources;
pub use instance_archive_resources::{
    InstanceStorageResources, read_instance_archive_cleanup_resources,
    read_instance_archive_resources, read_instance_retirement_resources,
};
mod instance_archive_roots;
pub use instance_archive_roots::{ArchiveRootSettingsGuard, guard_archive_root_settings_update};
mod instance_archive_files;
mod instance_archive_models;
mod instance_archive_store;
mod instance_autostart;
mod instance_creation_io;
mod instance_deletion;
mod instance_reconciliation;
mod instance_retirement_paths;
mod instance_stored_settings;
pub use instance_reconciliation::{list_missing_instance_candidates, reconcile_missing_instance};
pub use instance_stored_settings::read_instance_stored_settings;
mod instance_native_settings;
pub use instance_archive::{
    InstanceArchiveDetails, InstanceRemovalPlan, ensure_program_archive_dependencies,
    inspect_instance_removal, list_instance_archives, pending_instance_archive_ids,
    purge_instance_archive, read_instance_archive_details, recover_instance_archives,
    restore_instance_archive,
};
pub use instance_archive_models::{
    InstanceArchiveList, InstanceArchivePurgeResult, InstanceArchiveRestoreResult,
    InstanceArchiveState, InstanceArchiveSummary, PendingInstanceDeletion,
};
mod instance_file_patch;
mod instance_isolation;
pub use ark_cluster_backups::{
    ArkClusterBackupMember, ArkClusterBackupRestoreResult, ArkClusterBackupSummary,
    ArkClusterRecoveryResult, PendingArkClusterRestore, create_ark_cluster_backup,
    ensure_ark_cluster_backup_ready, list_ark_cluster_backups, read_pending_ark_cluster_restore,
    recover_ark_cluster_restore, restore_ark_cluster_backup,
};
pub use ark_clusters::{
    ArkClusterIdentity, ArkClusterIssue, ArkClusterMember, ArkClusterReport,
    MAX_ARK_CLUSTER_INSTANCES, is_ark_module, read_ark_cluster_report,
};
mod instance_runtime_recovery;
mod workshop_collection_removal;
pub use workshop_collection_removal::remove_instance_workshop_collection;
#[cfg(test)]
mod test_file_snapshot;
pub use instance_isolation::{
    InstanceIsolationReport, InstancePathConflict, read_instance_isolation,
};
mod instance_workspace;
pub use instance_workspace::{
    InstanceWorkspace, InstanceWorkspaceDocument, InstanceWorkspaceEntry, InstanceWorkspacePage,
    open_instance_workspace,
};
mod instance_settings_lock;
mod instances;
pub mod managed_console_log;
mod migration_history;
mod player_access;
mod player_access_delimited;
mod player_access_normalization;
mod private_runtime;
mod private_runtime_refresh;
mod program_adoption;
mod program_adoption_recovery;
mod program_exclusive;
mod program_runtime;
pub use program_exclusive::{InstanceProgramCreationPlan, inspect_instance_program_creation};
mod program_instance_acquisition;
mod program_inventory;
mod program_library_cleanup;
mod program_library_retention;
pub use program_library_cleanup::{
    LibraryCleanupPlan, library_cleanup_retained_paths, plan_module_library_cleanup,
};
pub use program_library_retention::retain_library_program_source;
pub mod program_removal_files;
pub mod program_removals_db;
pub use program_adoption_recovery::recover_interrupted_program_adoptions;
pub use program_instance_acquisition::{
    is_instance_program_acquisition, new_instance_program_acquisition,
};
pub use program_inventory::{
    ModuleProgramInventory, ProgramInstallationSummary, inspect_module_programs,
};
mod program_detach;
mod program_seed;
pub use program_detach::detach_instance_program;
pub use program_seed::{
    CleanLibrarySeed, LibraryProgramAcquisition, library_program_acquisition_is_trusted,
    library_program_is_pristine, prepare_clean_library_seed, prepare_clean_library_seed_at,
    prepare_instance_program_seed_at, read_library_program_acquisition,
    record_library_program_baseline, restore_library_program_acquisition,
    retain_published_library_program_baseline, retain_verified_library_program_baseline,
};
mod program_install_records;
#[cfg(test)]
pub use program_install_records::adopt_library_install_for_instance;
pub use program_install_records::{
    InstanceProgramInstall, ProgramInstallRecord, ProgramInstallScope,
    bind_shared_install_to_instance, ensure_library_program_path_isolated,
    ensure_library_program_target_available, read_all_instance_program_installs,
    read_instance_program_install, read_library_program_install, read_module_instance_installs,
    read_program_install_owner, register_instance_install, sync_instance_game_install,
};
mod runtime;
mod runtime_log_diagnostics;
mod save_paths;
mod settings;
mod settings_validation;
mod settings_value_formats;
mod seven_days_ban;
pub use seven_days_ban::{SevenDaysBanReceipt, record_seven_days_bans_from_native};
mod storage_db;
mod storage_location;
mod storage_private_directory;
mod storage_usage;
#[cfg(windows)]
mod storage_volumes;
pub use storage_usage::{StorageUsageEntry, StorageUsageReport, scan_storage_usage};
mod templates;
mod windrose_bootstrap;
pub use windrose_bootstrap::{
    WindroseBootstrap, WindroseBootstrapObservation, prepare_windrose_bootstrap,
};

pub use a2s::{A2S_MAX_RESPONSE_BYTES, A2sClient, A2sQueryError};
pub use backups::{
    PreparedInstanceBackupRestore, create_instance_auto_stop_backup, create_instance_backup,
    create_instance_pre_restore_backup, delete_instance_backup, list_instance_backups,
    prepare_instance_backup_restore, rename_instance_backup, restore_instance_backup,
    restore_prepared_instance_backup, revalidate_prepared_instance_backup_restore,
};
pub use broadcast::{
    insert_instance_broadcast_event, list_instance_broadcast_events,
    read_instance_broadcast_policy, upsert_instance_broadcast_policy,
};
pub use install_save_paths::install_save_directory_prefix;
pub use instance_autostart::update_instance_autostart;
pub use instance_deletion::{archive_instance, delete_instance};
pub use instance_file_patch::{
    InstanceFileEdits, InstanceFileEditsPreview, InstanceFilePatchOutcome, InstanceFilePatchResult,
    InstanceFilePatchState, InstanceFilePatchesResult, InstanceFilePatchesStatus,
    InstancePatchFile, InstancePatchFileList, InstanceTextEdit, InstanceTextPatch,
    InstanceTextPatchPreview, PreparedInstanceFilePatches, PreparedInstanceTextPatch,
    apply_instance_file_patch, apply_instance_file_patches, list_instance_patch_files,
    prepare_instance_file_patch, prepare_instance_file_patches, read_instance_patch_file,
    validate_instance_file_edits,
};
pub use instance_runtime_recovery::recover_interrupted_instance_runtime;
mod instance_storage_paths;
pub use instance_storage_paths::read_instance_storage_paths;
pub use instances::{
    CreateInstanceResult, InstancePortProjection, MAX_INSTANCE_PORT_PROJECTION_INSTANCES,
    create_instance, create_instance_with_options, list_instances,
    materialize_instance_configuration, materialize_instance_configuration_for_start,
    normalize_complete_instance_settings, read_instance_details, read_instance_port_projections,
    update_instance, update_instance_if_current, update_instance_ports,
};
pub use player_access::{
    ApplyInstancePlayerAccessMutationInput, PlayerAccessMutationOperation,
    PlayerAccessPersistentMutationResult, PlayerAccessPersistentStatus, PlayerAccessSyncMetadata,
    PlayerAccessSyncMode, apply_instance_player_access_mutation,
};
pub use private_runtime::{
    InstanceCreationOptions, PrivateRuntimeProjection, resolve_instance_private_runtime_root,
};
pub use private_runtime_refresh::{PrivateRuntimeRefresh, refresh_private_runtime};
pub use program_runtime::{
    InstanceProgramMode, instance_program_mode, instance_uses_exclusive_program,
    instance_uses_library_program, resolve_instance_runtime_root,
};
pub use runtime::{
    GameLogDocument, GameLogSnapshot, QueriedPlayerCount, StartedInstanceProcess,
    WindroseNativeStage, describe_expected_player_query_binding, list_active_instance_runs,
    mark_instance_process_started_with_identity, mark_instance_process_stopped,
    normalize_query_host, parse_a2s_info_payload, parse_minecraft_query_basic_response,
    parse_minecraft_query_challenge_response, query_a2s_player_count, query_live_player_count,
    read_active_instance_run, read_game_log_snapshot, read_instance_game_log_document,
    read_instance_log_document, read_instance_restart_failure_count,
    read_instance_runtime_overview, read_log_path_snapshot, resolve_player_query_target,
    steam_player_query_visibility_restriction, supports_live_player_query_protocol,
    windrose_native_stage,
};
pub use runtime_log_diagnostics::runtime_fatal_log_lines;
pub use save_paths::{InstanceSavePathContext, plan_instance_saves_dir};
pub use settings::{bootstrap_storage, bootstrap_storage_with_paths, save_app_settings};
pub use storage_db::{
    initialize_database, read_game_install_last_verified_unix_ms, resolve_module_install_root,
    sync_game_installs, sync_modules,
};
pub use templates::dst_world_settings::{
    dst_world_setting_evidence_known, extract_dst_static_lua_table, inspect_dst_mod_enablement,
    validate_dst_mod_preservation, validate_dst_shard_mod_requirements,
};
pub use templates::projectzomboid_mod_reorder_preserves_ids;

static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");
const _: &str = env!("LANGAME_MIGRATIONS_FINGERPRINT");
static STORAGE_INIT_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct StoragePaths {
    pub app_data_root: PathBuf,
    pub settings_path: PathBuf,
    pub database_path: PathBuf,
    pub logs_root: PathBuf,
    pub modules_root: PathBuf,
    pub migrations_root: PathBuf,
    pub steamcmd_root: PathBuf,
    pub games_root: PathBuf,
    pub instances_root: PathBuf,
    pub archives_root: PathBuf,
}

#[derive(Debug, Clone)]
struct StoredInstanceRecord {
    summary: InstanceSummary,
    config_dir: PathBuf,
    saves_dir: PathBuf,
    auto_backup_on_stop: bool,
    backup_retention_count: u32,
    runtime_mode: String,
    program_install_root: Option<PathBuf>,
}

impl Default for StoragePaths {
    fn default() -> Self {
        let app_data_root = default_app_data_root();
        let runtime_root = default_runtime_root(&app_data_root);
        Self::from_data_roots(app_data_root, runtime_root)
    }
}

impl StoragePaths {
    /// Resolve the saved data location, selecting and preparing it only on first use.
    pub fn resolve_default() -> Result<Self, StorageError> {
        storage_location::resolve_default_paths()
    }

    /// Inspect an existing location without choosing a disk or creating files.
    pub fn resolve_existing_default() -> Result<Option<Self>, StorageError> {
        storage_location::existing_paths(&default_app_data_root())
    }

    fn from_data_roots(app_data_root: PathBuf, runtime_root: PathBuf) -> Self {
        let modules_root = default_modules_root();

        Self {
            app_data_root: app_data_root.clone(),
            settings_path: app_data_root.join("settings.json"),
            database_path: app_data_root.join("db").join("lgs.db"),
            logs_root: app_data_root.join("logs"),
            modules_root: modules_root.clone(),
            migrations_root: modules_root
                .parent()
                .map(|root| root.join("migrations"))
                .unwrap_or_else(|| app_data_root.join("migrations")),
            steamcmd_root: runtime_root.join("cmd").join("steamcmd"),
            games_root: runtime_root.join("server-files"),
            instances_root: runtime_root.join("instances"),
            archives_root: runtime_root.join("instances").join(".trash"),
        }
    }

    pub fn app_log_path(&self) -> PathBuf {
        self.logs_root.join("desktop-app").join("active.jsonl")
    }

    pub fn with_app_settings(&self, settings: &AppSettings) -> Self {
        Self {
            app_data_root: self.app_data_root.clone(),
            settings_path: self.settings_path.clone(),
            database_path: self.database_path.clone(),
            logs_root: self.logs_root.clone(),
            modules_root: self.modules_root.clone(),
            migrations_root: self.migrations_root.clone(),
            steamcmd_root: PathBuf::from(&settings.steamcmd_root),
            games_root: PathBuf::from(&settings.games_root),
            instances_root: PathBuf::from(&settings.servers_root),
            archives_root: if settings.archives_root.trim().is_empty() {
                PathBuf::from(&settings.servers_root).join(".trash")
            } else {
                PathBuf::from(&settings.archives_root)
            },
        }
    }

    pub fn settings(&self) -> AppSettings {
        AppSettings {
            servers_root: self.instances_root.to_string_lossy().into_owned(),
            archives_root: self.archives_root.to_string_lossy().into_owned(),
            games_root: self.games_root.to_string_lossy().into_owned(),
            modules_root: self.modules_root.to_string_lossy().into_owned(),
            steamcmd_root: self.steamcmd_root.to_string_lossy().into_owned(),
        }
    }

    pub fn probe_status(&self) -> StorageStatus {
        StorageStatus {
            database_path: self.database_path.to_string_lossy().into_owned(),
            migrations_path: self.migrations_root.to_string_lossy().into_owned(),
            app_log_path: self.app_log_path().to_string_lossy().into_owned(),
            database_exists: self.database_path.exists(),
            schema_version: 0,
            migrations_applied: false,
        }
    }
}

fn default_app_data_root() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("XDG_DATA_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| {
            env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .map(|root| root.join(".local").join("share"))
        })
        .or_else(|| {
            env::var_os("USERPROFILE")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .map(|root| root.join("AppData").join("Local"))
        })
        .unwrap_or_else(|| PathBuf::from(DEFAULT_LANGAME_DATA_ROOT).join("data"))
        .join("LanGame")
        .join("ServerManager")
}

fn default_runtime_root(app_data_root: &Path) -> PathBuf {
    resolve_runtime_root(Path::new(DEFAULT_LANGAME_DATA_ROOT), app_data_root)
}

fn resolve_runtime_root(managed_root: &Path, app_data_root: &Path) -> PathBuf {
    if managed_root.is_dir() {
        managed_root.to_path_buf()
    } else {
        app_data_root.join("runtime")
    }
}

fn default_modules_root() -> PathBuf {
    let executable = env::current_exe().ok();
    #[cfg(debug_assertions)]
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent);
    #[cfg(not(debug_assertions))]
    let repository = None;
    resolve_modules_root(executable.as_deref(), repository)
}

fn resolve_modules_root(executable: Option<&Path>, repository: Option<&Path>) -> PathBuf {
    let bundled_modules = executable
        .and_then(Path::parent)
        .map(|root| root.join("modules"));
    // Exported debug builds must keep using their frozen resources even when
    // the compiler's source-snapshot directory is replaced by another build.
    if let Some(modules) = bundled_modules.as_ref().filter(|path| path.is_dir()) {
        return modules.clone();
    }
    if let Some(repository_modules) = repository.map(|root| root.join("modules"))
        && repository_modules.is_dir()
    {
        return repository_modules;
    }
    bundled_modules.unwrap_or_else(|| PathBuf::from("modules"))
}

#[cfg(test)]
#[path = "modules_root_tests.rs"]
mod modules_root_tests;

#[derive(Debug, Clone)]
pub struct StorageBootstrap {
    pub paths: StoragePaths,
    pub settings: AppSettings,
    pub storage_status: StorageStatus,
}

#[derive(Debug, Clone)]
pub struct GameInstallSyncRecord {
    pub module_id: String,
    pub install_root: String,
    pub install_state: InstallState,
    pub current_version: Option<String>,
    pub mark_verified: bool,
}

#[derive(Debug, Clone)]
pub struct ActiveInstanceRunEntry {
    pub instance_id: String,
    pub run_id: i64,
    pub session_id: Option<String>,
    pub process_key: String,
    pub display_name: String,
    pub pid: Option<u32>,
    pub process_identity: Option<ProcessIdentity>,
    pub log_path: Option<String>,
    pub is_primary: bool,
}

#[derive(Debug, Clone)]
struct StoredInstanceRunRow {
    run_id: i64,
    session_id: Option<String>,
    process_key: String,
    display_name: String,
    pid: Option<u32>,
    process_identity: Option<ProcessIdentity>,
    status: String,
    started_at: Option<String>,
    stopped_at: Option<String>,
    exit_code: Option<i32>,
    crash_flag: bool,
    log_path: Option<String>,
    is_primary: bool,
}

#[derive(Debug, Default, Clone, Copy)]
struct DirectoryCopyStats {
    file_count: usize,
    total_bytes: u64,
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("failed to create path {path}: {source}")]
    CreatePath {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read directory {path}: {source}")]
    ReadDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to inspect path {path}: {source}")]
    ReadPath {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read config file {path}: {source}")]
    ReadConfig {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write config file {path}: {source}")]
    WriteConfig {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to sync file {path}: {source}")]
    SyncFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to move path from {from} to {to}: {source}")]
    MovePath {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "failed to publish instance runtime from {from} to {to} after {attempts} attempt(s) in {elapsed_ms} ms; check file locks and directory permissions: {source}"
    )]
    PublishInstanceRuntime {
        from: PathBuf,
        to: PathBuf,
        attempts: u32,
        elapsed_ms: u128,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to copy path from {from} to {to}: {source}")]
    CopyPath {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("module `{module_id}` support materialization failed for {path}: {message}")]
    ModuleSupportMaterialization {
        module_id: String,
        path: PathBuf,
        message: String,
    },
    #[error("failed to delete path {path}: {source}")]
    DeletePath {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("refusing to manage destructive operation for path {path} outside {root}")]
    UnsafeManagedPath { path: PathBuf, root: PathBuf },
    #[error("invalid private runtime projection path {path}: {message}")]
    InvalidPrivateRuntimeProjection { path: PathBuf, message: String },
    #[error("a clean official program package is required before creating an instance from {path}")]
    CleanLibraryProgramRequired { path: PathBuf },
    #[error("Windrose bootstrap configuration failed at {path}: {message}")]
    WindroseBootstrap { path: PathBuf, message: String },
    #[error("private runtime refresh failed at {path}: {message}")]
    PrivateRuntimeRefresh { path: PathBuf, message: String },
    #[error(
        "instance {instance_id} {kind} path {path} overlaps instance {other_instance_id} path {other_path}"
    )]
    InstancePathConflict {
        instance_id: String,
        other_instance_id: String,
        kind: String,
        path: Box<std::path::Path>,
        other_path: Box<std::path::Path>,
    },
    #[error("invalid instance path {path}: {message}")]
    InvalidInstancePath { path: PathBuf, message: String },
    #[error("storage telemetry supports at most {max} instances; path coverage is incomplete")]
    StorageTelemetryInstanceLimit { max: usize },
    #[error("database schema mismatch for {table}: missing columns {missing_columns:?}")]
    SchemaMismatch {
        table: String,
        missing_columns: Vec<String>,
    },
    #[error("database file changed while initialization was in progress: {path}")]
    DatabaseIdentityChanged { path: PathBuf },
    #[error("failed to discover modules: {0}")]
    ModuleDiscovery(#[from] app_modules::ModuleDiscoveryError),
    #[error("failed to run database migration: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("database migration history is incompatible: {reason}")]
    IncompatibleMigrationHistory { reason: String },
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("invalid config json: {0}")]
    ConfigJson(#[from] serde_json::Error),
    #[error("invalid config json in {path}: {source}")]
    InvalidConfigJson {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("instance `{id}` was not found")]
    MissingInstance { id: String },
    #[error("backup `{backup_id}` for `{instance_id}` was not found")]
    MissingBackup {
        instance_id: String,
        backup_id: String,
    },
    #[error("backup id `{backup_id}` is not a single safe path component")]
    InvalidBackupId { backup_id: String },
    #[error("instance run `{run_id}` for `{instance_id}` was not found")]
    MissingInstanceRun { instance_id: String, run_id: i64 },
    #[error("game log for instance `{instance_id}`, run `{run_id}` is unavailable: {reason}")]
    InvalidGameLogSource {
        instance_id: String,
        run_id: i64,
        reason: String,
    },
    #[error("settings_json must be a JSON object")]
    InvalidSettingsRoot,
    #[error("module `{module_id}` settings field `{field}` is invalid: {message}")]
    InvalidModuleSetting {
        module_id: String,
        field: String,
        message: String,
    },
    #[error(
        "instance settings are locked by another process at {path}; retry after the active operation finishes"
    )]
    InstanceSettingsLocked { path: PathBuf },
    #[error(
        "instance `{id}` settings changed while this edit was pending; reload the server settings and retry"
    )]
    InstanceSettingsPreconditionFailed { id: String },
    #[error(
        "instance `{id}` is running; stop it before changing its bind address or port assignments"
    )]
    ActiveInstanceNetworkMutation { id: String },
    #[error("instance `{id}` is active; stop the server before deleting the instance")]
    ActiveInstanceDeletion { id: String },
    #[error("failed to acquire the instance settings lock at {path}: {source}")]
    InstanceSettingsLock {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "failed to acquire the instance creation lock for module `{module_id}` at {path}: {source}"
    )]
    InstanceCreationLock {
        module_id: String,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("instance creation was cancelled; incomplete instance files were removed")]
    InstanceCreationCancelled,
    #[error("{operation} task failed: {message}")]
    BlockingTaskFailed {
        operation: &'static str,
        message: String,
    },
    #[error(
        "instance creation failed ({creation_error}); failed to remove incomplete instance directory {path}: {cleanup_error}"
    )]
    InstanceCreationRollback {
        path: PathBuf,
        creation_error: String,
        cleanup_error: String,
    },
    #[error("duplicate port {protocol}/{port} in save request")]
    DuplicatePortInRequest { protocol: String, port: u16 },
    #[error("duplicate port binding name `{name}` in save request")]
    DuplicatePortNameInRequest { name: String },
    #[error("port group `{group_id}` is invalid for this request: {message}")]
    InvalidPortGroupRequest { group_id: String, message: String },
    #[error("port binding `{name}` was not assigned during allocation")]
    PortBindingNotAllocated { name: String },
    #[error("instance port projection accepts at most {max} instances, got {actual}")]
    InstancePortProjectionLimitExceeded { max: usize, actual: usize },
    #[error("instance port projection exceeded its bounded row limit of {max}")]
    InstancePortProjectionRowLimitExceeded { max: usize },
    #[error("instance `{instance_id}` has invalid stored port `{name}` value {port}")]
    InvalidStoredInstancePort {
        instance_id: String,
        name: String,
        port: i64,
    },
    #[error("port {protocol}/{port} is already used by another instance")]
    PortConflict { protocol: String, port: u16 },
    #[error("no available {protocol} port starting from {start_port}")]
    PortAllocationExhausted { protocol: String, start_port: u16 },
    #[error("module `{module_id}` resolved an invalid saves_path_template: {template}")]
    InvalidModuleSavePathTemplate { module_id: String, template: String },
}

#[cfg(test)]
mod tests;
