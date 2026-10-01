use super::*;
use crate::commands::commands_program_storage::{
    LibraryBaselineRecorder, acquire_library_program_mutation,
};
use std::io::{Cursor, Write};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct PackageServer {
    url: String,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl PackageServer {
    async fn start() -> TestResult<Self> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in [
            ("CoreKeeperServer.exe", "official fixture program"),
            (
                "CoreKeeperServer_Data/sharedassets0.assets",
                "official assets",
            ),
            (
                "CoreKeeperServer_Data/StreamingAssets/Mods/default.txt",
                "shipped default",
            ),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())?;
            zip.write_all(bytes.as_bytes())?;
        }
        let bytes = zip.finish()?.into_inner();
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
        Ok(Self { url, task })
    }
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
                    baseline.prepare_fresh_payload(root).await
                }
            },
        },
    )
    .await?;
    (&mut server.task).await??;
    assert_eq!(result.install_state, InstallState::Installed);
    let guard = baseline
        .finish(guard, fixture.original.clone(), false, false)
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
