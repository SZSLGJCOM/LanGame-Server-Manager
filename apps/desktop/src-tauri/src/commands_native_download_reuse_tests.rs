use super::*;
use crate::commands::tests::creation_lifecycle_tests::CREATION_INSTALL_FORBIDDEN;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const MODULE: &str = "valheim";
const CLEAN_PACKAGE: &str = ".langame-clean-package.json";
const APP_MANIFEST: &str = "steamapps/appmanifest_896660.acf";

#[path = "commands_native_download_retry_tests.rs"]
mod retry;

/// Failed probes preserve their acquired package so investigation needs no
/// additional download. No game process is started by either test.
struct ProbeRoot {
    path: PathBuf,
    parent: PathBuf,
    cleaned: bool,
}

impl ProbeRoot {
    fn new() -> TestResult<Self> {
        let path =
            crate::commands::tests::real_smoke_support::allocate_smoke_run_root("vdownload")?;
        let path = fs::canonicalize(path)?;
        let parent = path
            .parent()
            .ok_or("probe directory has no parent")?
            .to_owned();
        Ok(Self {
            path,
            parent,
            cleaned: false,
        })
    }

    fn cleanup(&mut self) -> TestResult {
        let path = fs::canonicalize(&self.path)?;
        check(
            path == self.path && path.parent() == Some(self.parent.as_path()),
            "refusing cleanup outside the owned temporary directory",
        )?;
        fs::remove_dir_all(path)?;
        self.cleaned = true;
        Ok(())
    }
}

impl Drop for ProbeRoot {
    fn drop(&mut self) {
        if !self.cleaned {
            eprintln!(
                "VALHEIM_DOWNLOAD_REUSE retained_root={} cleanup=preserved_for_investigation",
                self.path.display()
            );
        }
    }
}

#[derive(Debug)]
struct PackageEvidence {
    files: Option<BTreeMap<String, String>>,
    executable_sha256: String,
    app_manifest_sha256: Option<String>,
    acquisition_present: bool,
}

fn check(condition: bool, message: &str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn digest(path: &Path) -> TestResult<String> {
    Ok(Sha256::digest(fs::read(path)?)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn output_tail(output: &str) -> String {
    output
        .chars()
        .rev()
        .take(4096)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn report_install(result: &ModuleInstallResult, phase: &str) {
    println!(
        "VALHEIM_DOWNLOAD_REUSE {}",
        json!({
            "phase": phase,
            "operation": result.operation,
            "current_version": result.current_version,
            "output_tail": output_tail(&result.output_excerpt),
        })
    );
}

fn report_creation_failure(
    app: &tauri::App<tauri::test::MockRuntime>,
    storage: &StorageBootstrap,
    phase: &str,
    error: &str,
) -> TestResult {
    let state = app.state::<DesktopState>();
    let app_state = state
        .app_state
        .read()
        .map_err(|_| "app state lock poisoned")?;
    if let Some(job) = app_state.jobs.iter().find(|job| {
        job.id.starts_with("instance-create") && job.target_id.as_deref() == Some(MODULE)
    }) {
        println!(
            "VALHEIM_DOWNLOAD_REUSE {}",
            json!({
                "phase": phase,
                "creation_error": error,
                "job_status": job.status,
                "job_detail": job.detail,
                "job_output_tail": job.output_excerpt.as_deref().map(output_tail),
            })
        );
    }
    drop(app_state);
    let lines =
        crate::desktop_app_log::recent_lines(&storage.paths.app_log_path(), None, 64, 128 * 1024)?;
    for line in lines.into_iter().rev() {
        let entry: serde_json::Value = serde_json::from_str(&line)?;
        if entry["action"] != "instance.create.program_repair" {
            continue;
        }
        let context = &entry["context"];
        let target = PathBuf::from(
            context["program_root"]
                .as_str()
                .ok_or("repair target missing")?,
        );
        check(
            target.starts_with(&storage.paths.games_root),
            "repair target escaped the isolated games root",
        )?;
        let payload_entries = fs::read_dir(&target)?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with(".langame-"))
            .count();
        println!(
            "VALHEIM_DOWNLOAD_REUSE {}",
            json!({
                "phase": phase,
                "repair_context": context,
                "repair_target_payload_entries": payload_entries,
                "repair_target_executable_present": target.join("valheim_server.exe").try_exists()?,
                "installer_was_forbidden": true,
            })
        );
        break;
    }
    Ok(())
}

fn evidence(root: &Path, phase: &str) -> TestResult<PackageEvidence> {
    let manifest = root.join(CLEAN_PACKAGE);
    let files = if manifest.try_exists()? {
        let document: serde_json::Value = serde_json::from_slice(&fs::read(manifest)?)?;
        Some(serde_json::from_value(document["files"].clone())?)
    } else {
        None
    };
    let app_manifest = root.join(APP_MANIFEST);
    let evidence = PackageEvidence {
        files,
        executable_sha256: digest(&root.join("valheim_server.exe"))?,
        app_manifest_sha256: app_manifest
            .try_exists()?
            .then(|| digest(&app_manifest))
            .transpose()?,
        acquisition_present: root
            .join(".langame-program-acquisition.json")
            .try_exists()?,
    };
    println!(
        "VALHEIM_DOWNLOAD_REUSE {}",
        json!({
            "phase": phase,
            "program_root": root,
            "clean_package_present": evidence.files.is_some(),
            "clean_package_files": evidence.files.as_ref().map(BTreeMap::len),
            "initial_package_present": root.join(".langame-initial-package.json").try_exists()?,
            "acquisition_present": evidence.acquisition_present,
            "app_manifest_sha256": evidence.app_manifest_sha256,
            "executable_sha256": evidence.executable_sha256,
        })
    );
    Ok(evidence)
}

fn fixture_settings(root: &Path) -> AppSettings {
    AppSettings {
        archives_root: root.join("instances/.trash").to_string_lossy().into_owned(),
        servers_root: root.join("instances").to_string_lossy().into_owned(),
        games_root: root.join("games").to_string_lossy().into_owned(),
        modules_root: workspace_root()
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
    }
}

fn mock_app(storage: &StorageBootstrap) -> TestResult<tauri::App<tauri::test::MockRuntime>> {
    Ok(tauri::test::mock_builder()
        .manage(DesktopState::from_storage(storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?)
}

async fn download(
    app: &tauri::App<tauri::test::MockRuntime>,
    storage: &StorageBootstrap,
) -> TestResult<PathBuf> {
    println!("VALHEIM_DOWNLOAD_REUSE phase=prepare_steamcmd");
    Box::pin(ensure_steamcmd_ready(
        app.state::<DesktopState>(),
        "valheim-download-reuse".into(),
    ))
    .await?;
    println!("VALHEIM_DOWNLOAD_REUSE phase=download");
    let result = Box::pin(install_module_game(
        app.state::<DesktopState>(),
        MODULE.into(),
    ))
    .await?;
    check(
        result.install_state == InstallState::Installed && result.executable_exists,
        "Valheim download did not finish with an installed executable",
    )?;
    let root = PathBuf::from(&result.install_root);
    check(
        root == storage.paths.games_root.join(MODULE),
        "download escaped the isolated Valheim program root",
    )?;
    report_install(&result, "downloaded");
    Ok(root)
}

async fn create_without_download(
    app: &tauri::App<tauri::test::MockRuntime>,
    storage: &StorageBootstrap,
    source: &Path,
    expected: &PackageEvidence,
    source_expected: Option<&PackageEvidence>,
    phase: &str,
) -> TestResult {
    let source_expected = source_expected.unwrap_or(expected);
    println!("VALHEIM_DOWNLOAD_REUSE phase={phase} installer=forbidden");
    let creation = CREATION_INSTALL_FORBIDDEN
        .scope(
            true,
            create_instance_record(
                app.state::<DesktopState>(),
                CreateInstanceInput {
                    name: format!("Valheim download reuse {phase}"),
                    module_id: MODULE.into(),
                },
                Some(app_core::InstanceProgramMode::Independent),
            ),
        )
        .await;
    let created = match creation {
        Ok(created) => created,
        Err(error) => {
            if let Err(diagnostic_error) = report_creation_failure(app, storage, phase, &error) {
                eprintln!(
                    "VALHEIM_DOWNLOAD_REUSE phase={phase} diagnostic_error={diagnostic_error}"
                );
            }
            return Err(format!(
                "{phase}: creation could not reuse downloaded Valheim files: {error}"
            )
            .into());
        }
    };
    let program = app_storage::read_instance_program_install(&storage.paths, &created.summary.id)
        .await?
        .ok_or("created Valheim instance has no persisted program binding")?;
    check(
        program.install.install_state == InstallState::Installed,
        "created instance program is not installed",
    )?;
    let copied = evidence(&program.install.install_root, phase)?;
    check(
        copied.executable_sha256 == expected.executable_sha256,
        "created instance executable differs from the downloaded package",
    )?;
    check(
        copied.files.is_some() && copied.files == expected.files,
        "created instance did not retain the downloaded clean package inventory",
    )?;
    let library = app_storage::read_library_program_install(&storage.paths, MODULE)
        .await?
        .ok_or("creation discarded the downloaded Valheim library registration")?;
    // Validation may invalidate the library's inventory. A healthy instance can
    // then seed a replacement library without downloading; the rejected source
    // must remain untouched and copies must match the pre-validation package.
    check(
        source_expected.files.is_none()
            || fs::canonicalize(library.install_root)? == fs::canonicalize(source)?,
        "creation replaced the downloaded Valheim program source",
    )?;
    let preserved = evidence(source, &format!("{phase}_source"))?;
    check(
        preserved.files == source_expected.files
            && preserved.executable_sha256 == source_expected.executable_sha256
            && preserved.app_manifest_sha256 == source_expected.app_manifest_sha256,
        "creation modified the downloaded package or its Steam manifest",
    )?;
    println!("VALHEIM_DOWNLOAD_REUSE phase={phase}_passed downloads=0 real_game_processes=0");
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in: downloads real Valheim once into isolated storage, creates two instances with downloads forbidden, validates the library, then creates a third; no server is started"]
async fn native_valheim_download_create_validate_reuse() -> TestResult {
    let _serial = command_smoke_lock().lock().await;
    let mut root = ProbeRoot::new()?;
    let _environment = ProgramDataEnvGuard::set(&root.path.join("programdata"));
    save_app_settings(fixture_settings(&root.path))?;
    let storage = bootstrap_storage()?;
    let app = mock_app(&storage)?;
    let source = download(&app, &storage).await?;
    let downloaded = evidence(&source, "after_download")?;
    check(
        downloaded
            .files
            .as_ref()
            .is_some_and(|files| !files.is_empty())
            && !downloaded.acquisition_present,
        "completed download did not publish a clean package inventory",
    )?;
    create_without_download(&app, &storage, &source, &downloaded, None, "first").await?;
    create_without_download(&app, &storage, &source, &downloaded, None, "second").await?;
    println!("VALHEIM_DOWNLOAD_REUSE phase=validate");
    let validated = Box::pin(validate_module_game(
        app.state::<DesktopState>(),
        MODULE.into(),
    ))
    .await?;
    report_install(&validated, "validated");
    check(
        validated.install_state == InstallState::Installed && validated.executable_exists,
        "Valheim validation did not finish",
    )?;
    let after_validation = evidence(&source, "after_validation")?;
    check(
        after_validation.files.is_some() && !after_validation.acquisition_present,
        "same-version validation discarded the reusable package inventory",
    )?;
    println!(
        "VALHEIM_DOWNLOAD_REUSE phase=validation_manifest_comparison changed={}",
        downloaded.app_manifest_sha256 != after_validation.app_manifest_sha256
    );
    create_without_download(
        &app,
        &storage,
        &source,
        &after_validation,
        None,
        "third_after_validate",
    )
    .await?;
    drop(app);
    root.cleanup()?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in: allocates an empty managed acquisition, downloads real Valheim once, then creates an instance with downloads forbidden; preserves evidence on failure and never starts a server"]
async fn native_valheim_empty_acquisition_download_reuse() -> TestResult {
    let _serial = command_smoke_lock().lock().await;
    let mut root = ProbeRoot::new()?;
    let _environment = ProgramDataEnvGuard::set(&root.path.join("programdata"));
    save_app_settings(fixture_settings(&root.path))?;
    let storage = bootstrap_storage()?;
    initialize_database(&storage.paths).await?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    sync_modules(&storage.paths, &descriptors).await?;
    let descriptor = find_descriptor(&descriptors, MODULE)?;
    let source = storage.paths.games_root.join(MODULE);
    let guard =
        app_steamcmd::acquire_game_install_lifecycle(MODULE, std::slice::from_ref(&source)).await?;
    let seed =
        app_storage::prepare_clean_library_seed_at(&storage.paths, descriptor, &source, None)
            .await?;
    check(
        seed.requires_validation,
        "empty acquisition unexpectedly contains a verified package",
    )?;
    check(
        fs::read_dir(&source)?.all(|entry| {
            entry.is_ok_and(|entry| entry.file_name().to_string_lossy().starts_with(".langame-"))
        }),
        "empty acquisition contains payload or operator files",
    )?;
    drop(guard);
    let app = mock_app(&storage)?;
    let downloaded_source = download(&app, &storage).await?;
    check(
        downloaded_source == source,
        "download selected another acquisition root",
    )?;
    let downloaded = evidence(&source, "after_acquisition_download")?;
    create_without_download(
        &app,
        &storage,
        &source,
        &downloaded,
        None,
        "first_after_acquisition",
    )
    .await?;
    check(
        !downloaded.acquisition_present,
        "completed download left its pending acquisition marker",
    )?;
    drop(app);
    root.cleanup()?;
    Ok(())
}
