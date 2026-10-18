//! Preserve both SteamCMD-managed cache locations before native in-place writes.
//! Rollback is explicit and checked; uncertain recovery data remains on disk.
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::steam_depot;
use crate::workshop_cache_integrity::{self as integrity, Files};
use crate::{InstallCancellation, SteamWorkshopDownloadItemResult, SteamWorkshopDownloadResult};

static NEXT: AtomicU64 = AtomicU64::new(1);

/// Optional semantic source choices supplied by the game adapter. A choice can
/// only select this request's IDs within the two protected managed cache roots.
#[derive(Clone, Copy)]
pub struct WorkshopCacheScope<'a> {
    pub consumer_app_id: u32,
    pub install_root: &'a Path,
    pub ids: &'a [String],
    pub fallback_sources: Option<&'a BTreeMap<String, Vec<PathBuf>>>,
}

#[derive(Debug, Clone)]
struct VerifiedItem {
    id: String,
    path: PathBuf,
    files: Files,
}

#[derive(Debug, Clone)]
pub struct VerifiedWorkshopCache {
    result: SteamWorkshopDownloadResult,
    items: Vec<VerifiedItem>,
}

/// A complete original deployment containing additional operator/game files.
/// These files have no known replacement policy, so publication may only leave
/// this exact deployment in place when the native package itself is unchanged.
#[derive(Debug, Clone)]
pub struct WorkshopDeploymentProtection {
    id: String,
    path: PathBuf,
    files: Files,
    payload: Option<Files>,
}

impl WorkshopDeploymentProtection {
    pub fn item_id(&self) -> &str {
        &self.id
    }
    pub fn verify(&self, path: &Path, cancellation: &InstallCancellation) -> io::Result<()> {
        if path != self.path || integrity::inventory(path, cancellation)? != self.files {
            return Err(steam_depot::invalid(
                "original Workshop deployment or operator data changed; replacement refused",
            ));
        }
        Ok(())
    }
    pub fn payload_unchanged(
        &self,
        source: &Path,
        cancellation: &InstallCancellation,
    ) -> io::Result<bool> {
        match &self.payload {
            Some(payload) => Ok(integrity::inventory(source, cancellation)? == *payload),
            None => Ok(false),
        }
    }
}

impl VerifiedWorkshopCache {
    pub fn download_result(&self) -> &SteamWorkshopDownloadResult {
        &self.result
    }

    pub fn protect_deployed_item(
        &self,
        id: &str,
        path: &Path,
        cancellation: &InstallCancellation,
    ) -> io::Result<Option<WorkshopDeploymentProtection>> {
        self.protect_deployed_item_with_retained_files(id, path, |_| false, |_| false, cancellation)
    }

    pub fn protect_deployed_item_with_retained_files(
        &self,
        id: &str,
        path: &Path,
        retain: fn(&str) -> bool,
        reject_unowned: fn(&str) -> bool,
        cancellation: &InstallCancellation,
    ) -> io::Result<Option<WorkshopDeploymentProtection>> {
        let item = self
            .items
            .iter()
            .find(|item| item.id == id)
            .ok_or_else(|| steam_depot::invalid("unrequested Workshop deployment"))?;
        let files = match integrity::inventory_allow_empty(path, cancellation) {
            Ok(files) => files,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        if files
            .keys()
            .any(|name| !item.files.contains_key(name) && reject_unowned(name))
        {
            return Err(steam_depot::invalid(
                "Workshop loading root contains an unknown loadable payload outside the validated package; automatic replacement and cached startup refused, existing files preserved",
            ));
        }
        if files.iter().any(|(name, digest)| {
            retain(name) && item.files.get(name).is_some_and(|old| old != digest)
        }) {
            return Err(steam_depot::invalid(
                "operator configuration supplied by the Mod was modified; automatic replacement refused to preserve its bytes",
            ));
        }
        if files
            .keys()
            .all(|name| item.files.contains_key(name) && !retain(name))
        {
            return Ok(None);
        }
        if item
            .files
            .iter()
            .any(|(name, digest)| files.get(name) != Some(digest))
        {
            return Err(steam_depot::invalid(
                "Workshop deployment contains additional operator data and its payload is incomplete or damaged; automatic root replacement refused to preserve those files",
            ));
        }
        Ok(Some(WorkshopDeploymentProtection {
            id: id.into(),
            path: path.into(),
            files,
            payload: Some(item.files.clone()),
        }))
    }

    /// The actual native deployment must match the old, certified package too.
    pub fn verify_deployed_item(
        &self,
        id: &str,
        path: &Path,
        cancellation: &InstallCancellation,
    ) -> io::Result<()> {
        let item = self
            .items
            .iter()
            .find(|item| item.id == id)
            .ok_or_else(|| steam_depot::invalid("unrequested Workshop deployment"))?;
        if integrity::inventory(path, cancellation)? != item.files {
            return Err(steam_depot::invalid(
                "deployed Workshop package is missing or differs from verified cache",
            ));
        }
        Ok(())
    }

    fn recheck(&self) -> io::Result<()> {
        let cancellation = InstallCancellation::new();
        for item in &self.items {
            self.verify_deployed_item(&item.id, &item.path, &cancellation)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
enum Saved {
    Directory(Files),
    File(Vec<u8>),
    Missing,
}

#[derive(Debug)]
struct Entry {
    path: PathBuf,
    backup: PathBuf,
    saved: Saved,
}

/// Holds only requested cache packages and their per-app ACFs. Neither game
/// configuration, saves, credentials, nor disabled package payloads are backed up
/// or changed. The caller retains both lifecycle leases through every method.
pub struct WorkshopCacheTransaction {
    stage: PathBuf,
    stages: Vec<PathBuf>,
    entries: Vec<Entry>,
    fallback: Option<VerifiedWorkshopCache>,
}

impl WorkshopCacheTransaction {
    pub fn verified_cache(&self) -> Option<&VerifiedWorkshopCache> {
        self.fallback.as_ref()
    }

    /// No integrity proof can authorize treating an unknown original extra file
    /// as disposable payload. This only blocks publication; it never certifies
    /// fallback or prevents a native repairing download from being attempted.
    pub fn protect_uncertified_deployment(
        &self,
        id: &str,
        path: &Path,
        cancellation: &InstallCancellation,
    ) -> io::Result<Option<WorkshopDeploymentProtection>> {
        self.protect_uncertified_deployment_with_retained_files(id, path, |_| false, cancellation)
    }

    pub fn protect_uncertified_deployment_with_retained_files(
        &self,
        id: &str,
        path: &Path,
        retain: fn(&str) -> bool,
        cancellation: &InstallCancellation,
    ) -> io::Result<Option<WorkshopDeploymentProtection>> {
        let mut cache_names = std::collections::BTreeSet::new();
        let mut requested = false;
        for entry in &self.entries {
            if entry.path.file_name().and_then(|name| name.to_str()) == Some(id) {
                requested = true;
                if let Saved::Directory(files) = &entry.saved {
                    cache_names.extend(files.keys().cloned());
                }
            }
        }
        if !requested {
            return Err(steam_depot::invalid("unrequested Workshop deployment"));
        }
        let files = match integrity::inventory_allow_empty(path, cancellation) {
            Ok(files) => files,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        if files
            .keys()
            .all(|name| cache_names.contains(name) && !retain(name))
        {
            return Ok(None);
        }
        Ok(Some(WorkshopDeploymentProtection {
            id: id.into(),
            path: path.into(),
            files,
            payload: None,
        }))
    }
    /// Read-only proof used when a backup cannot be allocated before any native
    /// update has run. Caller must still verify actual deployment before starting.
    pub fn verify_existing(
        settings: &app_core::AppSettings,
        app: u32,
        install: &Path,
        ids: &[String],
        guard: &crate::GameInstallLifecycleGuard,
        steamcmd: &crate::SteamCmdLifecycleGuard,
        cancellation: &InstallCancellation,
    ) -> Result<Option<VerifiedWorkshopCache>, crate::SteamCmdError> {
        Self::verify_scoped(
            settings,
            WorkshopCacheScope {
                consumer_app_id: app,
                install_root: install,
                ids,
                fallback_sources: None,
            },
            guard,
            steamcmd,
            cancellation,
        )
    }

    pub fn verify_scoped(
        settings: &app_core::AppSettings,
        scope: WorkshopCacheScope<'_>,
        guard: &crate::GameInstallLifecycleGuard,
        steamcmd: &crate::SteamCmdLifecycleGuard,
        cancellation: &InstallCancellation,
    ) -> Result<Option<VerifiedWorkshopCache>, crate::SteamCmdError> {
        let (native, roots, ids) = scoped_context(settings, &scope, guard, steamcmd)?;
        verify_existing_cache(
            scope.consumer_app_id,
            scope.install_root,
            &native,
            &roots,
            &ids,
            cancellation,
            scope.fallback_sources,
        )
        .map_err(|source| crate::SteamCmdError::WorkshopInventoryRead {
            path: scope.install_root.to_path_buf(),
            source,
        })
    }

    pub fn instance_candidate_ids(
        settings: &app_core::AppSettings,
        app: u32,
        install: &Path,
        guard: &crate::GameInstallLifecycleGuard,
        steamcmd: &crate::SteamCmdLifecycleGuard,
        cancellation: &InstallCancellation,
    ) -> Result<Vec<String>, crate::SteamCmdError> {
        guard.0.ensure_root(install)?;
        steamcmd.ensure_settings(settings)?;
        crate::workshop_instance_membership::instance_candidate_ids(install, app, cancellation)
            .map_err(|source| crate::SteamCmdError::WorkshopInventoryRead {
                path: install.to_path_buf(),
                source,
            })
    }
    pub fn prepare(
        settings: &app_core::AppSettings,
        app: u32,
        install: &Path,
        ids: &[String],
        guard: &crate::GameInstallLifecycleGuard,
        steamcmd: &crate::SteamCmdLifecycleGuard,
        cancellation: &InstallCancellation,
    ) -> Result<Self, crate::SteamCmdError> {
        Self::prepare_scoped(
            settings,
            WorkshopCacheScope {
                consumer_app_id: app,
                install_root: install,
                ids,
                fallback_sources: None,
            },
            guard,
            steamcmd,
            cancellation,
        )
    }

    pub fn prepare_scoped(
        settings: &app_core::AppSettings,
        scope: WorkshopCacheScope<'_>,
        guard: &crate::GameInstallLifecycleGuard,
        steamcmd: &crate::SteamCmdLifecycleGuard,
        cancellation: &InstallCancellation,
    ) -> Result<Self, crate::SteamCmdError> {
        let (native, roots, ids) = scoped_context(settings, &scope, guard, steamcmd)?;
        Self::capture_scoped(
            WorkshopCacheScope { ids: &ids, ..scope },
            &native,
            &roots,
            cancellation,
        )
        .map_err(|source| crate::SteamCmdError::WorkshopInventoryRead {
            path: scope.install_root.to_path_buf(),
            source,
        })
    }

    #[cfg(test)]
    fn capture(
        app: u32,
        install: &Path,
        steamcmd: &Path,
        roots: &[PathBuf],
        ids: &[String],
        cancellation: &InstallCancellation,
    ) -> io::Result<Self> {
        Self::capture_scoped(
            WorkshopCacheScope {
                consumer_app_id: app,
                install_root: install,
                ids,
                fallback_sources: None,
            },
            steamcmd,
            roots,
            cancellation,
        )
    }

    fn capture_scoped(
        scope: WorkshopCacheScope<'_>,
        steamcmd: &Path,
        roots: &[PathBuf],
        cancellation: &InstallCancellation,
    ) -> io::Result<Self> {
        let app = scope.consumer_app_id;
        let install = scope.install_root;
        let ids = scope.ids;
        steam_depot::check_plain(install, true)?;
        require_no_unconfirmed_recovery(install)?;
        let fallback = verify_existing_cache(
            app,
            install,
            steamcmd,
            roots,
            ids,
            cancellation,
            scope.fallback_sources,
        )?;
        let stage = install.join(format!(
            ".langame-workshop-recovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&stage)?;
        let native_stage = steamcmd.join(format!(
            ".langame-workshop-recovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let native_stage_result = crate::dst_workshop_cache_fs::ensure_plain_directory(steamcmd)
            .map_err(cache_fs_error)
            .and_then(|_| fs::create_dir(&native_stage));
        if let Err(error) = native_stage_result {
            let _ = fs::remove_dir(&stage);
            return Err(error);
        }
        let mut result = Self {
            // Remove the secondary stage first so the coordinator/index remains
            // available if cleanup fails. Every live-path rename stays on its volume.
            stages: vec![native_stage.clone(), stage.clone()],
            stage,
            entries: Vec::new(),
            fallback,
        };
        let capture_result: io::Result<()> = (|| {
            let mut paths = std::collections::HashSet::new();
            for root in roots {
                for path in ids
                    .iter()
                    .map(|id| root.join(id))
                    .chain(
                        ids.iter()
                            .map(|id| integrity::receipt_path(root, app, id))
                            .collect::<io::Result<Vec<_>>>()?,
                    )
                    .chain(std::iter::once(
                        root.parent()
                            .and_then(Path::parent)
                            .ok_or_else(|| steam_depot::invalid("invalid Workshop root"))?
                            .join(format!("appworkshop_{app}.acf")),
                    ))
                {
                    if paths.insert(path.clone()) {
                        let local_stage = if path.starts_with(install) {
                            result.stage.clone()
                        } else if path.starts_with(steamcmd) {
                            native_stage.clone()
                        } else {
                            return Err(steam_depot::invalid("unmanaged Workshop recovery target"));
                        };
                        result.save(path, &local_stage, cancellation)?;
                    }
                }
            }
            for id in ids {
                let path = crate::workshop_instance_membership::membership_path(install, app, id)?;
                if paths.insert(path.clone()) {
                    result.save(path, &result.stage.clone(), cancellation)?;
                }
            }
            let index = result
                .entries
                .iter()
                .map(|entry| serde_json::json!({"target":entry.path,"backup":entry.backup}))
                .collect::<Vec<_>>();
            fs::write(
                result.stage.join("recovery.json"),
                serde_json::to_vec(&index).map_err(steam_depot::invalid)?,
            )?;
            Ok(())
        })();
        if let Err(error) = capture_result {
            // Native mutation has not started, so failed preparation needs no
            // recovery copies. Do not strand the disk-full copies on every retry.
            if let Err(cleanup) = result.cleanup() {
                return Err(io::Error::other(format!(
                    "Workshop backup preparation failed: {error}; cleanup failed at {}: {cleanup}",
                    result.stage.display()
                )));
            }
            return Err(error);
        }
        Ok(result)
    }

    fn save(
        &mut self,
        path: PathBuf,
        local_stage: &Path,
        cancellation: &InstallCancellation,
    ) -> io::Result<()> {
        let backup = local_stage.join(self.entries.len().to_string());
        let saved = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {
                let files = integrity::inventory_allow_empty(&path, cancellation)?;
                fs::create_dir(&backup)?;
                crate::dst_workshop_cache_fs::copy_tree(&path, &backup, 0)
                    .map_err(cache_fs_error)?;
                if integrity::inventory_allow_empty(&backup, cancellation)? != files {
                    return Err(steam_depot::invalid(
                        "Workshop backup copy differs from source",
                    ));
                }
                Saved::Directory(files)
            }
            Ok(metadata) => {
                let bytes = steam_depot::bounded(&path, 64 * 1024 * 1024)?;
                fs::write(&backup, &bytes)?;
                fs::set_permissions(&backup, metadata.permissions())?;
                Saved::File(bytes)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                plain_existing_ancestors(&path)?;
                Saved::Missing
            }
            Err(error) => return Err(error),
        };
        self.entries.push(Entry {
            path,
            backup,
            saved,
        });
        Ok(())
    }

    /// Restore even when cancelled, after the native child has exited. Recovery
    /// cannot be cancelled halfway through and then release the protected paths.
    pub fn rollback(&mut self) -> io::Result<Option<VerifiedWorkshopCache>> {
        for index in 0..self.entries.len() {
            let entry = &self.entries[index];
            plain_existing_ancestors(&entry.path)?;
            match &entry.saved {
                Saved::Directory(files)
                    if integrity::inventory_allow_empty(
                        &entry.backup,
                        &InstallCancellation::new(),
                    )? != *files =>
                {
                    return Err(steam_depot::invalid("preserved Workshop backup changed"));
                }
                Saved::File(bytes)
                    if steam_depot::bounded(&entry.backup, 64 * 1024 * 1024)? != *bytes =>
                {
                    return Err(steam_depot::invalid("preserved Workshop metadata changed"));
                }
                _ => {}
            }
        }
        for index in 0..self.entries.len() {
            let entry = &self.entries[index];
            match fs::symlink_metadata(&entry.path) {
                Ok(metadata) => {
                    steam_depot::check_plain(&entry.path, metadata.is_dir())?;
                    let local_stage = entry
                        .backup
                        .parent()
                        .ok_or_else(|| steam_depot::invalid("Workshop backup has no parent"))?;
                    fs::rename(&entry.path, local_stage.join(format!("failed-{index}")))?;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            if !matches!(entry.saved, Saved::Missing) {
                crate::dst_workshop_cache_fs::ensure_plain_directory(
                    entry
                        .path
                        .parent()
                        .ok_or_else(|| steam_depot::invalid("Workshop target has no parent"))?,
                )
                .map_err(cache_fs_error)?;
                fs::rename(&entry.backup, &entry.path)?;
            }
            match &entry.saved {
                Saved::Directory(files)
                    if integrity::inventory_allow_empty(
                        &entry.path,
                        &InstallCancellation::new(),
                    )? != *files =>
                {
                    return Err(steam_depot::invalid(
                        "restored Workshop package differs from backup",
                    ));
                }
                Saved::File(bytes)
                    if steam_depot::bounded(&entry.path, 64 * 1024 * 1024)? != *bytes =>
                {
                    return Err(steam_depot::invalid(
                        "restored Workshop manifest differs from backup",
                    ));
                }
                Saved::Missing if entry.path.try_exists()? => {
                    return Err(steam_depot::invalid(
                        "partial Workshop package remained after recovery",
                    ));
                }
                _ => {}
            }
        }
        if let Some(fallback) = &self.fallback {
            fallback.recheck()?;
        }
        let fallback = self.fallback.clone();
        self.cleanup()?;
        Ok(fallback)
    }

    pub fn commit(&mut self) -> io::Result<()> {
        self.cleanup()
    }
    pub fn recovery_path(&self) -> &Path {
        &self.stage
    }
    fn cleanup(&mut self) -> io::Result<()> {
        for stage in &self.stages {
            steam_depot::check_plain(stage, true)?;
            // inspect_tree refuses nested reparse points before recursive removal.
            crate::dst_workshop_cache_fs::inspect_tree(stage, 0).map_err(cache_fs_error)?;
        }
        for stage in &self.stages {
            clear_private_readonly_files(stage)?;
            fs::remove_dir_all(stage)?;
        }
        Ok(())
    }
}

#[cfg(windows)]
fn clear_private_readonly_files(path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_READONLY, SetFileAttributesW};

    // Only the checked, private recovery copies are modified. Original cache
    // permissions are retained when a backup is moved back to its live path.
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        steam_depot::check_plain(&path, metadata.is_dir())?;
        if metadata.is_dir() {
            clear_private_readonly_files(&path)?;
        } else if metadata.permissions().readonly() {
            let attributes = metadata.file_attributes() & !FILE_ATTRIBUTE_READONLY;
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            // Preserve ACLs and every other attribute. The owned path buffer
            // remains NUL-terminated and alive for the native call.
            if unsafe { SetFileAttributesW(wide.as_ptr(), attributes) } == 0 {
                return Err(io::Error::last_os_error());
            }
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn clear_private_readonly_files(_: &Path) -> io::Result<()> {
    Ok(())
}

fn cache_fs_error(error: crate::DstWorkshopCacheError) -> io::Error {
    match error {
        crate::DstWorkshopCacheError::Io { source, .. } => source,
        other => io::Error::other(other),
    }
}

fn scoped_context(
    settings: &app_core::AppSettings,
    scope: &WorkshopCacheScope<'_>,
    guard: &crate::GameInstallLifecycleGuard,
    steamcmd: &crate::SteamCmdLifecycleGuard,
) -> Result<(PathBuf, Vec<PathBuf>, Vec<String>), crate::SteamCmdError> {
    guard.0.ensure_root(scope.install_root)?;
    steamcmd.ensure_settings(settings)?;
    let ids = crate::normalize_steam_workshop_item_ids(scope.ids);
    if ids.is_empty()
        || ids.len() != scope.ids.len()
        || ids.len() > 8192
        || scope.consumer_app_id == 0
    {
        return Err(crate::SteamCmdError::MissingWorkshopItemIds);
    }
    let native = PathBuf::from(crate::steamcmd_status(settings).root);
    let roots = crate::workshop_paths::resolve_workshop_content_roots(
        scope.install_root,
        &native,
        scope.consumer_app_id,
    );
    let roots = vec![roots.0, roots.1];
    if let Some(sources) = scope.fallback_sources
        && (sources.len() > ids.len()
            || sources.iter().any(|(id, choices)| {
                !ids.contains(id)
                    || choices.len() > 2
                    || choices
                        .iter()
                        .any(|choice| !roots.iter().any(|root| *choice == root.join(id)))
            }))
    {
        return Err(crate::SteamCmdError::MissingWorkshopItemIds);
    }
    Ok((native, roots, ids))
}

fn require_no_unconfirmed_recovery(root: &Path) -> io::Result<()> {
    for (index, entry) in fs::read_dir(root)?.enumerate() {
        if index > 16_384 {
            return Err(steam_depot::invalid(
                "Workshop recovery inspection limit exceeded",
            ));
        }
        let entry = entry?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(".langame-workshop-recovery-"))
        {
            let path = entry.path();
            steam_depot::check_plain(&path, true)?;
            if path.join("recovery.json").try_exists()? {
                return Err(io::Error::other(format!(
                    "上次 Workshop 更新的退出或恢复未确认；请先检查保留的恢复数据 {}，再重试启动",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

// Dropping an unconfirmed operation deliberately preserves its private backup.
// It never mutates live paths or claims successful rollback from Drop.

fn verify_existing_cache(
    app: u32,
    install: &Path,
    steamcmd: &Path,
    roots: &[PathBuf],
    ids: &[String],
    cancellation: &InstallCancellation,
    sources: Option<&BTreeMap<String, Vec<PathBuf>>>,
) -> io::Result<Option<VerifiedWorkshopCache>> {
    let inventories = roots
        .iter()
        .map(|root| {
            crate::workshop_cache::read_installed_inventory(root, app, false)
                .map_err(io::Error::other)
        })
        .collect::<io::Result<Vec<_>>>()?;
    let mut verified = Vec::new();
    for id in ids {
        let mut selected = None;
        for (root, installed) in roots.iter().zip(&inventories) {
            if sources
                .and_then(|sources| sources.get(id))
                .is_some_and(|choices| !choices.contains(&root.join(id)))
            {
                continue;
            }
            if let Some(item) = installed.get(id) {
                match integrity::verify_old_item(
                    root,
                    steamcmd,
                    install,
                    app,
                    id,
                    *item,
                    cancellation,
                ) {
                    Ok(files) => {
                        selected = Some(VerifiedItem {
                            id: id.clone(),
                            path: root.join(id),
                            files,
                        });
                        break;
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => return Err(error),
                    // Missing/invalid old integrity proof prevents fallback but
                    // does not suppress a normal repairing update attempt.
                    Err(_) => {}
                }
            }
        }
        if let Some(item) = selected {
            verified.push(item);
        }
    }
    if verified.len() != ids.len() {
        return Ok(None);
    }
    Ok(Some(VerifiedWorkshopCache {
        result: SteamWorkshopDownloadResult {
            consumer_app_id: app,
            install_root: install.to_string_lossy().into_owned(),
            workshop_root: roots[0].to_string_lossy().into_owned(),
            items: verified
                .iter()
                .map(|item| SteamWorkshopDownloadItemResult {
                    item_id: item.id.clone(),
                    expected_path: item.path.to_string_lossy().into_owned(),
                    expected_path_exists: true,
                })
                .collect(),
            output_excerpt: String::new(),
        },
        items: verified,
    }))
}

fn plain_existing_ancestors(path: &Path) -> io::Result<()> {
    for ancestor in path.ancestors().skip(1) {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => return steam_depot::check_plain(ancestor, true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Err(steam_depot::invalid(
        "Workshop path has no existing plain ancestor",
    ))
}

#[cfg(test)]
#[path = "workshop_cache_transaction_tests.rs"]
mod tests;
