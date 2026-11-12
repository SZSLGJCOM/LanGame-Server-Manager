use super::*;
use crate::commands::commands_save_startup as save_startup;
use sha1::{Digest, Sha1};
use sha2::Sha256;
use std::io::{Cursor, Write};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

const PAYLOAD: &[u8] = b"abc";
const OPERATOR: &[u8] = b"operator configuration must survive";
const WORLD: &[u8] = b"existing world must survive";
const JAVA_RUNTIME_LIBRARY: &[u8] = b"fixture runtime library";

fn workspace() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
}

fn entrypoint(descriptor: &ModuleDescriptor) -> &str {
    descriptor
        .install
        .as_ref()
        .and_then(|install| install.verification_path.as_deref())
        .unwrap_or(&descriptor.process.as_ref().unwrap().executable)
}

fn complete_acf(app_id: u32) -> String {
    format!(
        "\"AppState\" {{ \"appid\" \"{app_id}\" \"StateFlags\" \"4\" \"buildid\" \"17\" \"TargetBuildID\" \"17\" }}"
    )
}

fn write_file(path: &Path, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, bytes)
}

fn operator_path(fixture: &Fixture) -> PathBuf {
    match fixture.descriptor.summary.id.as_str() {
        "rust" => fixture.program.join(format!(
            "server/{}/cfg/server.cfg",
            fixture.instance.summary.id
        )),
        "palworld" => fixture
            .program
            .join("Pal/Saved/Config/WindowsServer/PalWorldSettings.ini"),
        "abioticfactor" => fixture.program.join(format!(
            "AbioticFactor/Saved/Config/WindowsServer/LanGame/{}-SandboxSettings.ini",
            fixture.instance.summary.id
        )),
        "rimworld" => fixture.program.join("Configs/Server.json"),
        "minecraft" => crate::commands::commands_program_storage::instance_root(&fixture.instance)
            .unwrap()
            .join("server.properties"),
        module => panic!("unhandled fixture module {module}"),
    }
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The provider boundary is a real child process. It repairs the declared game
/// executable and optional original files only when startup requests validate;
/// it never decides eligibility.
async fn native_provider() -> TestResult<Vec<u8>> {
    let scratch = Scratch(
        std::env::temp_dir().join(format!("cross-native-{}", uuid::Uuid::new_v4().simple())),
    );
    fs::create_dir_all(&scratch.0)?;
    let source = scratch.0.join("provider.cs");
    fs::write(
        &source,
        r#"
using System;
using System.IO;
public static class Provider {
  private static string NativePath(string path) {
    string full = Path.GetFullPath(path.Replace('/', '\\'));
    if (full.StartsWith(@"\\?\", StringComparison.Ordinal)) return full;
    return full.StartsWith(@"\\", StringComparison.Ordinal)
      ? @"\\?\UNC\" + full.Substring(2) : @"\\?\" + full;
  }
  public static int Main(string[] args) {
    // Match the long paths supported by the production Rust/Win32 installer.
    // Set these before any System.IO path operation in this .NET fixture.
    AppContext.SetSwitch("Switch.System.IO.UseLegacyPathHandling", false);
    AppContext.SetSwitch("Switch.System.IO.BlockLongPaths", false);
    if (Array.IndexOf(args, "-version") >= 0) {
      Console.WriteLine("openjdk version \"21.0.1\"");
      return 0;
    }
    int script = Array.IndexOf(args, "+runscript");
    if (script < 0) {
      Console.WriteLine("Steam Console Client (c) Valve Corporation\nLoading Steam API...OK");
      return 0;
    }
    string home = NativePath(AppDomain.CurrentDomain.BaseDirectory);
    string[] spec = File.ReadAllLines(Path.Combine(home, "provider-spec"));
    string root = null, update = null;
    foreach (string line in File.ReadAllLines(NativePath(args[script + 1]))) {
      if (line.StartsWith("force_install_dir ")) root = line.Substring(18).Trim().Trim('"');
      if (line.StartsWith("app_update ")) update = line.Trim();
    }
    if (root == null || update == null || !update.StartsWith("app_update " + spec[0])) return 80;
    root = NativePath(root);
    File.AppendAllText(Path.Combine(home, "native-commands"), update + "\n");
    if (File.Exists(Path.Combine(home, "reject-update"))) {
      Console.WriteLine("ERROR! Failed to install app '" + spec[0] + "' (No subscription)");
      return 0;
    }
    string executable = NativePath(Path.Combine(root, spec[1]));
    if (update.EndsWith(" validate")) {
      Directory.CreateDirectory(Path.GetDirectoryName(executable));
      File.WriteAllText(executable, "abc");
      string originals = Path.Combine(home, "provider-original-files");
      if (File.Exists(originals)) {
        foreach (string relative in File.ReadAllLines(originals)) {
          string destination = NativePath(Path.Combine(root, relative));
          Directory.CreateDirectory(Path.GetDirectoryName(destination));
          File.Copy(NativePath(Path.Combine(home, "provider-originals", relative)), destination, true);
        }
      }
    }
    // Deliberately overwrite a declared mutable path. The actual installation
    // preservation transaction must restore this instance's operator content.
    string config = NativePath(Path.Combine(root, spec[2]));
    Directory.CreateDirectory(Path.GetDirectoryName(config));
    File.WriteAllText(config, "installer default must not replace operator data");
    Directory.CreateDirectory(Path.Combine(root, "steamapps"));
    File.WriteAllText(Path.Combine(root, "steamapps", "appmanifest_" + spec[0] + ".acf"),
      "\"AppState\" { \"appid\" \"" + spec[0] + "\" \"StateFlags\" \"4\" \"buildid\" \"17\" \"TargetBuildID\" \"17\" }");
    Console.WriteLine("Success! App '" + spec[0] + "' fully installed.");
    return 0;
  }
}
"#,
    )?;
    let executable = scratch.0.join("provider.exe");
    let output_path = executable.clone();
    let compiler = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("Microsoft.NET/Framework64/v4.0.30319/csc.exe");
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(compiler)
            .args(["/nologo", "/target:exe"])
            .arg(format!("/out:{}", output_path.display()))
            .arg(source)
            .creation_flags(0x0800_0000)
            .output()
    })
    .await??;
    assert!(output.status.success(), "{output:?}");
    Ok(fs::read(executable)?)
}

struct LocalSource {
    base: String,
    archive: Vec<u8>,
    java_archive: Vec<u8>,
    requests: Arc<Mutex<Vec<String>>>,
    fail_download: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<Result<(), String>>,
}

impl Drop for LocalSource {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl LocalSource {
    async fn new(native: &[u8]) -> TestResult<Self> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (path, bytes) in [
            ("RTServer.exe", PAYLOAD),
            ("Configs/Server.json", b"installer default".as_slice()),
        ] {
            zip.start_file(path, zip::write::SimpleFileOptions::default())?;
            zip.write_all(bytes)?;
        }
        let archive = zip.finish()?.into_inner();
        let mut java_zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (path, bytes) in [
            ("jdk/bin/java.exe", native),
            ("jdk/lib/modules", JAVA_RUNTIME_LIBRARY),
        ] {
            java_zip.start_file(path, zip::write::SimpleFileOptions::default())?;
            java_zip.write_all(bytes)?;
        }
        let java_archive = java_zip.finish()?.into_inner();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let base = format!("http://{}", listener.local_addr()?);
        let details = serde_json::to_vec(&json!({
            "id": "17", "javaVersion": {"majorVersion": 21},
            "downloads": {"server": {"url": format!("{base}/server.jar"),
                "size": PAYLOAD.len(), "sha1": Sha1::digest(PAYLOAD).iter().map(|byte| format!("{byte:02x}")).collect::<String>()}}
        }))?;
        let manifest = serde_json::to_vec(&json!({
            "latest": {"release": "17", "snapshot": "17"},
            "versions": [{"id": "17", "type": "release", "url": format!("{base}/version.json"),
                "sha1": Sha1::digest(&details).iter().map(|byte| format!("{byte:02x}")).collect::<String>()}]
        }))?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = requests.clone();
        let fail_download = Arc::new(AtomicBool::new(false));
        let fail = fail_download.clone();
        let zip_body = archive.clone();
        let java_body = java_archive.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
                let mut request = Vec::new();
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let mut chunk = [0; 1024];
                    let read = stream
                        .read(&mut chunk)
                        .await
                        .map_err(|error| error.to_string())?;
                    if read == 0 || request.len() + read > 8192 {
                        return Err("invalid cross-module HTTP request".into());
                    }
                    request.extend_from_slice(&chunk[..read]);
                }
                let path = String::from_utf8_lossy(&request)
                    .split_whitespace()
                    .nth(1)
                    .ok_or("missing route")?
                    .to_owned();
                observed.lock().unwrap().push(path.clone());
                let body = match path.as_str() {
                    "/manifest.json" => manifest.as_slice(),
                    "/version.json" => details.as_slice(),
                    "/server.jar" => PAYLOAD,
                    "/package.zip" => zip_body.as_slice(),
                    "/java.zip" => java_body.as_slice(),
                    _ => return Err(format!("unexpected route {path}")),
                };
                let failed = matches!(path.as_str(), "/server.jar" | "/package.zip")
                    && fail.load(Ordering::Acquire);
                let (status, body) = if failed {
                    (503, b"download unavailable".as_slice())
                } else {
                    (200, body)
                };
                let header = format!(
                    "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream
                    .write_all(header.as_bytes())
                    .await
                    .map_err(|error| error.to_string())?;
                stream
                    .write_all(body)
                    .await
                    .map_err(|error| error.to_string())?;
                stream.shutdown().await.map_err(|error| error.to_string())?;
            }
        });
        Ok(Self {
            base,
            archive,
            java_archive,
            requests,
            fail_download,
            task,
        })
    }

    fn downloads(&self) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|path| matches!(path.as_str(), "/server.jar" | "/package.zip"))
            .count()
    }

    fn with_java_package<T>(
        &self,
        future: impl std::future::Future<Output = T>,
    ) -> impl std::future::Future<Output = T> {
        app_steamcmd::with_minecraft_java_package_fixture(
            format!("{}/java.zip", self.base),
            Sha256::digest(&self.java_archive)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            self.java_archive.len() as u64,
            future,
        )
    }
}

/// The current catalog correctly rejects new shared Abiotic Factor instances. Build
/// this compatibility input from a real created instance, then reproduce only
/// its historical persisted binding. Production update and detach remain real.
async fn bind_legacy_shared_abioticfactor(
    storage: &StorageBootstrap,
    descriptor: &ModuleDescriptor,
    library: &Path,
    instance_id: &str,
) -> TestResult {
    assert_eq!(descriptor.summary.id, "abioticfactor");
    assert_eq!(
        descriptor.storage.program_sharing,
        app_modules::ModuleProgramSharing::Independent
    );
    assert_eq!(
        descriptor.runtime.program_sharing,
        app_core::InstanceProgramMode::Independent
    );
    let instance = read_instance_details(&storage.paths, instance_id).await?;
    let root = crate::commands::commands_program_storage::instance_root(&instance)?;
    let private = root.join("runtime");
    let backup = root.join("fixture-original-private-runtime");
    let fixture_root = fs::canonicalize(&storage.paths.instances_root)?;
    assert!(fs::canonicalize(root)?.starts_with(&fixture_root));
    assert_eq!(
        fs::canonicalize(&private)?.parent(),
        Some(fs::canonicalize(root)?.as_path())
    );
    assert_eq!(backup.parent(), Some(root));
    assert!(!backup.exists());
    let config_before = fs::read(&instance.config_file_path)?;
    let previous = app_storage::read_instance_program_install(&storage.paths, instance_id)
        .await?
        .unwrap();
    assert_eq!(
        previous.install.scope,
        app_storage::ProgramInstallScope::Instance
    );
    assert_eq!(
        previous.install.owner_instance_id.as_deref(),
        Some(instance_id)
    );
    let library_record = app_storage::read_library_program_install(&storage.paths, "abioticfactor")
        .await?
        .unwrap();

    // Retain the complete generated private tree within this fixture. These two
    // version-1 records are the historical on-disk format, not a new test API.
    fs::rename(&private, &backup)?;
    let program_id = uuid::Uuid::new_v4().to_string();
    let identity = library.join(".langame-program-identity.json");
    assert!(!identity.exists());
    write_file(
        &identity,
        serde_json::to_vec(&json!({
            "version": 1, "id": program_id, "module_id": "abioticfactor"
        }))?,
    )?;
    write_file(
        &private.join(".langame-shared-program.json"),
        serde_json::to_vec(&json!({
            "version": 1, "program_id": program_id, "module_id": "abioticfactor",
            "program_root": fs::canonicalize(library)?
        }))?,
    )?;
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&storage.paths.database_path)
                .foreign_keys(true),
        )
        .await?;
    let mut tx = pool.begin().await?;
    // This row belongs solely to the just-created test instance. Removing it
    // permits the public binding API to establish the old shared ownership.
    let removed = sqlx::query(
        "DELETE FROM game_installs WHERE id = ?1 AND scope = 'instance' AND owner_instance_id = ?2",
    )
    .bind(previous.install.id)
    .bind(instance_id)
    .execute(&mut *tx)
    .await?;
    assert_eq!(removed.rows_affected(), 1);
    app_storage::bind_shared_install_to_instance(
        &mut tx,
        "abioticfactor",
        library_record.id,
        instance_id,
    )
    .await?;
    let settings: Value = serde_json::from_str(&instance.settings_json)?;
    let world = settings["world_save_name"].as_str().unwrap();
    let saves = library.join(format!(
        "AbioticFactor/Saved/SaveGames/Server/Worlds/{world}"
    ));
    sqlx::query("UPDATE instances SET saves_path = ?1 WHERE id = ?2")
        .bind(saves.to_string_lossy().as_ref())
        .bind(instance_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    pool.close().await;
    assert_eq!(fs::read(&instance.config_file_path)?, config_before);
    assert!(backup.join(entrypoint(descriptor)).is_file());
    assert_eq!(
        app_storage::instance_program_mode(root)?,
        app_core::InstanceProgramMode::Shared
    );
    Ok(())
}

impl Fixture {
    async fn for_module(
        module_id: &str,
        shared: bool,
        native: &[u8],
        source: Option<&LocalSource>,
    ) -> TestResult<Self> {
        Self::for_module_with_original_files(module_id, shared, native, source, false, &[]).await
    }

    async fn for_module_with_original_files(
        module_id: &str,
        shared: bool,
        native: &[u8],
        source: Option<&LocalSource>,
        prefer_existing: bool,
        original_files: &[(&str, &[u8])],
    ) -> TestResult<Self> {
        assert!(!(shared && prefer_existing));
        // Native .NET and Expand-Archive fixtures still observe MAX_PATH. Keep
        // the harness prefix short while retaining every real module subpath.
        let root =
            std::env::temp_dir().join(format!("x-{:016x}", uuid::Uuid::new_v4().as_u128() as u64));
        let paths = app_storage::StoragePaths {
            app_data_root: root.join("app-data"),
            settings_path: root.join("app-data/settings.json"),
            database_path: root.join("app-data/db/lgs.db"),
            logs_root: root.join("app-data/logs"),
            modules_root: workspace().join("modules"),
            migrations_root: workspace().join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances/.trash"),
        };
        let storage = app_storage::bootstrap_storage_with_paths(paths)?;
        initialize_database(&storage.paths).await?;
        let mut descriptor = discover_modules(&storage.paths.modules_root)?
            .into_iter()
            .find(|module| module.summary.id == module_id)
            .unwrap();
        if let Some(source) = source {
            let install = descriptor.install.as_mut().unwrap();
            if module_id == "minecraft" {
                install.minecraft.as_mut().unwrap().manifest_url =
                    Some(format!("{}/manifest.json", source.base));
            } else {
                install.download_url_windows = Some(format!("{}/package.zip", source.base));
                install.download_integrity_windows = Some(app_core::DownloadIntegritySpec {
                    sha256: Sha256::digest(&source.archive)
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect(),
                    size: source.archive.len() as u64,
                });
            }
        }
        sync_modules(&storage.paths, std::slice::from_ref(&descriptor)).await?;
        let library = storage
            .paths
            .games_root
            .join(&descriptor.install.as_ref().unwrap().shared_game_dir);
        write_file(&library.join(entrypoint(&descriptor)), PAYLOAD)?;
        for (relative, bytes) in original_files {
            write_file(&library.join(relative), bytes)?;
        }
        if let Some(app_id) = descriptor.summary.steam_app_id {
            write_file(
                &library.join(format!("steamapps/appmanifest_{app_id}.acf")),
                complete_acf(app_id),
            )?;
        }
        if module_id == "minecraft" {
            let source = source.unwrap();
            write_file(&library.join("jre/bin/java.exe"), native)?;
            write_file(
                &library.join(".langame/minecraft-server.json"),
                serde_json::to_vec(&json!({
                    "version_id": "17", "version_type": "release", "manifest_url": format!("{}/manifest.json", source.base),
                    "version_url": format!("{}/version.json", source.base), "server_url": format!("{}/server.jar", source.base),
                    "server_sha1": Sha1::digest(PAYLOAD).iter().map(|byte| format!("{byte:02x}")).collect::<String>(), "server_size": PAYLOAD.len(),
                    "server_jar": "server.jar", "required_java_major": 21, "downloaded_at_unix_ms": 0
                }))?,
            )?;
        }
        app_storage::record_library_program_baseline(&library, &descriptor, true, None)?;
        sync_game_installs(
            &storage.paths,
            &[GameInstallSyncRecord {
                module_id: module_id.into(),
                install_root: library.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: Some("17".into()),
                mark_verified: true,
            }],
        )
        .await?;
        let created = app_storage::create_instance_with_options(
            &storage.paths,
            &descriptor,
            CreateInstanceInput {
                name: "T".into(),
                module_id: module_id.into(),
            },
            app_storage::InstanceCreationOptions {
                program_mode: Some(app_core::InstanceProgramMode::Independent),
                prefer_existing_install: prefer_existing,
                require_clean_program: true,
                ..Default::default()
            },
        )
        .await?;
        if shared {
            bind_legacy_shared_abioticfactor(
                &storage,
                &descriptor,
                &library,
                &created.provisioning.summary.id,
            )
            .await?;
        }
        let instance =
            read_instance_details(&storage.paths, &created.provisioning.summary.id).await?;
        let program = PathBuf::from(
            crate::commands::commands_runtime_lifecycle::private_runtime_install_root(&instance)?,
        );
        assert_eq!(
            fs::canonicalize(&program)? == fs::canonicalize(&library)?,
            shared || prefer_existing
        );
        let fixture = Self {
            root,
            storage,
            descriptor,
            instance,
            program,
            library,
        };
        write_file(&operator_path(&fixture), OPERATOR)?;
        write_file(
            &Path::new(&fixture.instance.saves_path).join("world.sentinel"),
            WORLD,
        )?;
        if let Some(app_id) = fixture.descriptor.summary.steam_app_id {
            write_file(
                &fixture.storage.paths.steamcmd_root.join("steamcmd.exe"),
                native,
            )?;
            let config = operator_path(&fixture);
            let relative = config.strip_prefix(&fixture.program)?.to_string_lossy();
            write_file(
                &fixture.storage.paths.steamcmd_root.join("provider-spec"),
                format!(
                    "{app_id}\n{}\n{relative}\n",
                    entrypoint(&fixture.descriptor)
                ),
            )?;
            if !original_files.is_empty() {
                for (relative, bytes) in original_files {
                    write_file(
                        &fixture
                            .storage
                            .paths
                            .steamcmd_root
                            .join("provider-originals")
                            .join(relative),
                        bytes,
                    )?;
                }
                write_file(
                    &fixture
                        .storage
                        .paths
                        .steamcmd_root
                        .join("provider-original-files"),
                    original_files
                        .iter()
                        .map(|(relative, _)| *relative)
                        .collect::<Vec<_>>()
                        .join("\n"),
                )?;
            }
        }
        assert_eq!(
            fixture.module().summary.install_state,
            InstallState::Installed,
            "{module_id}: the original fixture must be installed",
        );
        Ok(fixture)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Damage {
    Healthy,
    Missing,
    Changed,
    Incomplete,
}

fn damage(fixture: &Fixture, kind: Damage) -> TestResult {
    let artifact = fixture.program.join(entrypoint(&fixture.descriptor));
    match kind {
        Damage::Healthy => {}
        Damage::Missing => fs::remove_file(&artifact)?,
        Damage::Changed => fs::write(&artifact, b"xyz")?,
        Damage::Incomplete => {
            fs::write(&artifact, b"xyz")?;
            let app_id = fixture.descriptor.summary.steam_app_id.unwrap();
            // FullyInstalled | UpdateRequired alone can describe a usable build.
            // Record actual unfinished payload work, as an interrupted repair does.
            fs::write(
                fixture
                    .program
                    .join(format!("steamapps/appmanifest_{app_id}.acf")),
                complete_acf(app_id)
                    .replace("\"StateFlags\" \"4\"", "\"StateFlags\" \"6\"")
                    .replacen(
                        '{',
                        "{ \"BytesToDownload\" \"50\" \"BytesDownloaded\" \"1\"",
                        1,
                    ),
            )?;
        }
    }
    if kind != Damage::Healthy {
        assert!(
            !app_storage::library_program_is_pristine(&fixture.program, &fixture.descriptor, None)?,
            "{} {kind:?}: damaged files must fail the original inventory",
            fixture.descriptor.summary.id,
        );
    }
    if kind == Damage::Missing {
        assert_ne!(
            fixture.module().summary.install_state,
            InstallState::Installed,
            "{} {kind:?}: the missing entry must not probe installed",
            fixture.descriptor.summary.id,
        );
    }
    if kind == Damage::Incomplete {
        assert_eq!(
            fixture.module().summary.install_state,
            InstallState::Incomplete,
            "{} {kind:?}: unfinished payload work must probe incomplete",
            fixture.descriptor.summary.id,
        );
    }
    Ok(())
}

async fn exercise(
    fixture: &mut Fixture,
    damage_kind: Damage,
    pinned: bool,
    reject: bool,
) -> TestResult {
    damage(fixture, damage_kind)?;
    if pinned {
        let mut settings: Value = serde_json::from_str(&fixture.instance.settings_json)?;
        settings["program_update"] = json!({"policy": "pinned"});
        fixture.instance.settings_json = serde_json::to_string(&settings)?;
    }
    if reject && fixture.descriptor.summary.steam_app_id.is_some() {
        fs::write(
            fixture.storage.paths.steamcmd_root.join("reject-update"),
            b"",
        )?;
    }
    let module = fixture.module();
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    if fixture.descriptor.summary.steam_app_id.is_some() {
        app_steamcmd::ensure_steamcmd_installed(&state.install_locks, &fixture.storage.settings)
            .await?;
    }
    let id = &fixture.instance.summary.id;
    let reservation = match state.try_reserve_runtime_start(id, "manual")? {
        RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        other => panic!("unexpected reservation: {other:?}"),
    };
    let _instance = state.acquire_instance_mutation(id).await;
    let guard = app_steamcmd::acquire_game_install_lifecycle(
        &state.install_locks,
        &fixture.descriptor.summary.id,
        std::slice::from_ref(&fixture.program),
    )
    .await?;
    let result = tokio::time::timeout(
        BUDGET,
        run_prestart_update_if_needed(
            &state,
            &fixture.storage,
            PrestartUpdateRequest {
                descriptor: &fixture.descriptor,
                module: &module,
                instance: &fixture.instance,
                install_guard: guard,
                reservation: &reservation,
                context: PrestartUpdateLogContext {
                    source: "manual",
                    console_log_path: None,
                },
            },
        ),
    )
    .await
    .expect("cross-module prestart must settle");
    let after = fixture.module();
    let jobs = state.app_state.read().unwrap().jobs.clone();
    let commands = fixture.storage.paths.steamcmd_root.join("native-commands");
    if pinned || !module_supports_automatic_prestart_update(&module) {
        assert!(result.is_ok());
        assert!(jobs.is_empty());
        assert!(!commands.exists());
        assert_eq!(
            ensure_module_installed_for_start(&after).is_ok(),
            damage_kind == Damage::Healthy
        );
    } else if reject {
        assert!(result.is_err(), "failed repair must not authorize startup");
        assert_eq!(jobs.len(), 1);
        assert!(matches!(jobs[0].status, JobStatus::Failed));
        assert!(
            jobs[0].warnings.is_empty(),
            "damaged files cannot fall back to installed"
        );
        assert!(!app_storage::library_program_is_pristine(
            &fixture.program,
            &fixture.descriptor,
            None
        )?);
    } else {
        let outcome = result.map_err(|error| {
            format!("{} {damage_kind:?}: {error}", fixture.descriptor.summary.id)
        })?;
        outcome
            .install_guard
            .ensure_scope(&fixture.descriptor.summary.id, &fixture.program)?;
        assert!(outcome.warnings.is_empty());
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].target_id.as_deref(), Some(id.as_str()));
        assert!(matches!(jobs[0].status, JobStatus::Completed));
        ensure_module_installed_for_start(&after)?;
        assert_eq!(
            fs::read(fixture.program.join(entrypoint(&fixture.descriptor)))?,
            PAYLOAD
        );
        assert!(app_storage::library_program_is_pristine(
            &fixture.program,
            &fixture.descriptor,
            None
        )?);
        app_steamcmd::read_program_install_revision(
            &fixture.storage.paths.instances_root,
            &fixture.descriptor.summary.id,
            &fixture.program,
        )?;
        if let Some(app_id) = fixture.descriptor.summary.steam_app_id {
            let suffix = if damage_kind == Damage::Healthy {
                ""
            } else {
                " validate"
            };
            assert_eq!(
                fs::read_to_string(commands)?,
                format!("app_update {app_id}{suffix}\n")
            );
        }
    }
    assert_eq!(fs::read(operator_path(fixture))?, OPERATOR);
    assert_eq!(
        fs::read(Path::new(&fixture.instance.saves_path).join("world.sentinel"))?,
        WORLD
    );
    assert_eq!(
        fs::read(fixture.library.join(entrypoint(&fixture.descriptor)))?,
        PAYLOAD
    );
    assert!(
        read_active_instance_run(&fixture.storage.paths, id)
            .await?
            .is_none()
    );
    assert!(!state.runtime_supervisor.lock().unwrap().is_tracked(id));
    Ok(())
}

#[test]
fn shipped_catalog_allows_repairable_states_for_all_managed_sources() -> TestResult {
    let descriptors = discover_modules(workspace().join("modules"))?;
    let settings = app_core::AppSettings::default();
    let absent_root = std::env::temp_dir().join(format!("cross-catalog-{}", uuid::Uuid::new_v4()));
    assert_eq!(descriptors.len(), 32);
    assert_eq!(
        descriptors
            .iter()
            .filter(|module| module.summary.steam_app_id.is_some())
            .count(),
        29
    );
    for descriptor in descriptors {
        let mut module = map_module_details_with_install_state(
            &settings,
            &descriptor,
            Some(&absent_root.to_string_lossy()),
        );
        assert!(
            module_supports_automatic_prestart_update(&module),
            "{}",
            descriptor.summary.id
        );
        for state in [
            InstallState::Installed,
            InstallState::Incomplete,
            InstallState::Corrupted,
            InstallState::NotInstalled,
        ] {
            module.summary.install_state = state;
            assert!(
                should_run_prestart_update(&module),
                "{}",
                descriptor.summary.id
            );
        }
        for state in [
            InstallState::Installing,
            InstallState::Updating,
            InstallState::Uninstalling,
        ] {
            module.summary.install_state = state;
            assert!(
                !should_run_prestart_update(&module),
                "{}",
                descriptor.summary.id
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn steam_rust_palworld_and_abioticfactor_repair_same_build_and_preserve_operator_data()
-> TestResult {
    let _serial = crate::commands::tests::command_smoke_lock().lock().await;
    let native = native_provider().await?;
    assert!(!save_startup::requires_private_program("rust"));
    assert!(!save_startup::requires_private_program("palworld"));
    assert!(save_startup::requires_private_program("abioticfactor"));
    for module in ["rust", "palworld", "abioticfactor"] {
        for (damage, pinned, reject) in [
            (Damage::Healthy, false, false),
            (Damage::Missing, false, false),
            (Damage::Changed, false, false),
            (Damage::Incomplete, false, false),
            (Damage::Healthy, true, false),
            (Damage::Missing, true, false),
            (Damage::Changed, false, true),
        ] {
            let mut fixture = Fixture::for_module(module, false, &native, None).await?;
            exercise(&mut fixture, damage, pinned, reject).await?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn archive_and_minecraft_repair_through_real_downloads_without_losing_user_data() -> TestResult
{
    let _serial = crate::commands::tests::command_smoke_lock().lock().await;
    let native = native_provider().await?;
    for module in ["rimworld", "minecraft"] {
        for (damage, pinned, reject) in [
            (Damage::Healthy, false, false),
            (Damage::Missing, false, false),
            (Damage::Changed, false, false),
            (Damage::Healthy, true, false),
            (Damage::Missing, true, false),
            (Damage::Missing, false, true),
        ] {
            let source = LocalSource::new(&native).await?;
            source.fail_download.store(reject, Ordering::Release);
            let mut fixture = Fixture::for_module(module, false, &native, Some(&source)).await?;
            source
                .with_java_package(exercise(&mut fixture, damage, pinned, reject))
                .await?;
            if pinned {
                assert!(source.requests.lock().unwrap().is_empty());
            } else if damage == Damage::Healthy {
                assert_eq!(
                    source.downloads(),
                    0,
                    "current programs require no payload download"
                );
                assert!(
                    !source
                        .requests
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|path| path == "/java.zip"),
                    "an already-current program must keep its JRE without downloading",
                );
            } else {
                assert!(
                    source.downloads() > 0,
                    "repair must reach the real payload endpoint"
                );
            }
            assert!(
                !source.task.is_finished(),
                "local source must not silently fail"
            );
        }
    }
    Ok(())
}

async fn check_legacy_minecraft_inventory_repair(damage_kind: Damage) -> TestResult {
    let _serial = crate::commands::tests::command_smoke_lock().lock().await;
    let native = native_provider().await?;
    let source = LocalSource::new(&native).await?;
    let mut fixture = Fixture::for_module("minecraft", false, &native, Some(&source)).await?;
    let mut library_before = Vec::new();
    for relative in [
        "server.jar",
        "jre/bin/java.exe",
        ".langame/minecraft-server.json",
        ".langame-clean-package.json",
        ".langame-initial-package.json",
    ] {
        let path = fixture.library.join(relative);
        library_before.push((path.clone(), fs::read(path)?));
    }
    for name in [
        ".langame-clean-package.json",
        ".langame-initial-package.json",
    ] {
        let path = fixture.program.join(name);
        if path.exists() {
            fs::remove_file(&path)?;
        }
        assert!(!path.exists());
    }
    let plugin = fixture.program.join("plugins/operator-only.jar");
    write_file(
        &plugin,
        b"operator plugin is not an official server payload",
    )?;
    assert_eq!(
        app_storage::program_launch_files_match_baseline(
            &fixture.program,
            &fixture.descriptor,
            None,
        )?,
        None,
    );
    if damage_kind == Damage::Changed {
        // Legacy readiness has no inventory and only sees that the jar exists.
        // The provider's real SHA1 comparison must find this same-size damage.
        fs::write(fixture.program.join("server.jar"), b"xyz")?;
        assert_eq!(
            fs::metadata(fixture.program.join("server.jar"))?.len(),
            PAYLOAD.len() as u64,
        );
        assert_eq!(
            fixture.module().summary.install_state,
            InstallState::Installed
        );
    }
    source
        .with_java_package(exercise(&mut fixture, damage_kind, false, false))
        .await?;
    let requests = source.requests.lock().unwrap().clone();
    assert!(requests.iter().any(|path| path == "/manifest.json"));
    assert!(requests.iter().any(|path| path == "/version.json"));
    assert_eq!(
        requests
            .iter()
            .filter(|path| path.as_str() == "/server.jar")
            .count(),
        1,
        "a real verified payload download must repair the legacy jar: {requests:?}",
    );
    assert_eq!(
        requests
            .iter()
            .filter(|path| path.as_str() == "/java.zip")
            .count(),
        1,
        "legacy repair must establish the JRE inventory from a verified archive",
    );
    assert!(!source.task.is_finished());
    let expected_jar_hash = Sha256::digest(PAYLOAD)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let expected_java_hash = Sha256::digest(&native)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let expected_runtime_hash = Sha256::digest(JAVA_RUNTIME_LIBRARY)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    for name in [
        ".langame-clean-package.json",
        ".langame-initial-package.json",
    ] {
        let inventory: Value = serde_json::from_slice(&fs::read(fixture.program.join(name))?)?;
        let files = inventory["files"]
            .as_object()
            .expect("official Minecraft inventory");
        assert_eq!(files["server.jar"], expected_jar_hash);
        assert_eq!(files["jre/bin/java.exe"], expected_java_hash);
        assert_eq!(files["jre/lib/modules"], expected_runtime_hash);
        assert!(!files.keys().any(|path| path.starts_with("plugins/")));
        assert!(!files.contains_key("server.properties"));
        assert!(!files.keys().any(|path| path.ends_with("world.sentinel")));
    }
    assert_eq!(
        fs::read(plugin)?,
        b"operator plugin is not an official server payload",
    );
    for (path, bytes) in library_before {
        assert_eq!(
            fs::read(&path)?,
            bytes,
            "library changed: {}",
            path.display()
        );
    }
    Ok(())
}

#[tokio::test]
async fn minecraft_legacy_missing_inventory_repairs_missing_jar_without_certifying_user_plugins()
-> TestResult {
    check_legacy_minecraft_inventory_repair(Damage::Missing).await
}

#[tokio::test]
async fn minecraft_legacy_missing_inventory_repairs_same_size_jar_damage_through_provider_sha1()
-> TestResult {
    check_legacy_minecraft_inventory_repair(Damage::Changed).await
}

#[tokio::test]
async fn manual_source_cannot_claim_a_repair_or_launch_a_missing_program() -> TestResult {
    let _serial = crate::commands::tests::command_smoke_lock().lock().await;
    let native = native_provider().await?;
    let mut fixture = Fixture::for_module("rust", false, &native, None).await?;
    // There is no manual-only shipped module. Keep Rust's real ownership and
    // program layout, but explicitly remove its managed source at this boundary.
    fixture.descriptor.summary.steam_app_id = None;
    assert!(!module_supports_automatic_prestart_update(
        &fixture.module()
    ));
    exercise(&mut fixture, Damage::Missing, false, false).await
}

async fn check_legacy_shared_abioticfactor_repair(missing_non_entry_file: bool) -> TestResult {
    const DLL: &str = "AbioticFactor/Binaries/Win64/steam_api64.dll";
    let _serial = crate::commands::tests::command_smoke_lock().lock().await;
    let native = native_provider().await?;
    let originals: &[(&str, &[u8])] = if missing_non_entry_file {
        &[(DLL, PAYLOAD)]
    } else {
        &[]
    };
    let fixture = Fixture::for_module_with_original_files(
        "abioticfactor",
        true,
        &native,
        None,
        false,
        originals,
    )
    .await?;
    assert!(save_startup::requires_private_program("abioticfactor"));
    if missing_non_entry_file {
        fs::remove_file(fixture.program.join(DLL))?;
        assert_eq!(
            fixture.module().summary.install_state,
            InstallState::Installed
        );
        assert_eq!(
            app_storage::program_launch_files_match_baseline(
                &fixture.program,
                &fixture.descriptor,
                None,
            )?,
            Some(true),
        );
        assert!(!app_storage::library_program_is_pristine(
            &fixture.program,
            &fixture.descriptor,
            None,
        )?);
    } else {
        damage(&fixture, Damage::Missing)?;
    }
    let original_config = operator_path(&fixture);
    let original_save = Path::new(&fixture.instance.saves_path).join("world.sentinel");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    app_steamcmd::ensure_steamcmd_installed(&state.install_locks, &fixture.storage.settings)
        .await?;
    let reservation =
        match state.try_reserve_runtime_start(&fixture.instance.summary.id, "manual")? {
            RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
            other => panic!("unexpected reservation: {other:?}"),
        };
    let instance_guard = state
        .acquire_instance_mutation(&fixture.instance.summary.id)
        .await;
    let roots = save_startup::program_roots(&fixture.instance, &fixture.program)?;
    assert_eq!(roots.len(), 2);
    let guard =
        app_steamcmd::acquire_game_install_lifecycle(&state.install_locks, "abioticfactor", &roots)
            .await?;
    let (guards, repaired) = tokio::time::timeout(
        BUDGET,
        save_startup::repair_shared_program_before_detach(
            &state,
            &fixture.storage,
            &fixture.descriptor,
            &fixture.instance,
            &reservation,
            (instance_guard, Some(guard)),
            PrestartUpdateLogContext {
                source: "manual",
                console_log_path: None,
            },
        ),
    )
    .await??;
    assert!(repaired.is_some_and(|warnings| warnings.is_empty()));
    let (guards, detached) = save_startup::ensure_private_program(
        &fixture.storage,
        &fixture.descriptor,
        fixture.instance.clone(),
        &reservation,
        guards,
    )
    .await?;
    let private = PathBuf::from(
        crate::commands::commands_runtime_lifecycle::private_runtime_install_root(&detached)?,
    );
    assert_ne!(
        fs::canonicalize(&private)?,
        fs::canonicalize(&fixture.library)?
    );
    for root in [&fixture.library, &private] {
        guards
            .1
            .as_ref()
            .unwrap()
            .ensure_scope("abioticfactor", root)?;
        assert_eq!(
            fs::read(root.join(entrypoint(&fixture.descriptor)))?,
            PAYLOAD
        );
        if missing_non_entry_file {
            assert_eq!(fs::read(root.join(DLL))?, PAYLOAD);
        }
        assert!(app_storage::library_program_is_pristine(
            root,
            &fixture.descriptor,
            None,
        )?);
    }
    let after = map_module_details_with_install_state(
        &fixture.storage.settings,
        &fixture.descriptor,
        Some(&private.to_string_lossy()),
    );
    ensure_module_installed_for_start(&after)?;
    assert_eq!(
        fs::read_to_string(fixture.storage.paths.steamcmd_root.join("native-commands"))?,
        "app_update 2857200 validate\n"
    );
    assert_eq!(fs::read(original_config)?, OPERATOR);
    assert_eq!(fs::read(original_save)?, WORLD);
    assert_eq!(
        fs::read(private.join(format!(
            "AbioticFactor/Saved/Config/WindowsServer/LanGame/{}-SandboxSettings.ini",
            detached.summary.id
        )))?,
        OPERATOR
    );
    let settings: Value = serde_json::from_str(&detached.settings_json)?;
    let world = settings["world_save_name"].as_str().unwrap();
    assert_eq!(
        fs::canonicalize(&detached.saves_path)?,
        fs::canonicalize(private.join(format!(
            "AbioticFactor/Saved/SaveGames/Server/Worlds/{world}"
        )))?
    );
    assert_eq!(
        fs::read(Path::new(&detached.saves_path).join("world.sentinel"))?,
        WORLD
    );
    assert!(
        read_active_instance_run(&fixture.storage.paths, &detached.summary.id)
            .await?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn legacy_shared_abioticfactor_repairs_before_detaching_its_private_program() -> TestResult {
    check_legacy_shared_abioticfactor_repair(false).await
}

#[tokio::test]
async fn legacy_shared_abioticfactor_repairs_missing_non_entry_dll_before_detaching() -> TestResult
{
    check_legacy_shared_abioticfactor_repair(true).await
}

async fn check_rust_exclusive_library_patch(missing_inventory: bool) -> TestResult {
    const DLL: &str = "RustDedicated_Data/Managed/Assembly-CSharp.dll";
    const ORIGINAL_DLL: &[u8] = b"official Rust managed assembly";
    const PATCHED_DLL: &[u8] = b"external Oxide patched assembly";

    let _serial = crate::commands::tests::command_smoke_lock().lock().await;
    let native = native_provider().await?;
    let fixture = Fixture::for_module_with_original_files(
        "rust",
        false,
        &native,
        None,
        true,
        &[(DLL, ORIGINAL_DLL)],
    )
    .await?;
    let instance_root =
        crate::commands::commands_program_storage::instance_root(&fixture.instance)?;
    assert_eq!(
        app_storage::instance_program_mode(instance_root)?,
        app_core::InstanceProgramMode::Independent
    );
    assert!(app_storage::instance_uses_exclusive_program(instance_root)?);
    let binding = app_storage::read_instance_program_install(
        &fixture.storage.paths,
        &fixture.instance.summary.id,
    )
    .await?
    .unwrap();
    assert_eq!(
        binding.install.scope,
        app_storage::ProgramInstallScope::Library
    );
    assert!(binding.install.owner_instance_id.is_none());
    assert_eq!(
        fs::canonicalize(&fixture.program)?,
        fs::canonicalize(&fixture.library)?
    );

    let mut inventories = Vec::new();
    let original_hash = Sha256::digest(ORIGINAL_DLL)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    for name in [
        ".langame-clean-package.json",
        ".langame-initial-package.json",
    ] {
        let path = fixture.program.join(name);
        let bytes = fs::read(&path)?;
        let inventory: Value = serde_json::from_slice(&bytes)?;
        assert_eq!(inventory["files"][DLL], original_hash);
        if missing_inventory {
            fs::remove_file(&path)?;
        }
        inventories.push((path, bytes));
    }
    let expected_launch_health = (!missing_inventory).then_some(true);
    // External patching changes original program bytes. No mod-directory marker
    // tells the health checker to skip anything: the real launch entry stays clean.
    fs::write(fixture.program.join(DLL), PATCHED_DLL)?;
    assert!(!fixture.program.join("oxide").exists());
    assert_eq!(
        app_storage::program_launch_files_match_baseline(
            &fixture.program,
            &fixture.descriptor,
            None
        )?,
        expected_launch_health
    );
    assert!(!app_storage::library_program_is_pristine(
        &fixture.program,
        &fixture.descriptor,
        None
    )?);

    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    app_steamcmd::ensure_steamcmd_installed(&state.install_locks, &fixture.storage.settings)
        .await?;
    let id = &fixture.instance.summary.id;
    for attempt in 1..=2 {
        let module = fixture.module();
        let reservation = match state.try_reserve_runtime_start(id, "manual")? {
            RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
            other => panic!("unexpected reservation: {other:?}"),
        };
        let _instance = state.acquire_instance_mutation(id).await;
        let guard = app_steamcmd::acquire_game_install_lifecycle(
            &state.install_locks,
            "rust",
            std::slice::from_ref(&fixture.program),
        )
        .await?;
        let outcome = tokio::time::timeout(
            BUDGET,
            run_prestart_update_if_needed(
                &state,
                &fixture.storage,
                PrestartUpdateRequest {
                    descriptor: &fixture.descriptor,
                    module: &module,
                    instance: &fixture.instance,
                    install_guard: guard,
                    reservation: &reservation,
                    context: PrestartUpdateLogContext {
                        source: "manual",
                        console_log_path: None,
                    },
                },
            ),
        )
        .await??;
        outcome
            .install_guard
            .ensure_scope("rust", &fixture.program)?;
        assert!(outcome.warnings.is_empty());
        assert_eq!(
            fs::read_to_string(fixture.storage.paths.steamcmd_root.join("native-commands"))?,
            "app_update 258550\n".repeat(attempt)
        );
        assert_eq!(fs::read(fixture.program.join(DLL))?, PATCHED_DLL);
        for (path, bytes) in &inventories {
            if missing_inventory {
                assert!(
                    !path.exists(),
                    "an ordinary update must not certify the legacy patched tree: {}",
                    path.display()
                );
            } else {
                assert_eq!(
                    &fs::read(path)?,
                    bytes,
                    "attempt {attempt} lost original expectations: {}",
                    path.display()
                );
            }
        }
        assert_eq!(
            app_storage::program_launch_files_match_baseline(
                &fixture.program,
                &fixture.descriptor,
                None
            )?,
            expected_launch_health
        );
        assert!(
            !app_storage::library_program_is_pristine(&fixture.program, &fixture.descriptor, None)?,
            "the modified DLL must never be certified as official"
        );
        ensure_module_installed_for_start(&fixture.module())?;
        let jobs = state.app_state.read().unwrap().jobs.clone();
        assert_eq!(jobs.len(), attempt);
        assert!(
            jobs.iter()
                .all(|job| matches!(job.status, JobStatus::Completed) && job.warnings.is_empty())
        );
        assert_eq!(fs::read(operator_path(&fixture))?, OPERATOR);
        assert_eq!(
            fs::read(Path::new(&fixture.instance.saves_path).join("world.sentinel"))?,
            WORLD
        );
        assert!(
            read_active_instance_run(&fixture.storage.paths, id)
                .await?
                .is_none()
        );
        assert!(!state.runtime_supervisor.lock().unwrap().is_tracked(id));
    }
    Ok(())
}

#[tokio::test]
async fn rust_exclusive_library_reuse_keeps_external_dll_patch_across_repeated_automatic_updates()
-> TestResult {
    check_rust_exclusive_library_patch(false).await
}

#[tokio::test]
async fn rust_exclusive_library_without_inventory_keeps_ordinary_updates_and_external_dll_patch()
-> TestResult {
    check_rust_exclusive_library_patch(true).await
}
