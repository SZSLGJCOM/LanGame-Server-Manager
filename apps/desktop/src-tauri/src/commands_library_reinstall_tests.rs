use super::*;
use crate::commands::commands_program_storage::{
    LibraryBaselineRecorder, ProgramUpdateRequest, acquire_library_program_mutation,
    install_program_with_baseline,
};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Write};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct PackageServer {
    url: String,
    integrity: app_core::DownloadIntegritySpec,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl PackageServer {
    async fn start() -> TestResult<Self> {
        Self::with_files(&[
            ("CoreKeeperServer.exe", "official fixture program"),
            (
                "CoreKeeperServer_Data/sharedassets0.assets",
                "official assets",
            ),
            (
                "CoreKeeperServer_Data/StreamingAssets/Mods/default.txt",
                "shipped default",
            ),
        ])
        .await
    }

    async fn with_files(files: &[(&str, &str)]) -> TestResult<Self> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())?;
            zip.write_all(bytes.as_bytes())?;
        }
        let bytes = zip.finish()?.into_inner();
        let integrity = app_core::DownloadIntegritySpec {
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            size: bytes.len() as u64,
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/server.zip", listener.local_addr()?);
        let task = tokio::spawn(async move {
            tokio::time::timeout(std::time::Duration::from_secs(30), async move {
                let (mut socket, _) = listener.accept().await?;
                let mut request = Vec::new();
                while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                    let mut buffer = [0_u8; 1024];
                    let count = socket.read(&mut buffer).await?;
                    if count == 0 || request.len() + count > 16 * 1024 {
                        return Err(std::io::Error::other("invalid fixture HTTP request"));
                    }
                    request.extend_from_slice(&buffer[..count]);
                }
                socket.write_all(format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    bytes.len()
                ).as_bytes()).await?;
                socket.write_all(&bytes).await?;
                socket.shutdown().await
            }).await.map_err(std::io::Error::other)?
        });
        Ok(Self {
            url,
            integrity,
            task,
        })
    }
}

async fn archive_update_fixture() -> TestResult<(Fixture, PackageServer)> {
    let mut fixture = Fixture::for_module("rimworld").await?;
    for (path, content) in [
        ("Assets/world.save", "existing world"),
        ("Configs/ServerConfig.json", "operator native configuration"),
        ("unknown-mods/operator.dll", "operator extension"),
        (
            ".langame-program-identity.json",
            r#"{"version":1,"module_id":"rimworld","id":"existing-program"}"#,
        ),
        (
            ".langame-program-usage.json",
            r#"{"version":1,"module_id":"rimworld","program_id":"existing-program","instance_id":"existing-instance"}"#,
        ),
    ] {
        let file = fixture.original.join(path);
        fs::create_dir_all(file.parent().unwrap())?;
        fs::write(file, content)?;
    }
    let server = PackageServer::with_files(&[
        ("RTServer.exe", "updated official executable"),
        ("Configs/ServerConfig.json", "new package default"),
        ("assets.bin", "new required payload"),
    ])
    .await?;
    let install = fixture.descriptor.install.as_mut().unwrap();
    install.download_url_windows = Some(server.url.clone());
    install.download_integrity_windows = Some(server.integrity.clone());
    Ok((fixture, server))
}

#[tokio::test]
async fn existing_archive_update_preserves_native_data_and_ownership() -> TestResult {
    let (fixture, mut server) = archive_update_fixture().await?;
    let before: serde_json::Value = serde_json::from_slice(&fs::read(
        fixture.original.join(".langame-clean-package.json"),
    )?)?;
    assert!(
        before.get("archive_sha256").is_none(),
        "upgrade must accept the old inventory schema"
    );
    let owner = fs::read(fixture.original.join(".langame-program-identity.json"))?;
    let usage = fs::read(fixture.original.join(".langame-program-usage.json"))?;
    let module = map_module_details_with_install_state(
        &fixture.storage.settings,
        &fixture.descriptor,
        Some(&fixture.original.to_string_lossy()),
    );
    let (operation, job) = fixture.operation()?;
    let guard = acquire_library_program_mutation(
        &fixture.state,
        &fixture.storage,
        "rimworld",
        &fixture.original,
    )
    .await?;
    let (result, _guard) = install_program_with_baseline(
        ProgramUpdateRequest {
            storage: &fixture.storage,
            descriptor: &fixture.descriptor,
            module: &module,
            root: &fixture.original,
            operation: &operation,
            guard,
            validate: false,
            cancellation: job.cancellation(),
            account_name: None,
        },
        |_| {},
    )
    .await?;
    (&mut server.task).await??;
    assert_eq!(result.install_state, InstallState::Installed);
    assert_eq!(
        fs::read(fixture.original.join("RTServer.exe"))?,
        b"updated official executable"
    );
    assert_eq!(
        fs::read(fixture.original.join("Assets/world.save"))?,
        b"existing world"
    );
    assert_eq!(
        fs::read(fixture.original.join("Configs/ServerConfig.json"))?,
        b"operator native configuration"
    );
    assert_eq!(
        fs::read(fixture.original.join("unknown-mods/operator.dll"))?,
        b"operator extension"
    );
    assert_eq!(
        fs::read(fixture.original.join(".langame-program-identity.json"))?,
        owner
    );
    assert_eq!(
        fs::read(fixture.original.join(".langame-program-usage.json"))?,
        usage
    );
    assert!(app_storage::archive_program_is_current(
        &fixture.original,
        &fixture.descriptor,
        None
    )?);
    Ok(())
}

#[cfg(windows)]
#[tokio::test]
async fn archive_update_publication_failure_restores_the_original_directory() -> TestResult {
    use std::os::windows::fs::OpenOptionsExt;
    let (fixture, mut server) = archive_update_fixture().await?;
    let old_executable = fs::read(fixture.original.join("RTServer.exe"))?;
    let old_inventory = fs::read(fixture.original.join(".langame-clean-package.json"))?;
    let operation = fixture
        .state
        .begin_storage_context_operation("archive publication rollback")?;
    let cancellation = app_steamcmd::InstallCancellation::new();
    let baseline = LibraryBaselineRecorder::new(&operation, &fixture.descriptor, &cancellation);
    let module = map_module_details_with_install_state(
        &fixture.storage.settings,
        &fixture.descriptor,
        Some(&fixture.original.to_string_lossy()),
    );
    let guard = fixture.guard().await?;
    let held = std::sync::Mutex::new(None);
    let result = app_steamcmd::install_or_update_module_at_with_callbacks(
        &fixture.storage.settings,
        &module,
        &fixture.original,
        &guard,
        false,
        &cancellation,
        app_steamcmd::ModuleInstallCallbacks {
            on_progress: |_| {},
            prepare_fresh_payload: |stage: PathBuf| {
                let baseline = &baseline;
                let held = &held;
                let source = fixture.original.clone();
                async move {
                    baseline
                        .prepare_archive_replacement(stage.clone(), source, Vec::new(), |_| {})
                        .await?;
                    // Deny DELETE sharing on the prepared executable. The old
                    // root can move to rollback, but publishing this stage fails.
                    let file = fs::OpenOptions::new()
                        .read(true)
                        .share_mode(1)
                        .open(stage.join("RTServer.exe"))
                        .map_err(|error| app_steamcmd::SteamCmdError::DirectDownloadFailed {
                            output_excerpt: error.to_string(),
                        })?;
                    *held.lock().unwrap() = Some(file);
                    Ok(())
                }
            },
        },
    )
    .await;
    drop(held.lock().unwrap().take());
    (&mut server.task).await??;
    assert!(result.is_err(), "the held stage must prevent publication");
    assert_eq!(
        fs::read(fixture.original.join("RTServer.exe"))?,
        old_executable
    );
    assert_eq!(
        fs::read(fixture.original.join(".langame-clean-package.json"))?,
        old_inventory
    );
    assert_eq!(
        fs::read(fixture.original.join("Assets/world.save"))?,
        b"existing world"
    );
    assert_eq!(
        fs::read(fixture.original.join("Configs/ServerConfig.json"))?,
        b"operator native configuration"
    );
    assert_eq!(
        fs::read(fixture.original.join("unknown-mods/operator.dll"))?,
        b"operator extension"
    );
    Ok(())
}

impl Drop for PackageServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn retained_reinstall_creates_first_and_second_instances_without_another_download()
-> TestResult {
    let mut fixture = Fixture::for_module("corekeeper").await?;
    fs::remove_dir_all(&fixture.original)?;
    let mods = fixture
        .original
        .join("CoreKeeperServer_Data/StreamingAssets/Mods");
    fs::create_dir_all(&mods)?;
    fs::write(mods.join("default.txt"), b"operator changed default")?;
    fs::write(mods.join("local.dll"), b"operator private mod")?;
    app_steamcmd::mark_retained_install_data(&fixture.original)?;
    assert!(app_steamcmd::has_retained_install_data(&fixture.original));
    sync_game_installs(
        &fixture.storage.paths,
        &[GameInstallSyncRecord {
            module_id: fixture.descriptor.summary.id.clone(),
            install_root: fixture.original.to_string_lossy().into_owned(),
            install_state: InstallState::NotInstalled,
            current_version: None,
            mark_verified: false,
        }],
    )
    .await?;
    let mut server = PackageServer::start().await?;
    let install = fixture.descriptor.install.as_mut().unwrap();
    install.download_url_windows = Some(server.url.clone());
    install.verification_path = Some("CoreKeeperServer.exe".into());
    install.download_integrity_windows = None;
    let module = map_module_details_with_install_state(
        &fixture.storage.settings,
        &fixture.descriptor,
        Some(&fixture.original.to_string_lossy()),
    );
    let operation = fixture
        .state
        .begin_storage_context_operation("retained reinstall test")?;
    let cancellation = app_steamcmd::InstallCancellation::new();
    let baseline = LibraryBaselineRecorder::new(&operation, &fixture.descriptor, &cancellation);
    let guard = acquire_library_program_mutation(
        &fixture.state,
        &fixture.storage,
        &fixture.descriptor.summary.id,
        &fixture.original,
    )
    .await?;
    assert!(!super::super::super::program_directory_is_empty(
        &fixture.original
    )?);
    let result = app_steamcmd::install_or_update_module_at_with_callbacks(
        &fixture.storage.settings,
        &module,
        &fixture.original,
        &guard.install,
        false,
        &cancellation,
        app_steamcmd::ModuleInstallCallbacks {
            on_progress: |_| {},
            prepare_fresh_payload: |root: PathBuf| {
                let baseline = &baseline;
                async move {
                    assert_eq!(
                        fs::read(
                            root.join("CoreKeeperServer_Data/StreamingAssets/Mods/default.txt")
                        )
                        .unwrap(),
                        b"shipped default"
                    );
                    baseline.prepare_fresh_payload(root, |_| {}).await
                }
            },
        },
    )
    .await?;
    (&mut server.task).await??;
    assert_eq!(result.install_state, InstallState::Installed);
    let guard = baseline
        .finish(guard, fixture.original.clone(), false, false, |_| {})
        .await?;
    sync_game_installs(
        &fixture.storage.paths,
        &[GameInstallSyncRecord {
            module_id: fixture.descriptor.summary.id.clone(),
            install_root: result.install_root,
            install_state: result.install_state,
            current_version: result.current_version,
            mark_verified: true,
        }],
    )
    .await?;
    drop(guard);
    assert!(app_storage::library_program_is_pristine(
        &fixture.original,
        &fixture.descriptor,
        None
    )?);
    assert_eq!(
        fs::read(mods.join("default.txt"))?,
        b"operator changed default"
    );
    assert_eq!(fs::read(mods.join("local.dll"))?, b"operator private mod");
    let initial_path = fixture.original.join(".langame-initial-package.json");
    assert!(
        !initial_path.exists(),
        "retained defaults must not be treated as untouched shipped defaults: {:?}",
        fs::read_to_string(initial_path)
    );

    let guard = fixture.guard().await?;
    for name in ["First after reinstall", "Second after reinstall"] {
        let (operation, job) = fixture.operation()?;
        let mut request = fixture.request(&operation, &guard, &job);
        request.input = fixture.input(name);
        let created = create_with_program_repair(request, |_, _, _| async {
            Err("a completed reinstall must not request a second download".into())
        })
        .await?;
        let binding =
            app_storage::read_instance_program_install(&fixture.storage.paths, &created.summary.id)
                .await?
                .unwrap();
        assert_eq!(
            binding.install.scope,
            app_storage::ProgramInstallScope::Instance
        );
        assert_eq!(
            fs::read(binding.install.install_root.join("CoreKeeperServer.exe"))?,
            b"official fixture program"
        );
        assert!(
            !binding
                .install
                .install_root
                .join("CoreKeeperServer_Data/StreamingAssets/Mods/local.dll")
                .exists()
        );
    }
    assert!(
        !fixture.repair.exists(),
        "creation must reuse the installed original"
    );
    assert_eq!(list_instances(&fixture.storage.paths).await?.len(), 2);
    Ok(())
}
