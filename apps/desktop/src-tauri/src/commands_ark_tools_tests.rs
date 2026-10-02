use super::*;
use crate::commands::commands_ark_tools::{self as ark, protocol};

struct ArkFixture {
    root: PathBuf,
    _environment: ProgramDataEnvGuard,
    storage: StorageBootstrap,
    id: String,
    runtime: PathBuf,
    cleaned: bool,
}

impl ArkFixture {
    async fn new(module: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Self::with_existing_program(module, false).await
    }

    async fn with_existing_program(
        module: &str,
        exclusive: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let root = temp_test_dir("ark-command-boundary");
        let environment = ProgramDataEnvGuard::set(&root.join("programdata"));
        save_app_settings(AppSettings {
            archives_root: String::new(),
            servers_root: root.join("instances").to_string_lossy().into_owned(),
            games_root: root.join("games").to_string_lossy().into_owned(),
            modules_root: workspace_root()
                .join("modules")
                .to_string_lossy()
                .into_owned(),
            steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
        })?;
        let storage = bootstrap_storage()?;
        initialize_database(&storage.paths).await?;
        let descriptors = discover_modules(&storage.paths.modules_root)?;
        sync_modules(&storage.paths, &descriptors).await?;
        let descriptor = find_descriptor(&descriptors, module)?;
        prepare_fake_registered_program(&storage.paths, descriptor).await?;
        let created = app_storage::create_instance_with_options(
            &storage.paths,
            descriptor,
            CreateInstanceInput {
                name: "ARK command admission fixture".into(),
                module_id: module.into(),
            },
            app_storage::InstanceCreationOptions {
                prefer_existing_install: exclusive,
                program_mode: Some(app_core::InstanceProgramMode::Independent),
                require_clean_program: true,
                ..Default::default()
            },
        )
        .await?;
        let runtime = created.effective_install_root;
        let id = created.provisioning.summary.id;
        let binding = app_storage::read_instance_program_install(&storage.paths, &id)
            .await?
            .ok_or("ARK fixture program binding is missing")?;
        let expected_scope = if exclusive {
            app_storage::ProgramInstallScope::Library
        } else {
            app_storage::ProgramInstallScope::Instance
        };
        if binding.install.scope != expected_scope
            || binding.install.owner_instance_id.as_deref()
                != if exclusive { None } else { Some(id.as_str()) }
            || fs::canonicalize(&binding.install.install_root)? != fs::canonicalize(&runtime)?
            || app_storage::instance_uses_exclusive_program(
                &storage.paths.instances_root.join(&id),
            )? != exclusive
        {
            return Err(
                "ARK fixture did not create the requested real program ownership layout".into(),
            );
        }
        // Even a regression in an earlier admission guard cannot progress to a
        // network download: the installer must preserve this unowned loader.
        fs::write(
            runtime.join("ShooterGame/Binaries/Win64/version.dll"),
            b"inert operator loader fixture",
        )?;
        Ok(Self {
            root,
            _environment: environment,
            storage,
            id,
            runtime,
            cleaned: false,
        })
    }
    fn untouched(&self) {
        assert_eq!(
            fs::read(self.runtime.join("ShooterGame/Binaries/Win64/version.dll")).unwrap(),
            b"inert operator loader fixture"
        );
        assert!(
            !self
                .runtime
                .join("ShooterGame/Binaries/Win64/ArkApi")
                .exists()
        );
    }

    fn finish(mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.root.parent() != Some(env::temp_dir().as_path()) {
            return Err(
                "ARK fixture cleanup root is outside this test's temporary directory".into(),
            );
        }
        fs::remove_dir_all(&self.root)?;
        self.cleaned = true;
        Ok(())
    }
}

impl Drop for ArkFixture {
    fn drop(&mut self) {
        if !self.cleaned {
            eprintln!(
                "ARK command fixture retained after an incomplete test at {}",
                self.root.display()
            );
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn ark_tools_prepare_waits_for_instance_lock_and_rejects_pending_start()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = command_smoke_lock().lock().await;
    let fixture = ArkFixture::new("arksurvivalevolved").await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let held = app
        .state::<DesktopState>()
        .acquire_instance_mutation(&fixture.id)
        .await;
    let reservation = match app
        .state::<DesktopState>()
        .try_reserve_runtime_start(&fixture.id, "test")?
    {
        crate::state::RuntimeStartReservationAttempt::Reserved(lease) => lease,
        other => panic!("expected isolated start reservation: {other:?}"),
    };
    let handle = app.handle().clone();
    let id = fixture.id.clone();
    let mut operation = tokio::spawn(async move {
        ark::prepare_ark_tools(
            handle.state::<DesktopState>(),
            ark::PrepareInput {
                instance_id: id,
                allow_matching_symbols_download: true,
            },
        )
        .await
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(150), &mut operation)
            .await
            .is_err(),
        "preparation must not pass a held instance mutation lock"
    );
    fixture.untouched();
    drop(held);
    let result = tokio::time::timeout(Duration::from_secs(15), operation).await??;
    assert!(
        result
            .err()
            .ok_or("pending start was accepted")?
            .contains("停止")
    );
    fixture.untouched();
    drop(reservation);
    drop(app);
    fixture.finish()?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn ark_tools_prepare_requires_asa_symbol_download_consent_and_spawn_requires_running_owner()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = command_smoke_lock().lock().await;
    let fixture = ArkFixture::new("arksurvivalascended").await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let result = ark::prepare_ark_tools(
        app.state::<DesktopState>(),
        ark::PrepareInput {
            instance_id: fixture.id.clone(),
            allow_matching_symbols_download: false,
        },
    )
    .await;
    assert!(
        result
            .err()
            .ok_or("ASA preparation accepted missing consent")?
            .contains("permission")
    );
    let mut input = spawn_input();
    input.instance_id = fixture.id.clone();
    let result = ark::spawn_ark_creature(app.state::<DesktopState>(), input).await;
    assert!(
        result
            .err()
            .ok_or("stopped instance accepted a spawn")?
            .contains("not running")
    );
    fixture.untouched();
    drop(app);
    fixture.finish()?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn ark_tools_prepare_rejects_program_binding_to_another_runtime()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = command_smoke_lock().lock().await;
    let fixture = ArkFixture::new("arksurvivalevolved").await?;
    let other = fixture.root.join("another-runtime");
    fs::create_dir(&other)?;
    let binding = app_storage::read_instance_program_install(&fixture.storage.paths, &fixture.id)
        .await?
        .ok_or("ARK fixture instance binding disappeared")?;
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(&fixture.storage.paths.database_path),
    )
    .await?;
    let changed = sqlx::query("UPDATE game_installs SET install_root=?1 WHERE id=?2")
        .bind(other.to_string_lossy().as_ref())
        .bind(binding.install.id)
        .execute(&pool)
        .await;
    pool.close().await;
    assert_eq!(changed?.rows_affected(), 1);
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let result = ark::prepare_ark_tools(
        app.state::<DesktopState>(),
        ark::PrepareInput {
            instance_id: fixture.id.clone(),
            allow_matching_symbols_download: true,
        },
    )
    .await;
    let error = result.err().ok_or("wrong program binding was accepted")?;
    assert!(
        error.contains("program path does not match its registered installation"),
        "unexpected rejection: {error}"
    );
    fixture.untouched();
    assert_eq!(fs::read_dir(other)?.count(), 0);
    drop(app);
    fixture.finish()?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn ark_tools_prepare_accepts_first_exclusive_program_ownership_before_loader_preflight()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = command_smoke_lock().lock().await;
    let fixture = ArkFixture::with_existing_program("arksurvivalevolved", true).await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let result = ark::prepare_ark_tools(
        app.state::<DesktopState>(),
        ark::PrepareInput {
            instance_id: fixture.id.clone(),
            allow_matching_symbols_download: true,
        },
    )
    .await;
    let error = result.err().ok_or("unowned loader was overwritten")?;
    assert!(
        error.contains("unknown or modified") && error.contains("version.dll"),
        "the valid exclusive owner must reach the installer conflict guard: {error}"
    );
    fixture.untouched();
    drop(app);
    fixture.finish()?;
    Ok(())
}

#[test]
fn ark_tools_rcon_ownership_checks_the_dispatch_address_not_only_port() {
    use std::net::IpAddr;
    let parse = |value: &str| value.parse::<IpAddr>().unwrap();
    let interfaces = [parse("192.0.2.10"), parse("2001:db8::10")];
    assert!(ark::rcon_endpoint_covers(
        "127.0.0.1",
        parse("127.0.0.1"),
        &interfaces
    ));
    assert!(!ark::rcon_endpoint_covers(
        "127.0.0.2",
        parse("127.0.0.1"),
        &interfaces
    ));
    assert!(ark::rcon_endpoint_covers(
        "0.0.0.0",
        parse("127.0.0.1"),
        &interfaces
    ));
    assert!(ark::rcon_endpoint_covers(
        "0.0.0.0",
        parse("192.0.2.10"),
        &interfaces
    ));
    assert!(!ark::rcon_endpoint_covers(
        "0.0.0.0",
        parse("192.0.2.11"),
        &interfaces
    ));
    assert!(!ark::rcon_endpoint_covers(
        "::",
        parse("127.0.0.1"),
        &interfaces
    ));
    assert!(ark::rcon_endpoint_covers(
        "::",
        parse("2001:db8::10"),
        &interfaces
    ));
    assert!(!ark::rcon_endpoint_covers(
        "::",
        parse("2001:db8::11"),
        &interfaces
    ));
    assert!(ark::rcon_endpoint_covers(
        "::ffff:127.0.0.1",
        parse("127.0.0.1"),
        &interfaces
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn ark_tools_interrupted_install_prevents_normal_server_start()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = command_smoke_lock().lock().await;
    let fixture = ArkFixture::new("arksurvivalevolved").await?;
    let pending = fixture
        .runtime
        .join("ShooterGame/Binaries/Win64/.langame-ark-tools-pending");
    fs::create_dir(&pending)?;
    fs::write(
        pending.join("retained-evidence"),
        b"interrupted preparation",
    )?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let result = start_instance_process_after_reconcile(
        None,
        &app.state::<DesktopState>(),
        &fixture.storage,
        fixture.id.clone(),
        "manual",
        None,
    )
    .await;
    let error = result
        .err()
        .ok_or("interrupted extension allowed a server start")?;
    assert!(
        error.contains("ARK") && error.contains("interrupted"),
        "{error}"
    );
    assert!(
        read_active_instance_run(&fixture.storage.paths, &fixture.id)
            .await?
            .is_none()
    );
    assert!(
        !app.state::<DesktopState>()
            .runtime_supervisor
            .lock()
            .unwrap()
            .is_tracked(&fixture.id)
    );
    assert_eq!(
        fs::read(pending.join("retained-evidence"))?,
        b"interrupted preparation"
    );
    fixture.untouched();
    drop(app);
    fixture.finish()?;
    Ok(())
}

fn spawn_input() -> protocol::SpawnInput {
    protocol::SpawnInput {
        instance_id: "fixture".into(),
        request_id: "0123456789abcdef0123456789abcdef".into(),
        creature: "Rex_Character_BP_C".into(),
        level: 150,
        x: 100.0,
        y: 200.0,
        z: 300.0,
        tamed: false,
        player_id: 0,
    }
}

fn observed() -> protocol::Creature {
    protocol::Creature {
        id1: 123,
        id2: 456,
        class_name: "Rex_Character_BP_C".into(),
        level: 150,
        team: 0,
        x: 100.0,
        y: 200.0,
        z: 300.0,
        tamed: false,
    }
}

#[test]
fn ark_tools_readback_cannot_validate_a_different_requested_wild_level_or_location() {
    let input = spawn_input();
    let mut wrong = observed();
    wrong.level = 1;
    assert!(
        protocol::verify_spawn(&input, &wrong, &wrong).is_err(),
        "two matching receipts do not prove the requested level"
    );
    wrong = observed();
    wrong.x = 9_000_000.0;
    assert!(
        protocol::verify_spawn(&input, &wrong, &wrong).is_err(),
        "two matching receipts do not prove the requested location"
    );
    assert!(protocol::verify_spawn(&input, &observed(), &observed()).is_ok());
}

#[test]
fn ark_tools_failed_or_stale_entity_envelopes_cannot_be_success() {
    let input = spawn_input();
    let reply = |id: &str, ok: bool| {
        format!(
            "LGSM_ARK_TOOLS {}",
            json!({"version":1,"edition":"ase","action":"spawn","requestId":id,"ok":ok,"error":"native failure","id1":123,"id2":456,"className":"Rex_Character_BP_C","level":150,"team":0,"x":100.0,"y":200.0,"z":300.0,"tamed":false})
        )
    };
    assert!(
        protocol::creature(
            &reply(&input.request_id, false),
            "arksurvivalevolved",
            "spawn",
            &input.request_id
        )
        .is_err()
    );
    assert!(
        protocol::creature(
            &reply("0123456789abcdef0123456789abcdee", true),
            "arksurvivalevolved",
            "spawn",
            &input.request_id
        )
        .is_err()
    );
    assert!(
        protocol::creature(
            &reply(&input.request_id, true),
            "arksurvivalascended",
            "spawn",
            &input.request_id
        )
        .is_err()
    );
    assert!(
        protocol::creature(
            "Server received, But no response!!",
            "arksurvivalevolved",
            "spawn",
            &input.request_id
        )
        .is_err()
    );
}
