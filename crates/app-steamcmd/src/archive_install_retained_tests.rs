use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[path = "archive_install_library_marker_tests.rs"]
mod library_marker_tests;

#[path = "archive_install_payload_callback_tests.rs"]
mod payload_callback_tests;

struct Fixture {
    root: PathBuf,
    settings: AppSettings,
    module: ModuleDetails,
}

impl Fixture {
    fn new() -> Self {
        let root = crate::tests::unique_test_root();
        let settings = AppSettings {
            archives_root: String::new(),
            games_root: root.join("games").to_string_lossy().into_owned(),
            servers_root: root.join("instances").to_string_lossy().into_owned(),
            modules_root: root.join("modules").to_string_lossy().into_owned(),
            steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
        };
        let module = ModuleDetails {
            summary: app_core::ModuleSummary {
                id: String::from("retained-fixture"),
                name: String::from("Retained Fixture"),
                version: String::from("1"),
                description: None,
                steam_app_id: None,
                install_state: InstallState::NotInstalled,
                instance_program_count: 0,
                archived_program_count: 0,
                supported_platforms: vec![String::from("windows")],
            },
            schema_json: None,
            default_ports: vec![],
            install: Some(InstallSpec {
                shared_game_dir: String::from("game"),
                verification_path: Some(String::from("bin/Server.exe")),
                download_url_windows: Some(String::from("http://127.0.0.1/fixture.zip")),
                download_integrity_windows: None,
                source: None,
                minecraft: None,
            }),
            process: Some(ProcessSpec {
                executable: String::from("{{config.server_executable}}"),
                args_template: vec![],
                environment_template: Default::default(),
                working_directory_template: None,
                window_policy: app_core::ProcessWindowPolicy::Background,
                host_surface: app_core::ProcessHostSurface::ManagedTerminal,
                host_notes: None,
            }),
            workshop: None,
            mods: None,
            runtime: app_core::ModuleRuntimeSpec::default(),
        };
        let fixture = Self {
            root,
            settings,
            module,
        };
        for (path, content) in [
            ("Assets/Worlds/world.sav", b"retained world".as_slice()),
            (
                "Assets/ServerConfig.json",
                b"retained native configuration".as_slice(),
            ),
            ("local/operator.txt", b"retained operator data".as_slice()),
        ] {
            let destination = fixture.install_root().join(path);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::write(destination, content).unwrap();
        }
        mark_retained_install_data(&fixture.install_root()).unwrap();
        fixture
    }

    fn install_root(&self) -> PathBuf {
        PathBuf::from(&self.settings.games_root).join("game")
    }

    fn assert_retained(&self, uninstalled: bool) {
        let root = self.install_root();
        assert_eq!(
            fs::read(root.join("Assets/Worlds/world.sav")).unwrap(),
            b"retained world"
        );
        assert_eq!(
            fs::read(root.join("Assets/ServerConfig.json")).unwrap(),
            b"retained native configuration"
        );
        assert_eq!(
            fs::read(root.join("local/operator.txt")).unwrap(),
            b"retained operator data"
        );
        assert_eq!(has_retained_install_data(&root), uninstalled);
    }

    async fn archive(&self, extra_entry: Option<&str>) -> Vec<u8> {
        let zip = self.root.join("fixture.zip");
        let mut script = format!(
            "$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.IO.Compression,System.IO.Compression.FileSystem; \
             $zip=[IO.Compression.ZipFile]::Open({}, [IO.Compression.ZipArchiveMode]::Create); try {{\n",
            ps_literal(&zip),
        );
        let mut entries = vec![
            ("bin/Server.exe", "new server payload"),
            ("Assets/ServerConfig.json", "package defaults"),
            ("Assets/package-only.txt", "new package file"),
            ("local", "package file replaced by retained directory"),
        ];
        if let Some(name) = extra_entry {
            entries.push((name, "outside"));
        }
        for (name, content) in entries {
            script.push_str(&format!(
                "$entry=$zip.CreateEntry('{}'); $stream=$entry.Open(); try {{ \
                 $bytes=[Text.Encoding]::UTF8.GetBytes('{}'); $stream.Write($bytes,0,$bytes.Length) \
                 }} finally {{ $stream.Dispose() }}\n",
                name.replace('\'', "''"), content.replace('\'', "''"),
            ));
        }
        script.push_str("} finally { $zip.Dispose() }");
        let result = run_powershell(&script, Some(&self.root), test_deadline())
            .await
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            output_excerpt(&result.stdout, &result.stderr)
        );
        fs::read(zip).unwrap()
    }

    async fn install(
        &self,
        url: &str,
        progress: &mut impl FnMut(InstallProgressUpdate),
    ) -> Result<ModuleInstallResult, SteamCmdError> {
        install_or_update_module_from_download(
            DirectDownloadInstallRequest {
                settings: &self.settings,
                module: &self.module,
                install: self.module.install.as_ref().unwrap(),
                process: self.module.process.as_ref().unwrap(),
                steam_app_id: 0,
                operation: "install",
                download_url: url,
                deadline: test_deadline(),
            },
            progress,
            &mut |_| async { Ok(()) },
        )
        .await
    }

    async fn assert_transaction_clean(&self) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let entries: Vec<_> = fs::read_dir(&self.settings.games_root)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            if entries == [std::ffi::OsString::from("game")] {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Transaction debris remains: {entries:?}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn test_deadline() -> InstallDeadline {
    InstallDeadline::new("retained ZIP fixture", Duration::from_secs(30))
}

struct PackageServer {
    url: String,
    task: tokio::task::JoinHandle<()>,
}

impl PackageServer {
    async fn new(body: Vec<u8>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/package.zip", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut stream, _) = tokio::time::timeout(Duration::from_secs(30), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut request = [0; 4096];
            tokio::time::timeout(Duration::from_secs(5), async {
                let mut length = 0;
                while !request[..length]
                    .windows(4)
                    .any(|bytes| bytes == b"\r\n\r\n")
                {
                    assert!(
                        length < request.len(),
                        "fixture request header is too large"
                    );
                    let count = stream.read(&mut request[length..]).await.unwrap();
                    assert!(count > 0, "fixture request ended before its headers");
                    length += count;
                }
            })
            .await
            .unwrap();
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes()).await.unwrap();
            stream.write_all(&body).await.unwrap();
            stream.shutdown().await.unwrap();
        });
        Self { url, task }
    }

    async fn finish(&mut self) {
        tokio::time::timeout(Duration::from_secs(5), &mut self.task)
            .await
            .unwrap()
            .unwrap();
    }
}

impl Drop for PackageServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn retained_archive_reinstall_merges_native_config_and_saves_before_publication() {
    use sha2::{Digest, Sha256};

    let mut fixture = Fixture::new();
    let archive = fixture.archive(None).await;
    fixture
        .module
        .install
        .as_mut()
        .unwrap()
        .download_integrity_windows = Some(app_core::DownloadIntegritySpec {
        sha256: Sha256::digest(&archive)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        size: archive.len() as u64,
    });
    let mut server = PackageServer::new(archive).await;
    let mut saw_rollback = false;
    let result = fixture
        .install(&server.url, &mut |update| {
            if update
                .install_progress
                .as_ref()
                .is_some_and(|progress| progress.phase == InstallPhase::Verifying)
                && !has_retained_install_data(&fixture.install_root())
            {
                let rollback = fs::read_dir(&fixture.settings.games_root)
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .find(|path| {
                        let name = path.file_name().unwrap().to_string_lossy();
                        name.starts_with(".lg-") && name.ends_with(".rollback")
                    })
                    .unwrap();
                assert_eq!(
                    fs::read(rollback.join("Assets/Worlds/world.sav")).unwrap(),
                    b"retained world"
                );
                assert!(has_retained_install_data(&rollback));
                saw_rollback = true;
            }
        })
        .await
        .unwrap();
    server.finish().await;
    assert!(
        saw_rollback,
        "Original data must remain as rollback through verification"
    );
    assert_eq!(result.install_state, InstallState::Installed);
    fixture.assert_retained(false);
    assert_eq!(
        fs::read(fixture.install_root().join("bin/Server.exe")).unwrap(),
        b"new server payload"
    );
    assert_eq!(
        fs::read(fixture.install_root().join("Assets/package-only.txt")).unwrap(),
        b"new package file"
    );
    fixture.assert_transaction_clean().await;
}

#[tokio::test]
async fn retained_archive_integrity_failure_never_extracts_or_publishes() {
    use sha2::{Digest, Sha256};

    for wrong_hash in [true, false] {
        let mut fixture = Fixture::new();
        let archive = fixture.archive(None).await;
        fixture
            .module
            .install
            .as_mut()
            .unwrap()
            .download_integrity_windows = Some(app_core::DownloadIntegritySpec {
            sha256: if wrong_hash {
                "0".repeat(64)
            } else {
                Sha256::digest(&archive)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect()
            },
            size: archive.len() as u64 + u64::from(!wrong_hash),
        });
        let mut server = PackageServer::new(archive).await;
        let mut extracted = false;
        let result = fixture
            .install(&server.url, &mut |update| {
                extracted |= update.install_progress.as_ref().is_some_and(|progress| {
                    matches!(
                        progress.phase,
                        InstallPhase::Extracting | InstallPhase::Verifying
                    )
                });
            })
            .await;
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains(if wrong_hash {
                "SHA-256 checksum mismatch"
            } else {
                "download size mismatch"
            }),
            "{error}"
        );
        assert!(!extracted, "unverified archive reached extraction");
        fixture.assert_retained(true);
        assert!(!fixture.install_root().join("bin/Server.exe").exists());
        fixture.assert_transaction_clean().await;
        server.finish().await;
    }
}

#[tokio::test]
async fn retained_archive_failed_probe_restores_original_data_and_marker() {
    let fixture = Fixture::new();
    let mut server = PackageServer::new(fixture.archive(None).await).await;
    let result = fixture
        .install(&server.url, &mut |update| {
            if update
                .install_progress
                .as_ref()
                .is_some_and(|progress| progress.phase == InstallPhase::Verifying)
                && !has_retained_install_data(&fixture.install_root())
            {
                fs::remove_file(fixture.install_root().join("bin/Server.exe")).unwrap();
            }
        })
        .await;
    server.finish().await;
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallationVerificationFailed { .. })
    ));
    fixture.assert_retained(true);
    assert!(!fixture.install_root().join("bin").exists());
    fixture.assert_transaction_clean().await;
}

#[tokio::test]
async fn retained_archive_cancellation_after_publish_restores_original_data_and_marker() {
    let fixture = Fixture::new();
    let mut server = PackageServer::new(fixture.archive(None).await).await;
    let cancellation = InstallCancellation::new();
    let result = cancellation
        .scope(fixture.install(&server.url, &mut |update| {
            if update
                .install_progress
                .as_ref()
                .is_some_and(|progress| progress.phase == InstallPhase::Verifying)
                && !has_retained_install_data(&fixture.install_root())
            {
                cancellation.cancel();
            }
        }))
        .await;
    server.finish().await;
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    fixture.assert_retained(true);
    assert!(!fixture.install_root().join("bin").exists());
    fixture.assert_transaction_clean().await;
}

#[tokio::test]
async fn retained_archive_rejects_verification_ancestor_conflict_without_losing_data() {
    let fixture = Fixture::new();
    fs::write(
        fixture.install_root().join("bin"),
        b"retained data at package directory",
    )
    .unwrap();
    let mut server = PackageServer::new(fixture.archive(None).await).await;
    let error = fixture.install(&server.url, &mut |_| {}).await.unwrap_err();
    server.finish().await;
    assert!(
        error
            .to_string()
            .contains("conflicts with the required server verification file"),
        "{error}"
    );
    fixture.assert_retained(true);
    assert_eq!(
        fs::read(fixture.install_root().join("bin")).unwrap(),
        b"retained data at package directory"
    );
    fixture.assert_transaction_clean().await;
}

#[tokio::test]
async fn retained_archive_rejects_existing_verification_file_before_download() {
    let fixture = Fixture::new();
    let verification = fixture.install_root().join("bin/Server.exe");
    fs::create_dir(verification.parent().unwrap()).unwrap();
    fs::write(
        &verification,
        b"retained data cannot verify the new package",
    )
    .unwrap();
    // Port 0 cannot provide a package: this conflict must be rejected before HTTP.
    let error = fixture
        .install("http://127.0.0.1:0/package.zip", &mut |_| {})
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("conflicts with the required server payload"),
        "{error}"
    );
    fixture.assert_retained(true);
    assert_eq!(
        fs::read(verification).unwrap(),
        b"retained data cannot verify the new package"
    );
    fixture.assert_transaction_clean().await;
}

#[tokio::test]
async fn retained_archive_rejects_source_hardlink_without_changing_target() {
    let fixture = Fixture::new();
    let outside = fixture.root.join("outside.txt");
    fs::write(&outside, b"untouched hardlink target").unwrap();
    fs::hard_link(&outside, fixture.install_root().join("linked.txt")).unwrap();
    let mut server = PackageServer::new(fixture.archive(None).await).await;
    let error = fixture.install(&server.url, &mut |_| {}).await.unwrap_err();
    server.finish().await;
    assert!(
        error.to_string().contains("link or reparse point"),
        "{error}"
    );
    fixture.assert_retained(true);
    assert_eq!(fs::read(outside).unwrap(), b"untouched hardlink target");
    fixture.assert_transaction_clean().await;
}

#[tokio::test]
async fn retained_archive_rejects_source_junction_without_accessing_its_target() {
    let fixture = Fixture::new();
    let outside = fixture.root.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"untouched").unwrap();
    let script = format!(
        "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path {} -Target {} | Out-Null",
        ps_literal(&fixture.install_root().join("linked")),
        ps_literal(&outside)
    );
    let linked = run_powershell(&script, Some(&fixture.root), test_deadline())
        .await
        .unwrap();
    assert!(
        linked.status.success(),
        "{}",
        output_excerpt(&linked.stdout, &linked.stderr)
    );
    let mut server = PackageServer::new(fixture.archive(None).await).await;
    let error = fixture.install(&server.url, &mut |_| {}).await.unwrap_err();
    server.finish().await;
    assert!(
        error.to_string().contains("link or reparse point"),
        "{error}"
    );
    fixture.assert_retained(true);
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"untouched");
    fixture.assert_transaction_clean().await;
}

#[tokio::test]
async fn retained_archive_rejects_zip_path_escape_without_publication() {
    let fixture = Fixture::new();
    let mut server = PackageServer::new(fixture.archive(Some("../escape.txt")).await).await;
    let error = fixture.install(&server.url, &mut |_| {}).await.unwrap_err();
    server.finish().await;
    assert!(
        error.to_string().contains("escapes the staging directory"),
        "{error}"
    );
    fixture.assert_retained(true);
    assert!(
        !Path::new(&fixture.settings.games_root)
            .join("escape.txt")
            .exists()
    );
    fixture.assert_transaction_clean().await;
}
