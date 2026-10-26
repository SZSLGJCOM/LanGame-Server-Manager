use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use app_core::{
    AppSettings, InstallSource, InstallSpec, InstallState, MinecraftJavaInstallSpec, ModuleDetails,
    ProcessSpec,
};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::mpsc;

mod archive_install;
mod discovery;
mod dst_workshop_cache;
mod dst_workshop_cache_fs;
mod dst_workshop_cache_publish;
mod dst_workshop_cache_vdf;
mod executable_resolution;
mod http_download;
mod install_cancellation;
mod install_operation;
mod install_process;
mod install_progress;
mod install_publication_metadata;
mod install_resources;
mod install_transaction;
mod minecraft_files;
mod minecraft_install;
mod minecraft_java;
mod minecraft_metadata;
mod module_install;
mod ownership;
mod package_revision;
mod portable_runtime;
mod program_update_check;
mod retained_install_data;
mod steam_app_access;
pub mod steam_depot;
mod steam_install_retained;
mod steamcmd_account;
mod steamcmd_bootstrap_log;
mod steamcmd_content_evidence;
mod steamcmd_manifest_recovery;
mod steamcmd_prepare;
mod steamcmd_prepare_process;
mod steamcmd_readiness;
mod steamcmd_runtime_evidence;
mod steamcmd_stream;
mod steamcmd_update_bridge;
mod steamcmd_update_cache;
mod steamcmd_update_manifest;
mod windows_runtime_catalog;
mod windows_runtime_setup;
mod workshop_cache;
mod workshop_cache_integrity;
mod workshop_cache_transaction;
mod workshop_deployment_baseline;
mod workshop_instance_membership;
mod workshop_paths;

use archive_install::{DirectDownloadInstallRequest, install_or_update_module_from_download};
use discovery::{configured_steamcmd_root, require_steamcmd_ready};
pub use discovery::{managed_steamcmd_status, steamcmd_executable_path, steamcmd_status};
pub use dst_workshop_cache::{
    DstWorkshopCacheDeployment, DstWorkshopCacheError, deploy_dst_workshop_cache,
    deploy_verified_dst_workshop_cache,
};
use executable_resolution::{normalized_relative_path, resolve_probe_process_executable};
use http_download::{DownloadIntegrity, fetch_json, http_client};
pub use install_cancellation::InstallCancellation;
pub use install_operation::InstallLockContext;
#[cfg(test)]
use install_operation::test_install_lock_context;
use install_operation::{InstallAcquireError, InstallDeadline, InstallDeadlineElapsed};
use install_process::{
    AbortOnDropTask, run_command_capture, run_powershell, spawn_managed_command,
    terminate_and_reap_child,
};
pub use install_progress::InstallProgressUpdate;
use install_resources::ResourceLocks;
#[cfg(test)]
use install_transaction::direct_download_publish_script;
use install_transaction::{
    await_failed_directory_cleanup, finish_published_jre, recover_direct_download_install,
    restore_published_directory, schedule_verified_directory_cleanup,
};
#[cfg(test)]
use minecraft_files::write_minecraft_server_file_atomically_with;
use minecraft_files::{
    create_minecraft_staging_file, publish_minecraft_staging_file, sha1_file_hex,
    write_minecraft_server_file_atomically,
};
use minecraft_install::install_or_update_minecraft_java_module;
use minecraft_java::ensure_minecraft_jre;
#[cfg(test)]
use module_install::steamcmd_install_script_lines;
pub use module_install::{
    ModuleInstallCallbacks, SteamModuleInstallRequest, install_or_update_module_at_with_callbacks,
    install_or_update_module_at_with_login_and_callbacks,
    install_or_update_module_at_with_progress_and_cancellation,
};
pub use ownership::SteamCmdOwnership;
use ownership::{prepare_configured_steamcmd_root, validate_steamcmd_ownership};
use package_revision::{begin_game_install_revision, complete_game_install_revision};
pub use package_revision::{read_game_install_revision, read_program_install_revision};
/// Prepare versioned startup runtimes without replacing installed game files.
pub use portable_runtime::{PortableRuntime, prepare_portable_runtime};
pub use program_update_check::current_program_version;
pub use retained_install_data::{
    RETAINED_INSTALL_DATA_MARKER, has_retained_install_data, mark_retained_install_data,
};
pub use steam_depot::{
    SteamPackageVerificationOptions, VerifiedSteamPackage, verify_installed_steam_package,
    verify_installed_steam_package_with_progress,
};
pub use steamcmd_account::{
    SteamWorkshopAccountAuthenticationResult, SteamWorkshopAccountInput, SteamWorkshopAccountPhase,
    SteamWorkshopAccountProgress, authenticate_workshop_account_with_progress,
    normalize_workshop_account_name,
};
pub use steamcmd_prepare::{
    SteamCmdPreparePhase, SteamCmdPrepareProgress, ensure_steamcmd_installed,
    ensure_steamcmd_installed_with_progress,
    ensure_steamcmd_installed_with_progress_and_cancellation,
};
#[cfg(test)]
use steamcmd_stream::{
    finish_bounded_output_line, format_command_failure_excerpt, read_steamcmd_content_log_excerpt,
    spawn_line_forwarder, steamcmd_output_is_retryable_file_lock, updated_content_log_excerpt,
};
use steamcmd_stream::{run_steamcmd_script_with_progress, steamcmd_failure_context};
/// Verify and prepare official Windows prerequisites before game creation.
pub use windows_runtime_setup::prepare_windows_runtime_dependencies;
pub use workshop_cache_integrity::record_validated_workshop_items;
pub use workshop_cache_transaction::{
    VerifiedWorkshopCache, WorkshopCacheScope, WorkshopCacheTransaction,
    WorkshopDeploymentProtection,
};
pub use workshop_deployment_baseline::WorkshopDeploymentBaseline;
#[cfg(test)]
use workshop_paths::inspect_workshop_item_paths;
use workshop_paths::resolve_workshop_content_roots;
pub use workshop_paths::{inspect_workshop_items, inspect_workshop_items_with_dst_ugc_roots};
mod workshop_download;

const MINECRAFT_VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
// Each budget starts before the resource locks are acquired, so queueing,
// SteamCMD self-update retries, downloads, and verification share one deadline.
const MODULE_INSTALL_TIMEOUT: Duration = Duration::from_secs(6 * 60 * 60);
const WORKSHOP_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);
const STEAMCMD_UNINSTALL_TIMEOUT: Duration = Duration::from_secs(2 * 60);
const GAME_INSTALL_LIFECYCLE_TIMEOUT: Duration = Duration::from_secs(2 * 60);
const CHILD_REAP_TIMEOUT: Duration = Duration::from_secs(5);
const STEAMCMD_OUTPUT_CHANNEL_CAPACITY: usize = 64;
const STEAMCMD_OUTPUT_LINE_LIMIT_BYTES: usize = 4 * 1024;
const STEAMCMD_OUTPUT_READ_CHUNK_BYTES: usize = 4 * 1024;
const STEAMCMD_OUTPUT_LINE_TRUNCATED_SUFFIX: &str = " [line truncated]";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;
#[cfg(all(windows, test))]
const DETACHED_PROCESS: u32 = 0x00000008;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SteamCmdSource {
    Configured,
    Discovered,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteamCmdStatus {
    pub root: String,
    pub executable_path: String,
    pub executable_exists: bool,
    pub ready: bool,
    pub configured_root: String,
    pub configured_executable_path: String,
    pub source: SteamCmdSource,
    pub ownership: SteamCmdOwnership,
    pub can_uninstall: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInstallProbe {
    pub module_id: String,
    pub install_root: String,
    pub executable_path: String,
    pub executable_exists: bool,
    pub install_state: InstallState,
    pub current_version: Option<String>,
    pub steam_manifest: Option<SteamAppManifestProbe>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteamAppManifestProbe {
    pub path: String,
    pub app_id: u32,
    pub name: Option<String>,
    pub install_dir: Option<String>,
    pub state_flags: Option<u64>,
    pub build_id: Option<String>,
    pub target_build_id: Option<String>,
    pub bytes_to_download: Option<u64>,
    pub bytes_downloaded: Option<u64>,
    pub bytes_to_stage: Option<u64>,
    pub bytes_staged: Option<u64>,
    pub downloading_path_exists: bool,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInstallResult {
    pub module_id: String,
    pub steam_app_id: u32,
    pub operation: String,
    pub install_root: String,
    pub executable_path: String,
    pub executable_exists: bool,
    pub install_state: InstallState,
    pub current_version: Option<String>,
    pub output_excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteamWorkshopDownloadItemResult {
    pub item_id: String,
    pub expected_path: String,
    pub expected_path_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteamWorkshopDownloadResult {
    pub consumer_app_id: u32,
    pub install_root: String,
    pub workshop_root: String,
    pub items: Vec<SteamWorkshopDownloadItemResult>,
    pub output_excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteamWorkshopInstallationItemStatus {
    pub item_id: String,
    pub path: String,
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteamWorkshopInstallationSnapshot {
    pub consumer_app_id: u32,
    pub searched_roots: Vec<String>,
    pub items: Vec<SteamWorkshopInstallationItemStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MinecraftInstallMetadata {
    version_id: String,
    version_type: String,
    manifest_url: String,
    version_url: String,
    server_url: String,
    server_sha1: String,
    server_size: u64,
    server_jar: String,
    #[serde(default)]
    required_java_major: u16,
    downloaded_at_unix_ms: u128,
}

#[derive(Debug, Deserialize)]
struct MinecraftVersionManifest {
    latest: MinecraftLatestVersions,
    versions: Vec<MinecraftVersionManifestEntry>,
}

#[derive(Debug, Deserialize)]
struct MinecraftLatestVersions {
    release: String,
    snapshot: String,
}

#[derive(Debug, Deserialize)]
struct MinecraftVersionManifestEntry {
    id: String,
    #[serde(rename = "type")]
    version_type: String,
    url: String,
    #[serde(deserialize_with = "minecraft_metadata::deserialize_sha1")]
    sha1: String,
}

#[derive(Debug, Deserialize)]
struct MinecraftVersionDetails {
    id: String,
    downloads: MinecraftVersionDownloads,
    #[serde(rename = "javaVersion")]
    java_version: MinecraftJavaVersion,
}

#[derive(Debug, Deserialize)]
struct MinecraftJavaVersion {
    #[serde(rename = "majorVersion")]
    major_version: u16,
}

#[derive(Debug, Deserialize)]
struct MinecraftVersionDownloads {
    server: Option<MinecraftDownloadDescriptor>,
}

#[derive(Debug, Deserialize)]
struct MinecraftDownloadDescriptor {
    sha1: String,
    size: u64,
    url: String,
}

#[derive(Debug, Error)]
pub enum SteamCmdError {
    #[error("{operation} was cancelled")]
    InstallCancelled { operation: String },
    #[error("failed to stop {operation}: {detail}")]
    InstallProcessCleanupFailed { operation: String, detail: String },
    #[error("failed to update SteamCMD readiness record {path}: {source}")]
    SteamCmdReadinessIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("module `{module_id}` does not declare a managed install source")]
    MissingInstallSource { module_id: String },
    #[error("module `{module_id}` does not declare an [install] section")]
    MissingInstallSpec { module_id: String },
    #[error("module `{module_id}` does not declare a [process] section")]
    MissingProcessSpec { module_id: String },
    #[error("failed to create path {path}: {source}")]
    CreatePath {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read or write game install revision at {path}: {source}")]
    PackageRevisionIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "failed to read or write game install revision at {path}: the last game install did not finish; validate or update this program installation before starting it"
    )]
    PackageRevisionPending { path: PathBuf },
    #[error("failed to download or extract SteamCMD: {output_excerpt}")]
    PrepareSteamCmd { output_excerpt: String },
    #[error(
        "SteamCMD preparation produced no output for {timeout_seconds} seconds; check access to Steam download services and retry. Last output: {output_excerpt}"
    )]
    SteamCmdPreparationStalled {
        timeout_seconds: u64,
        output_excerpt: String,
    },
    #[error(
        "SteamCMD preparation timed out after {timeout_seconds} seconds. Last output: {output_excerpt}"
    )]
    SteamCmdPreparationTimedOut {
        timeout_seconds: u64,
        output_excerpt: String,
    },
    #[error("failed to stop the SteamCMD preparation process tree: {output_excerpt}")]
    SteamCmdPreparationCleanupFailed { output_excerpt: String },
    #[error(
        "failed to inspect the SteamCMD preparation log: {source}. Last output: {output_excerpt}"
    )]
    SteamCmdPreparationLogRead {
        #[source]
        source: std::io::Error,
        output_excerpt: String,
    },
    #[error("failed to download and extract module payload: {output_excerpt}")]
    DirectDownloadFailed { output_excerpt: String },
    #[error("failed to download {url}: {detail}")]
    HttpDownload {
        url: String,
        detail: String,
        permits_source_fallback: bool,
        source_unavailable: bool,
        permits_ip_family_retry: bool,
    },
    #[error("download response was interrupted: {detail}")]
    HttpDownloadInterrupted { detail: String },
    #[error("failed to restore the previous install at {path}: {detail}")]
    InstallRollbackFailed { path: PathBuf, detail: String },
    #[error("Minecraft version `{version}` was not found in the Mojang manifest")]
    MinecraftVersionNotFound { version: String },
    #[error("Minecraft version `{version}` does not publish a vanilla server.jar download")]
    MinecraftServerJarUnavailable { version: String },
    #[error("Minecraft Java runtime preparation failed: {detail}")]
    MinecraftJrePreparation { detail: String },
    #[error("Server runtime preparation failed: {detail}")]
    RuntimePreparation { detail: String },
    #[error("failed to write installation file {path}: {source}")]
    WriteMinecraftServerFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "failed to replace Minecraft server file {path}: {source}; restoring backup {backup_path} also failed: {rollback_source}"
    )]
    MinecraftServerFileRollbackFailed {
        path: PathBuf,
        backup_path: PathBuf,
        #[source]
        source: std::io::Error,
        rollback_source: std::io::Error,
    },
    #[error("module `{module_id}` declares an unsafe install-relative path: {path}")]
    InvalidInstallRelativePath { module_id: String, path: String },
    #[error("SteamCMD executable was not found after extraction: {path}")]
    MissingSteamCmdExecutable { path: String },
    #[error("SteamCMD is not ready at {path}. Check or install SteamCMD on the System page first.")]
    SteamCmdNotReady { path: String },
    #[error("refusing to remove unowned SteamCMD root {path}")]
    UnmanagedSteamCmdRoot { path: PathBuf },
    #[error("failed to inspect SteamCMD ownership at {path}: {source}")]
    SteamCmdOwnershipIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("SteamCMD ownership marker is invalid at {path}")]
    InvalidSteamCmdOwnership { path: PathBuf },
    #[error("failed to {action} at {path}: {source}")]
    InstallOperationLock {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write SteamCMD script {path}: {source}")]
    WriteScript {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to remove staged game install directory {path}: {source}")]
    RemoveGameInstallPath {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "refusing unsafe staged game install removal: published path {published_path}, staged path {staged_path}"
    )]
    UnsafeGameInstallRemovalPath {
        published_path: PathBuf,
        staged_path: PathBuf,
    },
    #[error("failed to spawn SteamCMD or PowerShell: {source}")]
    SpawnCommand {
        #[source]
        source: std::io::Error,
    },
    #[error("{operation} timed out after {timeout_seconds} seconds")]
    OperationTimedOut {
        operation: &'static str,
        timeout_seconds: u64,
    },
    #[error("no valid Steam Workshop item IDs were provided")]
    MissingWorkshopItemIds,
    #[error("failed to inspect Workshop directory {path}: {source}")]
    WorkshopInventoryRead {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("SteamCMD command failed: {output_excerpt}")]
    SteamCmdCommandFailed { output_excerpt: String },
    #[error(
        "Steam app {app_id} requires a Steam account with access to the requested server files: {output_excerpt}"
    )]
    SteamAppAccountRequired { app_id: u32, output_excerpt: String },
    #[error(
        "Steam account name must contain 1 to 64 ASCII letters, digits, underscores, dots, at signs or hyphens, and cannot be anonymous"
    )]
    InvalidSteamAccountName,
    #[error("Steam account authentication failed: {output_excerpt}")]
    SteamAccountAuthenticationFailed { output_excerpt: String },
    #[error("interactive Steam account authentication requires Windows")]
    SteamAccountInteractiveUnsupported,
    #[error(
        "Steam Workshop app {consumer_app_id} download is missing a decryption key in the current Steam session: {output_excerpt}"
    )]
    WorkshopDecryptionKeyMissing {
        consumer_app_id: u32,
        output_excerpt: String,
    },
    #[error(
        "Steam Workshop app {consumer_app_id} requires a Steam account with access to the requested content: {output_excerpt}"
    )]
    WorkshopAccountRequired {
        consumer_app_id: u32,
        output_excerpt: String,
    },
    #[error(
        "module `{module_id}` finished `{operation}` but expected executable is still missing: {path}"
    )]
    MissingInstalledExecutable {
        module_id: String,
        operation: String,
        path: String,
    },
    #[error(
        "module `{module_id}` finished `{operation}` but install verification failed: {detail}"
    )]
    InstallationVerificationFailed {
        module_id: String,
        operation: String,
        detail: String,
    },
}

/// Holds process-wide and cross-process locks for one module and its resource
/// trees. Unrelated modules with disjoint trees can run concurrently.
///
/// Callers must finish all filesystem and persistence work for one game-file
/// lifecycle transition before dropping this guard.
#[must_use = "dropping this guard releases the game install lifecycle lock"]
pub struct GameInstallLifecycleGuard(
    std::sync::Arc<ResourceLocks>,
    std::collections::BTreeSet<String>,
);

pub async fn acquire_game_install_lifecycle(
    lock_context: &InstallLockContext,
    module_id: &str,
    program_roots: &[PathBuf],
) -> Result<GameInstallLifecycleGuard, SteamCmdError> {
    let deadline = InstallDeadline::new(
        "game server install lifecycle change",
        GAME_INSTALL_LIFECYCLE_TIMEOUT,
    );
    acquire_game_lifecycle(lock_context, module_id, program_roots, deadline).await
}

async fn acquire_game_lifecycle(
    lock_context: &InstallLockContext,
    module_id: &str,
    roots: &[PathBuf],
    deadline: InstallDeadline,
) -> Result<GameInstallLifecycleGuard, SteamCmdError> {
    Ok(GameInstallLifecycleGuard(
        std::sync::Arc::new(
            ResourceLocks::acquire(lock_context, &[module_id], roots, deadline).await?,
        ),
        std::iter::once(module_id.to_owned()).collect(),
    ))
}

/// Acquire a recovery operation's complete resource union in one order. This
/// avoids self-deadlock when different modules reference overlapping save roots.
pub async fn acquire_game_install_lifecycles(
    lock_context: &InstallLockContext,
    resources: &[(String, Vec<PathBuf>)],
) -> Result<GameInstallLifecycleGuard, SteamCmdError> {
    let modules: Vec<&str> = resources
        .iter()
        .map(|(module, _)| module.as_str())
        .collect();
    let roots: Vec<PathBuf> = resources
        .iter()
        .flat_map(|(_, roots)| roots.iter().cloned())
        .collect();
    let deadline = InstallDeadline::new(
        "game server recovery lifecycle change",
        GAME_INSTALL_LIFECYCLE_TIMEOUT,
    );
    Ok(GameInstallLifecycleGuard(
        std::sync::Arc::new(
            ResourceLocks::acquire(lock_context, &modules, &roots, deadline).await?,
        ),
        modules.into_iter().map(str::to_owned).collect(),
    ))
}

impl GameInstallLifecycleGuard {
    pub fn ensure_scope(&self, module_id: &str, root: &Path) -> Result<(), SteamCmdError> {
        if !self.1.contains(module_id) {
            return Err(install_resources::scope_error(
                root,
                "lifecycle lease belongs to another module",
            ));
        }
        self.0.ensure_root(root)
    }

    pub async fn acquire_steamcmd(
        &self,
        settings: &AppSettings,
    ) -> Result<SteamCmdLifecycleGuard, SteamCmdError> {
        self.acquire_steamcmd_with_deadline(
            settings,
            InstallDeadline::new("SteamCMD lifecycle change", GAME_INSTALL_LIFECYCLE_TIMEOUT),
        )
        .await
    }

    async fn acquire_steamcmd_with_deadline(
        &self,
        settings: &AppSettings,
        deadline: InstallDeadline,
    ) -> Result<SteamCmdLifecycleGuard, SteamCmdError> {
        let roots = steamcmd_resource_roots(settings);
        for root in &roots {
            self.0.ensure_disjoint(root)?;
        }
        acquire_steamcmd_roots(self.0.context(), settings, &roots, deadline).await
    }

    /// Removes a directory already detached from its published install path.
    /// The lifecycle lock is moved into the blocking worker, so cancellation of
    /// the async caller cannot let a new install race an in-flight deletion.
    pub async fn remove_staged_directory(
        self,
        published_path: PathBuf,
        path: PathBuf,
    ) -> Result<(), SteamCmdError> {
        self.0.ensure_root(&published_path)?;
        validate_staged_game_install_removal_path(&published_path, &path)?;
        let worker_path = path.clone();
        tokio::task::spawn_blocking(move || {
            let _operation = self.0;
            fs::remove_dir_all(&worker_path).map_err(|source| {
                SteamCmdError::RemoveGameInstallPath {
                    path: worker_path,
                    source,
                }
            })
        })
        .await
        .map_err(|source| SteamCmdError::RemoveGameInstallPath {
            path,
            source: std::io::Error::other(format!("game install deletion worker failed: {source}")),
        })?
    }
}

/// Protects SteamCMD's executable, update state and shared Workshop cache.
/// Cache consumers retain it until their final inspection/copy has finished.
#[must_use = "dropping this guard releases the SteamCMD runtime locks"]
pub struct SteamCmdLifecycleGuard(ResourceLocks);

async fn acquire_steamcmd_operation(
    lock_context: &InstallLockContext,
    settings: &AppSettings,
    deadline: InstallDeadline,
) -> Result<SteamCmdLifecycleGuard, SteamCmdError> {
    acquire_steamcmd_roots(
        lock_context,
        settings,
        &steamcmd_resource_roots(settings),
        deadline,
    )
    .await
}

fn steamcmd_resource_roots(settings: &AppSettings) -> [PathBuf; 2] {
    [
        configured_steamcmd_root(settings),
        PathBuf::from(steamcmd_status(settings).root),
    ]
}

async fn acquire_steamcmd_roots(
    lock_context: &InstallLockContext,
    settings: &AppSettings,
    roots: &[PathBuf],
    deadline: InstallDeadline,
) -> Result<SteamCmdLifecycleGuard, SteamCmdError> {
    let guard =
        SteamCmdLifecycleGuard(ResourceLocks::acquire(lock_context, &[], roots, deadline).await?);
    // Discovery may change while queued; never use a newly selected runtime
    // outside the roots actually covered by this lease.
    guard.ensure_settings(settings)?;
    Ok(guard)
}

impl SteamCmdLifecycleGuard {
    fn ensure_settings(&self, settings: &AppSettings) -> Result<(), SteamCmdError> {
        self.0.ensure_root(&configured_steamcmd_root(settings))?;
        self.0
            .ensure_root(Path::new(&steamcmd_status(settings).root))
    }

    async fn require_ready_with_progress<F: FnMut(SteamCmdPrepareProgress)>(
        &self,
        settings: &AppSettings,
        deadline: InstallDeadline,
        on_progress: F,
    ) -> Result<SteamCmdStatus, SteamCmdError> {
        self.ensure_settings(settings)?;
        steamcmd_prepare::require_ready_locked(settings, deadline, on_progress).await
    }
}

fn validate_staged_game_install_removal_path(
    published_path: &Path,
    staged_path: &Path,
) -> Result<(), SteamCmdError> {
    let published_name = published_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty());
    let staged_name = staged_path.file_name().and_then(|name| name.to_str());
    let has_expected_name =
        published_name
            .zip(staged_name)
            .is_some_and(|(published_name, staged_name)| {
                let prefix = format!(".{published_name}.uninstall-");
                staged_name
                    .strip_prefix(&prefix)
                    .is_some_and(|suffix| !suffix.is_empty())
            });
    if published_path == staged_path
        || published_path.parent().is_none()
        || published_path.parent() != staged_path.parent()
        || !has_expected_name
    {
        return Err(SteamCmdError::UnsafeGameInstallRemovalPath {
            published_path: published_path.to_path_buf(),
            staged_path: staged_path.to_path_buf(),
        });
    }
    Ok(())
}

pub async fn remove_managed_steamcmd(
    lock_context: &InstallLockContext,
    settings: &AppSettings,
) -> Result<SteamCmdStatus, SteamCmdError> {
    let deadline = InstallDeadline::new("SteamCMD uninstall", STEAMCMD_UNINSTALL_TIMEOUT);
    let operation = acquire_steamcmd_operation(lock_context, settings, deadline).await?;
    let root = configured_steamcmd_root(settings);
    if !root.exists() {
        return Ok(steamcmd_status(settings));
    }
    let owned_root = validate_steamcmd_ownership(&root)?;
    let deletion_root = owned_root.clone();
    let deletion = tokio::task::spawn_blocking(move || {
        // The guard remains in the blocking task even if its caller reaches the
        // deadline. That prevents a new install from racing a deletion that the
        // operating system cannot safely cancel halfway through.
        let _operation = operation;
        fs::remove_dir_all(&deletion_root).map_err(|source| SteamCmdError::SteamCmdOwnershipIo {
            path: deletion_root,
            source,
        })
    });
    deadline
        .run(deletion)
        .await
        .map_err(|InstallDeadlineElapsed| operation_timeout(deadline))?
        .map_err(|source| SteamCmdError::SteamCmdOwnershipIo {
            path: owned_root.clone(),
            source: std::io::Error::other(format!("SteamCMD deletion worker failed: {source}")),
        })??;
    Ok(steamcmd_status(settings))
}

fn operation_timeout(deadline: InstallDeadline) -> SteamCmdError {
    if InstallCancellation::current().is_some_and(|token| token.is_cancelled()) {
        return SteamCmdError::InstallCancelled {
            operation: deadline.operation().to_owned(),
        };
    }
    SteamCmdError::OperationTimedOut {
        operation: deadline.operation(),
        timeout_seconds: deadline.timeout().as_secs(),
    }
}

fn install_acquire_error(error: InstallAcquireError, deadline: InstallDeadline) -> SteamCmdError {
    match error {
        InstallAcquireError::Deadline => operation_timeout(deadline),
        InstallAcquireError::LockFile {
            action,
            path,
            source,
        } => SteamCmdError::InstallOperationLock {
            action,
            path,
            source,
        },
    }
}

pub fn probe_module_install_state(
    settings: &AppSettings,
    module_id: &str,
    steam_app_id: Option<u32>,
    install: Option<&InstallSpec>,
    process: Option<&ProcessSpec>,
) -> ModuleInstallProbe {
    probe_module_install_state_with_override(
        settings,
        module_id,
        steam_app_id,
        install,
        process,
        None,
    )
}

pub fn probe_module_install_state_with_override(
    settings: &AppSettings,
    module_id: &str,
    steam_app_id: Option<u32>,
    install: Option<&InstallSpec>,
    process: Option<&ProcessSpec>,
    install_root_override: Option<&str>,
) -> ModuleInstallProbe {
    let install_root = install_root_override
        .map(PathBuf::from)
        .or_else(|| {
            install.map(|spec| PathBuf::from(&settings.games_root).join(&spec.shared_game_dir))
        })
        .unwrap_or_else(|| PathBuf::from(&settings.games_root).join(module_id));

    let verification_path = install.and_then(|spec| install_verification_path(&install_root, spec));
    let executable_path = match (install, process) {
        (Some(_), Some(process_spec)) => resolve_probe_process_executable(
            &install_root,
            process_spec,
            verification_path.as_deref(),
        ),
        _ => PathBuf::new(),
    };

    let executable_exists = !executable_path.as_os_str().is_empty() && executable_path.exists();
    let verification_exists = verification_path.as_ref().map(|path| path.exists());
    let steam_manifest = steam_app_id
        .filter(|app_id| *app_id > 0)
        .and_then(|app_id| probe_steam_app_manifest(&install_root, app_id));
    let current_version = install
        .filter(|spec| is_minecraft_java_install(spec))
        .and_then(|_| read_minecraft_install_metadata(&install_root).ok())
        .map(|metadata| metadata.version_id)
        .or_else(|| {
            steam_manifest
                .as_ref()
                .and_then(|manifest| manifest.build_id.clone())
                .filter(|version| version != "0")
        });
    let diagnostics = install_probe_diagnostics(
        &install_root,
        &executable_path,
        executable_exists,
        verification_path.as_deref(),
        verification_exists,
        steam_app_id,
        steam_manifest.as_ref(),
    );
    let install_state = derive_install_state(
        &install_root,
        executable_exists,
        verification_exists,
        steam_manifest.as_ref(),
    );

    ModuleInstallProbe {
        module_id: String::from(module_id),
        install_root: install_root.to_string_lossy().into_owned(),
        executable_path: executable_path.to_string_lossy().into_owned(),
        executable_exists,
        install_state,
        current_version,
        steam_manifest,
        diagnostics,
    }
}

fn derive_install_state(
    install_root: &Path,
    executable_exists: bool,
    verification_exists: Option<bool>,
    steam_manifest: Option<&SteamAppManifestProbe>,
) -> InstallState {
    if !install_root.exists() {
        return InstallState::NotInstalled;
    }

    if let Some(manifest) = steam_manifest
        && !manifest.complete
    {
        return InstallState::Incomplete;
    }

    if verification_exists.unwrap_or(executable_exists) {
        InstallState::Installed
    } else if steam_manifest.is_none()
        && !executable_exists
        && has_retained_install_data(install_root)
    {
        InstallState::NotInstalled
    } else if install_root.exists() {
        InstallState::Corrupted
    } else {
        InstallState::NotInstalled
    }
}

fn install_probe_diagnostics(
    install_root: &Path,
    executable_path: &Path,
    executable_exists: bool,
    verification_path: Option<&Path>,
    verification_exists: Option<bool>,
    steam_app_id: Option<u32>,
    steam_manifest: Option<&SteamAppManifestProbe>,
) -> Vec<String> {
    let mut diagnostics = Vec::new();

    if !install_root.exists() {
        diagnostics.push(format!(
            "Install root does not exist: {}",
            install_root.display()
        ));
        return diagnostics;
    }

    match (steam_app_id, steam_manifest) {
        (Some(app_id), Some(manifest)) if !manifest.complete => {
            diagnostics.push(format!(
                "Steam AppID {app_id} manifest is present but the payload is still incomplete."
            ));
        }
        (Some(app_id), Some(_)) => {
            diagnostics.push(format!("Steam AppID {app_id} manifest is present."));
        }
        (Some(app_id), None) if app_id > 0 => {
            diagnostics.push(format!(
                "Steam AppID {app_id} manifest was not found under {}.",
                install_root.join("steamapps").display()
            ));
        }
        _ => {}
    }

    if let Some(path) = verification_path {
        if verification_exists.unwrap_or(false) {
            diagnostics.push(format!(
                "Required server file is present: {}.",
                path.display()
            ));
        } else {
            diagnostics.push(format!(
                "Required server file is missing: {}.",
                path.display()
            ));
        }
    }

    if executable_exists {
        diagnostics.push(format!(
            "Expected server executable is present: {}.",
            executable_path.display()
        ));
    } else if !executable_path.as_os_str().is_empty() {
        diagnostics.push(format!(
            "Expected server executable is missing: {}.",
            executable_path.display()
        ));
    }

    diagnostics
}

fn is_minecraft_java_install(install: &InstallSpec) -> bool {
    matches!(install.source, Some(InstallSource::MinecraftJava)) || install.minecraft.is_some()
}

fn install_verification_path(install_root: &Path, install: &InstallSpec) -> Option<PathBuf> {
    let raw_path = install.verification_path.as_deref()?.trim();
    if raw_path.is_empty() {
        return None;
    }

    safe_install_relative_path(raw_path).map(|path| install_root.join(path))
}

fn safe_install_relative_path(raw_path: &str) -> Option<PathBuf> {
    let path = normalized_relative_path(raw_path);
    if path.as_os_str().is_empty() || path.is_absolute() {
        return None;
    }

    if path.components().any(|component| {
        matches!(
            component,
            Component::Prefix(_) | Component::RootDir | Component::ParentDir
        )
    }) {
        return None;
    }

    Some(path)
}

fn resolve_install_relative_path(
    module_id: &str,
    install_root: &Path,
    raw_path: &str,
) -> Result<PathBuf, SteamCmdError> {
    safe_install_relative_path(raw_path)
        .map(|path| install_root.join(path))
        .ok_or_else(|| SteamCmdError::InvalidInstallRelativePath {
            module_id: String::from(module_id),
            path: String::from(raw_path),
        })
}

fn minecraft_metadata_path(install_root: &Path) -> PathBuf {
    install_root.join(".langame").join("minecraft-server.json")
}

fn read_minecraft_install_metadata(
    install_root: &Path,
) -> Result<MinecraftInstallMetadata, SteamCmdError> {
    let path = minecraft_metadata_path(install_root);
    let text =
        fs::read_to_string(&path).map_err(|source| SteamCmdError::WriteMinecraftServerFile {
            path: path.clone(),
            source,
        })?;
    serde_json::from_str(&text).map_err(|source| SteamCmdError::WriteMinecraftServerFile {
        path,
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, source),
    })
}

fn probe_steam_app_manifest(install_root: &Path, app_id: u32) -> Option<SteamAppManifestProbe> {
    let path = install_root
        .join("steamapps")
        .join(format!("appmanifest_{app_id}.acf"));
    let text = fs::read_to_string(&path).ok()?;
    let fields = parse_steam_app_manifest_fields(&text);
    let downloading_path = install_root
        .join("steamapps")
        .join("downloading")
        .join(app_id.to_string());
    let downloading_path_exists = downloading_path_has_pending_payload(&downloading_path);
    let bytes_to_download = parse_u64_field(&fields, "BytesToDownload");
    let bytes_downloaded = parse_u64_field(&fields, "BytesDownloaded");
    let bytes_to_stage = parse_u64_field(&fields, "BytesToStage");
    let bytes_staged = parse_u64_field(&fields, "BytesStaged");
    let state_flags = parse_u64_field(&fields, "StateFlags");
    let complete = steam_manifest_is_complete(
        state_flags,
        bytes_to_download,
        bytes_downloaded,
        bytes_to_stage,
        bytes_staged,
        downloading_path_exists,
    ) && steam_depot::manifest::parse_acf_app_state(text.as_bytes(), app_id)
        .and_then(|app| {
            module_install::shared_dependencies::unfinished_shared_depots(
                install_root,
                &app,
                None,
                || Ok(()),
            )
        })
        .is_ok_and(|unfinished| unfinished.is_empty());

    Some(SteamAppManifestProbe {
        path: path.to_string_lossy().into_owned(),
        app_id,
        name: fields.get("name").cloned(),
        install_dir: fields.get("installdir").cloned(),
        state_flags,
        build_id: fields.get("buildid").cloned(),
        target_build_id: fields.get("TargetBuildID").cloned(),
        bytes_to_download,
        bytes_downloaded,
        bytes_to_stage,
        bytes_staged,
        downloading_path_exists,
        complete,
    })
}

fn steam_manifest_is_complete(
    state_flags: Option<u64>,
    bytes_to_download: Option<u64>,
    bytes_downloaded: Option<u64>,
    bytes_to_stage: Option<u64>,
    bytes_staged: Option<u64>,
    downloading_path_has_pending_payload: bool,
) -> bool {
    if downloading_path_has_pending_payload
        && (bytes_to_download.unwrap_or(0) > 0 || bytes_to_stage.unwrap_or(0) > 0)
    {
        return false;
    }

    if let Some(state_flags) = state_flags
        && state_flags & 4 == 0
    {
        return false;
    }

    if known_incomplete_byte_progress(bytes_to_download, bytes_downloaded) {
        return false;
    }

    if known_incomplete_byte_progress(bytes_to_stage, bytes_staged) {
        return false;
    }

    true
}

fn downloading_path_has_pending_payload(downloading_path: &Path) -> bool {
    match fs::read_dir(downloading_path) {
        Ok(mut entries) => matches!(entries.next(), Some(Ok(_))),
        Err(_) => false,
    }
}

fn known_incomplete_byte_progress(expected: Option<u64>, actual: Option<u64>) -> bool {
    matches!((expected, actual), (Some(expected), Some(actual)) if expected > 0 && actual < expected)
}

fn parse_u64_field(fields: &HashMap<String, String>, key: &str) -> Option<u64> {
    fields.get(key).and_then(|value| value.parse::<u64>().ok())
}

fn parse_steam_app_manifest_fields(text: &str) -> HashMap<String, String> {
    let mut fields = HashMap::new();

    for line in text.lines() {
        if let Some((key, value)) = parse_quoted_key_value_line(line) {
            fields.insert(key, value);
        }
    }

    fields
}

fn parse_quoted_key_value_line(line: &str) -> Option<(String, String)> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut escaped = false;

    for character in line.trim().chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }

        if character == '\\' && in_quote {
            escaped = true;
            continue;
        }

        if character == '"' {
            if in_quote {
                values.push(current.clone());
                current.clear();
            }
            in_quote = !in_quote;
            continue;
        }

        if in_quote {
            current.push(character);
        }
    }

    if values.len() == 2 {
        Some((values.remove(0), values.remove(0)))
    } else {
        None
    }
}

pub struct SteamWorkshopDownloadRequest<'a> {
    pub consumer_app_id: u32,
    pub install_root: &'a Path,
    pub ids: &'a [String],
    pub account_name: Option<&'a str>,
}

/// Cancels runtime verification and downloading through the same owned-process
/// cleanup used by installation. The caller must await this operation after
/// signalling cancellation so SteamCMD and its descendants are reaped before
/// releasing their lifecycle leases.
pub async fn download_workshop_items_with_login_and_cancellation_and_progress<F>(
    settings: &AppSettings,
    request: SteamWorkshopDownloadRequest<'_>,
    guard: &GameInstallLifecycleGuard,
    steamcmd_guard: &SteamCmdLifecycleGuard,
    cancellation: &InstallCancellation,
    on_progress: F,
) -> Result<SteamWorkshopDownloadResult, SteamCmdError>
where
    F: FnMut(InstallProgressUpdate),
{
    cancellation
        .scope(download_workshop_items_with_login_and_progress(
            settings,
            request,
            guard,
            steamcmd_guard,
            on_progress,
        ))
        .await
}

/// Uses only an explicitly selected, previously authenticated Steam account.
/// Cached credentials are owned by SteamCMD; passwords never enter this API.
/// Every requested item runs native `workshop_download_item ... validate`, even
/// when the local cache is already complete, so Steam checks its current version.
pub async fn download_workshop_items_with_login_and_progress<F>(
    settings: &AppSettings,
    request: SteamWorkshopDownloadRequest<'_>,
    guard: &GameInstallLifecycleGuard,
    steamcmd_guard: &SteamCmdLifecycleGuard,
    mut on_progress: F,
) -> Result<SteamWorkshopDownloadResult, SteamCmdError>
where
    F: FnMut(InstallProgressUpdate),
{
    let SteamWorkshopDownloadRequest {
        consumer_app_id,
        install_root,
        ids,
        account_name,
    } = request;
    let deadline = InstallDeadline::new("Steam Workshop download", WORKSHOP_DOWNLOAD_TIMEOUT);
    let account_name = account_name
        .map(normalize_workshop_account_name)
        .transpose()?;
    let login_line = steamcmd_account::workshop_login_line(account_name.as_deref());
    guard.0.ensure_root(install_root)?;
    steamcmd_guard.ensure_settings(settings)?;
    let item_ids = normalize_steam_workshop_item_ids(ids);
    if item_ids.is_empty() {
        return Err(SteamCmdError::MissingWorkshopItemIds);
    }
    let steamcmd = steamcmd_guard
        .require_ready_with_progress(settings, deadline, |update| {
            on_progress(InstallProgressUpdate::preparing_steamcmd(update));
        })
        .await?;
    on_progress(InstallProgressUpdate {
        install_progress: None,
        progress_percent: 1.0,
        detail: String::from("Preparing Steam Workshop download..."),
        output_excerpt: String::new(),
    });
    fs::create_dir_all(install_root).map_err(|source| SteamCmdError::CreatePath {
        path: install_root.to_path_buf(),
        source,
    })?;

    on_progress(InstallProgressUpdate {
        install_progress: None,
        progress_percent: 4.0,
        detail: String::from("Checking SteamCMD runtime..."),
        output_excerpt: String::new(),
    });
    on_progress(InstallProgressUpdate {
        install_progress: None,
        progress_percent: 14.0,
        detail: format!("SteamCMD ready: {}", steamcmd.executable_path),
        output_excerpt: String::new(),
    });

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let script_root = steamcmd_script_root();
    fs::create_dir_all(&script_root).map_err(|source| SteamCmdError::CreatePath {
        path: script_root.clone(),
        source,
    })?;
    let script_path = script_root.join(format!("steamcmd-workshop-{consumer_app_id}-{stamp}.txt"));

    let mut script_lines = vec![
        String::from("@ShutdownOnFailedCommand 1"),
        String::from("@NoPromptForPassword 1"),
        format!("force_install_dir {}", steamcmd_script_path(install_root)),
        login_line,
    ];
    for item_id in &item_ids {
        script_lines.push(format!(
            "workshop_download_item {consumer_app_id} {item_id} validate"
        ));
    }
    script_lines.push(String::from("quit"));

    fs::write(&script_path, script_lines.join("\n")).map_err(|source| {
        SteamCmdError::WriteScript {
            path: script_path.clone(),
            source,
        }
    })?;

    on_progress(InstallProgressUpdate {
        install_progress: None,
        progress_percent: 18.0,
        detail: format!(
            "Starting SteamCMD Workshop download for {} item(s)...",
            item_ids.len()
        ),
        output_excerpt: String::new(),
    });

    let mut stream_progress = 18.0_f32;
    let output = match run_steamcmd_script_with_progress(
        &steamcmd.executable_path,
        &script_path,
        &steamcmd.root,
        deadline,
        |update| {
            if update.progress_percent > 0.0 {
                let mapped = 22.0 + (update.progress_percent * 0.72);
                stream_progress = stream_progress.max(mapped.min(96.0));
            } else {
                stream_progress = (stream_progress + 0.3).min(96.0);
            }

            on_progress(InstallProgressUpdate {
                install_progress: None,
                progress_percent: stream_progress,
                detail: update.detail,
                output_excerpt: update.output_excerpt,
            });
        },
    )
    .await
    {
        Ok(output) => output,
        Err(error) => {
            let _ = fs::remove_file(&script_path);
            return Err(error);
        }
    };

    let _ = fs::remove_file(&script_path);

    workshop_download::verify_command_result(consumer_app_id, account_name.as_deref(), &output)?;

    let (workshop_root, alternate_workshop_root) =
        resolve_workshop_content_roots(install_root, Path::new(&steamcmd.root), consumer_app_id);
    let items = workshop_cache::inspect_downloaded_items(
        consumer_app_id,
        &[workshop_root.clone(), alternate_workshop_root],
        item_ids,
        &output.workshop_command_evidence,
    )?;
    verify_downloaded_workshop_items(&items, &workshop_download::failure_context(&output))
        .map_err(|error| match error {
            SteamCmdError::SteamCmdCommandFailed { output_excerpt } => {
                workshop_download::command_error(consumer_app_id, &output, output_excerpt)
            }
            other => other,
        })?;
    let workshop_root = items
        .first()
        .and_then(|item| Path::new(&item.expected_path).parent())
        .map(Path::to_path_buf)
        .unwrap_or(workshop_root);
    let excerpt = output.excerpt;

    on_progress(InstallProgressUpdate {
        install_progress: None,
        progress_percent: 100.0,
        detail: format!(
            "Workshop download complete under {}",
            workshop_root.display()
        ),
        output_excerpt: excerpt.clone(),
    });

    Ok(SteamWorkshopDownloadResult {
        consumer_app_id,
        install_root: install_root.to_string_lossy().into_owned(),
        workshop_root: workshop_root.to_string_lossy().into_owned(),
        items,
        output_excerpt: excerpt,
    })
}

fn verify_downloaded_workshop_items(
    items: &[SteamWorkshopDownloadItemResult],
    output_excerpt: &str,
) -> Result<(), SteamCmdError> {
    if output_excerpt
        .lines()
        .any(workshop_download::is_workshop_download_failure_line)
    {
        return Err(SteamCmdError::SteamCmdCommandFailed {
            output_excerpt: output_excerpt.to_owned(),
        });
    }
    let missing_ids = items
        .iter()
        .filter(|item| !item.expected_path_exists)
        .map(|item| item.item_id.as_str())
        .collect::<Vec<_>>();
    if missing_ids.is_empty() {
        return Ok(());
    }
    Err(SteamCmdError::SteamCmdCommandFailed {
        output_excerpt: format!(
            "SteamCMD did not materialize Workshop items: {}.\n{}",
            missing_ids.join(", "),
            output_excerpt
        ),
    })
}

fn normalize_steam_workshop_item_ids(ids: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for raw_id in ids {
        for matched in raw_id
            .split(|character: char| !character.is_ascii_digit())
            .map(str::trim)
            .filter(|entry| {
                entry.len() >= 6 && entry.chars().all(|character| character.is_ascii_digit())
            })
        {
            if seen.insert(matched.to_owned()) {
                normalized.push(matched.to_owned());
            }
        }
    }

    normalized
}

fn capitalize_operation(value: &str) -> String {
    let mut characters = value.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

fn steamcmd_script_path(path: &Path) -> String {
    format!("\"{}\"", command_path_string(path).replace('\\', "/"))
}

fn steamcmd_script_root() -> PathBuf {
    env::temp_dir().join("LanGame").join("steamcmd-scripts")
}

#[cfg(windows)]
fn command_path_string(path: &Path) -> String {
    let raw = path.to_string_lossy();
    if let Some(stripped) = raw.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{stripped}");
    }
    if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        return stripped.to_string();
    }
    raw.into_owned()
}

#[cfg(not(windows))]
fn command_path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn ps_literal(path: &Path) -> String {
    let escaped = command_path_string(path).replace('\'', "''");
    format!("'{}'", escaped)
}

#[cfg(windows)]
fn apply_no_window(command: &mut Command) {
    command.creation_flags(hidden_child_creation_flags());
}

#[cfg(windows)]
fn hidden_child_creation_flags() -> u32 {
    CREATE_NO_WINDOW
}

#[cfg(not(windows))]
fn apply_no_window(_command: &mut Command) {}

fn output_excerpt(stdout: &[u8], stderr: &[u8]) -> String {
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    );

    let lines = combined
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();

    let start = lines.len().saturating_sub(40);
    let excerpt = lines[start..].join("\n");
    if excerpt.is_empty() {
        String::from("Installer produced no additional output.")
    } else {
        excerpt
    }
}

#[cfg(test)]
mod dst_workshop_cache_tests;
#[cfg(test)]
mod install_cancellation_safety_tests;
#[cfg(test)]
#[path = "steamcmd_tests.rs"]
mod tests;
#[cfg(all(windows, test))]
mod workshop_download_cancellation_tests;
