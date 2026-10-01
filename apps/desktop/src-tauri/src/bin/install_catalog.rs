//! Operator-invoked installation verification using the desktop's production crates.
//! `probe` is read-only; `install <module-id|all>` explicitly installs library files.
//! `inventory <module-id|all>` and `verify <module-id|all>` only inspect existing
//! Steam files; verification hashes payloads without installing or deleting them.
//! `certify <module-id|all>` officially validates an unused existing Steam tree
//! in place, then records a baseline only when its complete exact inventory matches.
//! Commands accept `--games-root <absolute-path>` and `--modules-root <absolute-path>`.
//! Redirect JSONL output to the operator's work directory, outside the repository.
//! The production installer enforces deadlines; this CLI has no graceful-cancel input.
use std::error::Error;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use app_core::{InstallState, ModuleDetails};
use app_modules::{ModuleDescriptor, discover_modules};
use app_steamcmd::{
    GameInstallLifecycleGuard, InstallCancellation, acquire_game_install_lifecycle,
    install_or_update_module_at_with_progress_and_cancellation, probe_module_install_state,
    steamcmd_status,
};
use app_storage::{
    GameInstallSyncRecord, ProgramInstallScope, StoragePaths,
    ensure_library_program_target_available, ensure_program_archive_dependencies,
    initialize_database, list_active_instance_runs, list_instances, prepare_clean_library_seed_at,
    read_instance_details, read_library_program_acquisition, read_module_instance_installs,
    read_program_install_owner, record_library_program_baseline, resolve_instance_runtime_root,
    restore_library_program_acquisition, sync_game_installs, sync_modules,
};
use serde_json::json;

#[path = "install_catalog/steam_seed.rs"]
mod steam_seed;

#[path = "install_catalog/steam_inspect.rs"]
mod steam_inspect;

#[path = "install_catalog/steam_certify.rs"]
mod steam_certify;

const USAGE: &str = "usage: install_catalog [--games-root <absolute-path>] [--modules-root <absolute-path>] probe | install <module-id|all> | inventory <module-id|all> | verify <module-id|all> | certify <module-id|all>";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Inspection {
    Inventory,
    Verify,
}

#[derive(Debug, PartialEq)]
struct Options {
    module: Option<String>,
    games_root: Option<PathBuf>,
    modules_root: Option<PathBuf>,
    inspection: Option<Inspection>,
    certify: bool,
}

fn parse_options(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut options = Options {
        module: None,
        games_root: None,
        modules_root: None,
        inspection: None,
        certify: false,
    };
    let mut positional = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let slot = match arg.as_str() {
            "--games-root" => &mut options.games_root,
            "--modules-root" => &mut options.modules_root,
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}; {USAGE}")),
            _ => {
                positional.push(arg);
                continue;
            }
        };
        if slot.is_some() {
            return Err(format!("duplicate option: {arg}"));
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {arg}"))?;
        let path = PathBuf::from(value);
        if !path.is_absolute()
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(format!(
                "{arg} requires an absolute path without parent traversal"
            ));
        }
        *slot = Some(path);
    }
    match positional.as_slice() {
        [mode] if mode == "probe" => {}
        [mode, module] if mode == "install" => options.module = Some(module.clone()),
        [mode, module] if mode == "certify" => {
            options.module = Some(module.clone());
            options.certify = true;
        }
        [mode, module] if mode == "inventory" || mode == "verify" => {
            options.module = Some(module.clone());
            options.inspection = Some(if mode == "verify" {
                Inspection::Verify
            } else {
                Inspection::Inventory
            });
        }
        _ => return Err(USAGE.into()),
    }
    Ok(options)
}

// Canonicalize existing ancestors without creating a probe's missing directory.
fn resolved_directory(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "absolute directory required",
        ));
    }
    let mut ancestor = path.to_owned();
    let mut tail = Vec::new();
    let mut resolved = loop {
        match std::fs::metadata(&ancestor) {
            Ok(metadata) if metadata.is_dir() => break dunce::canonicalize(&ancestor)?,
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "directory points to a file",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                tail.push(ancestor.file_name().ok_or(error)?.to_owned());
                ancestor.pop();
            }
            Err(error) => return Err(error),
        }
    };
    for name in tail.into_iter().rev() {
        resolved.push(name);
    }
    Ok(resolved)
}

fn paths_overlap(left: &Path, right: &Path) -> io::Result<bool> {
    let left = resolved_directory(left)?;
    let right = resolved_directory(right)?;
    #[cfg(windows)]
    let (left, right) = (
        PathBuf::from(left.to_string_lossy().to_lowercase()),
        PathBuf::from(right.to_string_lossy().to_lowercase()),
    );
    Ok(left.starts_with(&right) || right.starts_with(&left))
}

fn directory_is_empty(root: &Path) -> io::Result<bool> {
    match std::fs::read_dir(root) {
        Ok(mut entries) => Ok(entries.next().transpose()?.is_none()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
    }
}

/// The lifecycle lock excludes starts that use a library during this transition.
/// Unlike the desktop, this standalone operator has no in-process supervisor;
/// refuse every existing instance reference, including stopped reservations.
async fn ensure_target_unused(
    paths: &StoragePaths,
    id: &str,
    root: &Path,
) -> Result<(), Box<dyn Error>> {
    if !list_active_instance_runs(paths).await?.is_empty() {
        return Err(
            "managed game runs are active; stop them before installing library files".into(),
        );
    }
    ensure_library_program_target_available(paths, root).await?;
    let database_root = paths
        .database_path
        .parent()
        .ok_or("database has no parent directory")?;
    for protected in [
        database_root,
        &paths.logs_root,
        &paths.modules_root,
        &paths.steamcmd_root,
        &paths.archives_root,
    ] {
        if paths_overlap(root, protected)? {
            return Err(format!(
                "installation overlaps protected data: {}",
                protected.display()
            )
            .into());
        }
    }
    if let Some(owner) = read_program_install_owner(paths, root).await?
        && (owner.scope != ProgramInstallScope::Library || owner.module_id != id)
    {
        return Err("installation belongs to another module or an independent instance".into());
    }
    // The database also reserves bindings whose on-disk runtime is incomplete.
    for reference in read_module_instance_installs(paths, id).await? {
        if paths_overlap(root, &reference.install.install_root)? {
            return Err(format!(
                "installation is bound to instance {}",
                reference.instance_id
            )
            .into());
        }
    }
    // Resolve actual runtime bindings as well as database ownership. Never repair
    // or rebind an existing instance as a side effect of this operator command.
    for summary in list_instances(paths).await? {
        let instance = read_instance_details(paths, &summary.id).await?;
        let instance_root = Path::new(&instance.config_file_path)
            .parent()
            .and_then(Path::parent)
            .ok_or("instance configuration has no instance root")?;
        let runtime_root = resolve_instance_runtime_root(instance_root)?;
        if paths_overlap(root, &runtime_root)? {
            return Err(format!("installation is used by instance {}", summary.id).into());
        }
    }
    ensure_program_archive_dependencies(paths, root).await?;
    Ok(())
}

async fn persist_verified_install(
    guard: GameInstallLifecycleGuard,
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
    record: GameInstallSyncRecord,
    source_is_clean: bool,
    seed_receipt: Option<steam_seed::SeedReceipt>,
) -> Result<(), Box<dyn Error>> {
    // Move the lease into the hash worker: dropping an async waiter must not
    // expose files while a baseline is still being recorded. Return it to the
    // async caller so registration is part of the same protected transition.
    let baseline_descriptor = descriptor.clone();
    let root = PathBuf::from(&record.install_root);
    let (_guard, baseline_result) = tokio::task::spawn_blocking(move || {
        let result =
            record_library_program_baseline(&root, &baseline_descriptor, source_is_clean, None);
        (guard, result)
    })
    .await?;
    baseline_result?;
    sync_game_installs(paths, &[record]).await?;
    if let Some(receipt) = &seed_receipt {
        steam_seed::finish(receipt)?;
    }
    Ok(())
}

fn emit(mut value: serde_json::Value) {
    value["unix_ms"] = json!(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    println!("{value}");
    let _ = io::stdout().flush();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Keep the catalog state machine off the Windows entry thread's stack.
    let operation = Box::pin(run_catalog());
    emit(
        json!({"event":"future_layout", "catalog_future_bytes":std::mem::size_of_val(operation.as_ref().get_ref())}),
    );
    operation.await
}

async fn run_catalog() -> Result<(), Box<dyn Error>> {
    let options = parse_options(std::env::args().skip(1))?;
    let installing = options.module.is_some() && options.inspection.is_none();
    let defaults = StoragePaths::default();
    let mut settings = if defaults.settings_path.is_file() {
        serde_json::from_slice(&std::fs::read(&defaults.settings_path)?)?
    } else {
        defaults.settings()
    };
    let modules_root = resolved_directory(
        options
            .modules_root
            .as_deref()
            .unwrap_or(&defaults.modules_root),
    )?;
    let requested_games_root = options
        .games_root
        .as_deref()
        .unwrap_or(Path::new(&settings.games_root));
    // Preserve the caller's spelling until reparse checks have examined it.
    if options.inspection.is_some() || options.certify {
        steam_seed::checked_root(requested_games_root)?;
    }
    let games_root = resolved_directory(requested_games_root)?;
    // Overrides are operation-local. Persist neither settings nor instance roots.
    settings.games_root = games_root.to_string_lossy().into_owned();
    settings.modules_root = modules_root.to_string_lossy().into_owned();
    let mut paths = defaults.with_app_settings(&settings);
    paths.modules_root = modules_root;
    let modules = discover_modules(&paths.modules_root)?;
    if let Some(module) = &options.module
        && module != "all"
        && !modules.iter().any(|m| m.summary.id == *module)
    {
        return Err(format!("unknown module: {module}").into());
    }
    emit(
        json!({"event":"environment", "games_root":settings.games_root,
        "modules_root":paths.modules_root, "database_path":paths.database_path,
        "steamcmd":steamcmd_status(&settings), "module_count":modules.len()}),
    );
    if installing {
        // This command operates on the current desktop user's database, never
        // silently creates a second installation catalog under the new root.
        if !paths.database_path.is_file() {
            return Err("desktop user database does not exist".into());
        }
        initialize_database(&paths).await?;
        sync_modules(&paths, &modules).await?;
    }
    let mut failures = Vec::new();
    for descriptor in modules {
        let id = &descriptor.summary.id;
        if options
            .module
            .as_ref()
            .is_some_and(|target| target != "all" && target != id)
        {
            continue;
        }
        let probe = || {
            probe_module_install_state(
                &settings,
                id,
                descriptor.summary.steam_app_id,
                descriptor.install.as_ref(),
                descriptor.process.as_ref(),
            )
        };
        let before = probe();
        emit(json!({"event":"before", "probe":before}));
        if let Some(mode) = options.inspection {
            let worker_settings = settings.clone();
            let worker_descriptor = descriptor.clone();
            let result = tokio::task::spawn_blocking(move || {
                steam_inspect::run(&worker_settings, &worker_descriptor, mode, emit)
            })
            .await?;
            if let Err(error) = result {
                emit(
                    json!({"event":"inspection_failed", "module_id":id, "reason":error.to_string()}),
                );
                failures.push(id.clone());
            }
            continue;
        }
        if !installing {
            continue;
        }
        if options.certify {
            if let Err(reason) = steam_certify::run(&paths, &settings, &descriptor).await {
                emit(json!({"event":"certification_failed", "module_id":id, "reason":reason}));
                failures.push(id.clone());
            }
            continue;
        }
        let root = PathBuf::from(&before.install_root);
        let guard = acquire_game_install_lifecycle(id, std::slice::from_ref(&root)).await?;
        if let Err(error) = ensure_target_unused(&paths, id, &root).await {
            emit(json!({"event":"blocked", "module_id":id, "reason":error.to_string()}));
            failures.push(id.clone());
            continue;
        }
        let was_empty = directory_is_empty(&root)?;
        let pending = read_library_program_acquisition(&root, &descriptor)?;
        // Direct ZIP publication replaces a directory. Apply the desktop's
        // nonempty-directory boundary so this operator tool cannot erase saves.
        if descriptor
            .install
            .as_ref()
            .is_some_and(|spec| spec.download_url_windows.is_some())
            && !was_empty
            && pending.is_none()
        {
            emit(json!({"event":"blocked", "module_id":id,
                "reason":"direct-download target is nonempty; preserve existing files"}));
            failures.push(id.clone());
            continue;
        }
        // Allocate through the production acquisition API before downloading.
        // An incomplete managed acquisition can be resumed; arbitrary retained
        // files never become a clean source merely because validation succeeds.
        let worker_paths = paths.clone();
        let worker_descriptor = descriptor.clone();
        let worker_root = root.clone();
        let (guard, acquisition_result) = tokio::spawn(async move {
            let prepared = async {
                if was_empty {
                    if worker_root.exists() {
                        // remove_dir only succeeds for an empty directory.
                        std::fs::remove_dir(&worker_root).map_err(|error| error.to_string())?;
                    }
                    prepare_clean_library_seed_at(
                        &worker_paths,
                        &worker_descriptor,
                        &worker_root,
                        None,
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                }
                read_library_program_acquisition(&worker_root, &worker_descriptor)
                    .map_err(|error| error.to_string())
            }
            .await;
            // A dropped waiter cannot release the lease while a seed copier
            // still owns archive or instance source files.
            (guard, prepared)
        })
        .await?;
        let acquisition = match acquisition_result {
            Ok(acquisition) => acquisition,
            Err(reason) => {
                emit(json!({"event":"blocked", "module_id":id, "reason":reason}));
                failures.push(id.clone());
                continue;
            }
        };
        let mut source_is_clean = was_empty || acquisition.is_some();
        // A persisted cache receipt always forces final inventory verification,
        // when resuming an interrupted acquisition. Never infer cleanliness from
        // the copied ACF's Installed flag.
        let worker_root = root.clone();
        let worker_id = id.clone();
        let app_id = descriptor.summary.steam_app_id;
        let (guard, seed_result) = tokio::task::spawn_blocking(move || {
            let prepared = (|| -> io::Result<Option<steam_seed::SeedReceipt>> {
                let Some(app_id) = app_id else {
                    return Ok(None);
                };
                steam_seed::load(&worker_root, &worker_id, app_id)
            })();
            (guard, prepared)
        })
        .await?;
        let seed_receipt = match seed_result {
            Ok(receipt) => receipt,
            Err(error) => {
                emit(json!({"event":"blocked", "module_id":id, "reason":error.to_string()}));
                failures.push(id.clone());
                continue;
            }
        };
        if let Some(receipt) = &seed_receipt {
            source_is_clean = true;
            emit(json!({"event":"cache_prepared", "module_id":id, "summary":receipt.summary}));
        }
        // Receipt recovery can restore an acquisition removed by an interrupted
        // baseline publication. Capture that exact identity before installation.
        let acquisition = if seed_receipt.is_some() {
            Some(
                read_library_program_acquisition(&root, &descriptor)?
                    .ok_or("cache receipt has no managed acquisition")?,
            )
        } else {
            acquisition
        };
        let module = ModuleDetails {
            summary: descriptor.summary.clone(),
            schema_json: descriptor.schema_json.clone(),
            default_ports: descriptor.default_ports.clone(),
            install: descriptor.install.clone(),
            process: descriptor.process.clone(),
            workshop: descriptor.workshop.clone(),
            mods: None,
            runtime: descriptor.runtime.clone(),
        };
        // The production installer owns its six-hour per-game deadline and
        // subprocess cleanup; do not drop its future via an outer timeout.
        let cancellation = InstallCancellation::new();
        // The installer composes SteamCMD preparation, HTTP and game providers.
        // Store that large nested future separately instead of embedding it in
        // the catalog future and repeatedly moving it through runtime entry.
        let install = Box::pin(install_or_update_module_at_with_progress_and_cancellation(
            &settings,
            &module,
            &root,
            &guard,
            true,
            &cancellation,
            |update| {
                emit(json!({"event":"progress", "module_id":id,
                "percent":update.progress_percent, "detail":update.detail}));
            },
        ));
        emit(json!({"event":"future_layout", "module_id":id,
            "installer_future_bytes":std::mem::size_of_val(install.as_ref().get_ref())}));
        let result = install.await;
        // Whole-directory publication can discard the marker. Restore only the
        // exact acquisition captured under this lease, even after a failed run.
        let acquisition_error = acquisition.as_ref().and_then(|pending| {
            if root.exists() {
                restore_library_program_acquisition(pending)
                    .err()
                    .map(|error| error.to_string())
            } else {
                None
            }
        });
        let after = probe();
        // A module may materialize its launch wrapper only when an instance
        // starts. Its explicit verification path identifies the shipped payload.
        let payload_exists = descriptor
            .install
            .as_ref()
            .and_then(|spec| spec.verification_path.as_ref())
            .map_or(after.executable_exists, |relative| {
                Path::new(&after.install_root).join(relative).is_file()
            });
        let mut verified = result.is_ok()
            && acquisition_error.is_none()
            && matches!(after.install_state, InstallState::Installed)
            && payload_exists
            && (descriptor.summary.steam_app_id.is_none()
                || after
                    .steam_manifest
                    .as_ref()
                    .is_some_and(|manifest| manifest.complete));
        let mut persistence_error = None;
        let worker_steamcmd = paths.steamcmd_root.clone();
        let (guard, seed_receipt, cache_verification) = tokio::task::spawn_blocking(move || {
            let checked = if verified {
                seed_receipt
                    .as_ref()
                    .map(|receipt| steam_seed::finalize(receipt, &worker_steamcmd))
                    .transpose()
            } else {
                Ok(None)
            };
            (guard, seed_receipt, checked)
        })
        .await?;
        match cache_verification {
            Ok(Some(summary)) => {
                emit(json!({"event":"cache_verified", "module_id":id, "summary":summary}))
            }
            Ok(None) => {}
            Err(error) => {
                verified = false;
                persistence_error = Some(error.to_string());
            }
        }
        if verified {
            emit(
                json!({"event":"recording_baseline", "module_id":id, "source_is_clean":source_is_clean}),
            );
            let persisted = persist_verified_install(
                guard,
                &paths,
                &descriptor,
                GameInstallSyncRecord {
                    module_id: id.clone(),
                    install_root: after.install_root.clone(),
                    install_state: after.install_state.clone(),
                    current_version: after.current_version.clone(),
                    mark_verified: true,
                },
                source_is_clean,
                seed_receipt,
            )
            .await;
            if let Err(error) = persisted {
                verified = false;
                persistence_error = Some(error.to_string());
            }
        }
        emit(
            json!({"event":"result", "module_id":id, "verified":verified,
            "result":result.as_ref().ok(), "error":result.as_ref().err().map(|e| format!("{e:?}")),
            "acquisition_error":acquisition_error, "persistence_error":persistence_error, "probe":after}),
        );
        if !verified {
            failures.push(id.clone());
        }
    }
    emit(json!({"event":"complete", "mode":match options.inspection {
            Some(Inspection::Inventory) => "inventory",
            Some(Inspection::Verify) => "verify",
            None if options.certify => "certify",
            None if installing => "install",
            None => "probe",
        },
        "failures":failures, "instances_started":false}));
    if !failures.is_empty() {
        return Err("one or more installations failed; see result events".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Options, String> {
        parse_options(args.iter().map(|value| value.to_string()))
    }

    #[test]
    fn accepts_explicit_roots_on_either_side_of_the_command() {
        let root = std::env::temp_dir();
        let path = root.to_str().unwrap();
        assert_eq!(
            parse(&[
                "--games-root",
                path,
                "install",
                "all",
                "--modules-root",
                path
            ])
            .unwrap(),
            Options {
                module: Some("all".into()),
                games_root: Some(root.clone()),
                modules_root: Some(root.clone()),
                inspection: None,
                certify: false,
            }
        );
        assert_eq!(parse(&["probe"]).unwrap().module, None);
        assert_eq!(
            parse(&["install", "minecraft"]).unwrap().module.as_deref(),
            Some("minecraft")
        );
    }

    #[test]
    fn rejects_ambiguous_or_unsafe_arguments() {
        let root = std::env::temp_dir();
        let path = root.to_str().unwrap();
        for args in [
            vec!["probe", "all"],
            vec!["install"],
            vec!["install", "all", "extra"],
            vec!["probe", "--games-root"],
            vec!["probe", "--games-root", "relative"],
            vec!["probe", "--games-root", path, "--games-root", path],
            vec!["probe", "--unknown"],
            vec!["inventory"],
            vec!["inventory", "all", "--discard-development-data"],
            vec!["certify"],
            vec!["certify", "all", "--discard-development-data"],
        ] {
            assert!(parse(&args).is_err(), "accepted {args:?}");
        }
        let traversal = root.join("..").join("unsafe");
        assert!(parse(&["probe", "--games-root", traversal.to_str().unwrap()]).is_err());
    }

    #[test]
    fn removed_transfer_option_is_rejected_for_every_command() {
        let root = std::env::temp_dir();
        let path = root.to_str().unwrap();
        for mut args in [
            vec!["probe"],
            vec!["install", "all"],
            vec!["inventory", "all"],
            vec!["verify", "all"],
            vec!["certify", "all"],
        ] {
            args.extend(["--transfer-games-root", path]);
            let error = parse(&args).unwrap_err();
            assert!(error.starts_with("unknown option: --transfer-games-root;"));
        }
    }

    #[test]
    fn inspection_commands_are_explicit_and_never_install() {
        for (command, mode) in [
            ("inventory", Inspection::Inventory),
            ("verify", Inspection::Verify),
        ] {
            let options = parse(&[command, "valheim"]).unwrap();
            assert_eq!(options.module.as_deref(), Some("valheim"));
            assert_eq!(options.inspection, Some(mode));
            assert!(!options.certify);
        }
    }

    #[test]
    fn certification_is_explicit_and_cannot_enable_transfer_or_discard() {
        let options = parse(&["certify", "valheim"]).unwrap();
        assert_eq!(options.module.as_deref(), Some("valheim"));
        assert!(options.certify);
        assert!(options.inspection.is_none());
    }

    #[test]
    fn resolves_missing_roots_read_only_and_compares_path_components() {
        let root =
            std::env::temp_dir().join(format!("lgsm-install-catalog-{}", uuid::Uuid::new_v4()));
        assert!(directory_is_empty(&root).unwrap());
        assert!(paths_overlap(&root, &root.join("game")).unwrap());
        assert!(!paths_overlap(&root.join("game"), &root.join("game-other")).unwrap());
        assert!(!root.exists());
    }
}
