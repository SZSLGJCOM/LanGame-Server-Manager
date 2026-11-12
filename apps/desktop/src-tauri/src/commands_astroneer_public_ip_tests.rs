use super::*;
use crate::astroneer_public_ip::prepare_with_sources;
use app_platform_win::BindAddressCandidate;
use std::net::Ipv4Addr;

async fn no_adapter_lookup() -> Result<Vec<BindAddressCandidate>, String> {
    panic!("this connection mode must not query local adapters")
}

fn reserve(state: &DesktopState, id: &str) -> RuntimeStartReservationLease {
    match state.try_reserve_runtime_start(id, "manual").unwrap() {
        RuntimeStartReservationAttempt::Reserved(lease) => lease,
        other => panic!("unexpected reservation: {other:?}"),
    }
}

fn update(details: &InstanceDetails, settings: Value) -> UpdateInstanceInput {
    UpdateInstanceInput {
        id: details.summary.id.clone(),
        bind_ip: details.summary.bind_ip.clone(),
        auto_backup_on_stop: details.auto_backup_on_stop,
        backup_retention_count: details.backup_retention_count,
        settings_json: settings.to_string(),
        ports: details.ports.clone(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn astroneer_invalid_address_and_empty_manual_mode_block_production_start_before_updates()
-> Result<(), Box<dyn std::error::Error>> {
    let _lock = command_smoke_lock().lock().await;
    let root = temp_test_dir("astro-preflight");
    let environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    let result = async {
        isolated_smoke_app_settings(&root)?;
        let storage = bootstrap_storage()?;
        let descriptor = discover_modules(&storage.paths.modules_root)?
            .into_iter()
            .find(|value| value.summary.id == "astroneer")
            .ok_or("Astro fixture descriptor missing")?;
        sync_modules(&storage.paths, std::slice::from_ref(&descriptor)).await?;
        prepare_fake_registered_program(&storage.paths, &descriptor).await?;
        let app = tauri::test::mock_builder()
            .manage(DesktopState::from_storage(&storage))
            .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
        sync_modules_to_storage(app.state::<DesktopState>()).await?;
        let created = create_fake_module_instance(
            app.state::<DesktopState>(),
            "astroneer",
            "Network preflight fixture",
        )
        .await?;
        let created = read_instance_details(&storage.paths, &created.summary.id).await?;
        // There is no installer or game executable to run. Moving the network
        // gate behind the update path produces an installer error instead.
        assert!(!storage.paths.steamcmd_root.join("steamcmd.exe").exists());
        let state = app.state::<DesktopState>();
        // Instance creation retains its completed job. Startup must neither
        // add an update job nor change any of those existing receipts.
        let jobs_before = serde_json::to_value(&state.app_state.read().unwrap().jobs)?;
        for (mode, address, reason) in [
            ("public", "0.0.0.0", "invalid_public_ip"),
            ("manual", "", "manual_address_required"),
        ] {
            let mut settings: Value = serde_json::from_str(&created.settings_json)?;
            settings["public_ip"] = Value::String(address.into());
            settings["connection_mode"] = Value::String(mode.into());
            let mut input = update(&created, settings);
            // This module has no strict interface binding; use its normal
            // wildcard bind so production reaches the address prerequisite.
            input.bind_ip = "0.0.0.0".into();
            let before = update_instance(&storage.paths, input).await?;
            let error = start_instance_process_after_reconcile(
                None,
                &state,
                &storage,
                before.summary.id.clone(),
                "manual",
                None,
            )
            .await
            .unwrap_err();
            let error: Value = serde_json::from_str(&error).unwrap_or_else(|parse| {
                panic!("expected network prerequisite error: {error}; {parse}")
            });
            assert_eq!(error["code"], "astroneer_network_preflight_failed");
            assert_eq!(error["reason"], reason);
            assert_eq!(
                serde_json::to_value(&state.app_state.read().unwrap().jobs)?,
                jobs_before
            );
            assert!(
                read_active_instance_run(&storage.paths, &before.summary.id)
                    .await?
                    .is_none()
            );
            assert_eq!(
                read_instance_details(&storage.paths, &before.summary.id)
                    .await?
                    .settings_json,
                before.settings_json
            );
        }
        drop(app);
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .await;
    drop(environment);
    fs::remove_dir_all(root)?;
    result
}

#[tokio::test(flavor = "current_thread")]
async fn astroneer_public_ip_preparation_persists_renders_reuses_and_rejects_stale_or_cancelled_work()
-> Result<(), Box<dyn std::error::Error>> {
    let _lock = command_smoke_lock().lock().await;
    let root = temp_test_dir("astroneer-public-ip");
    let environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    let result = async {
        isolated_smoke_app_settings(&root)?;
        let storage = bootstrap_storage()?;
        let descriptor = discover_modules(&storage.paths.modules_root)?
            .into_iter()
            .find(|value| value.summary.id == "astroneer")
            .ok_or("Astro fixture descriptor missing")?;
        sync_modules(&storage.paths, std::slice::from_ref(&descriptor)).await?;
        prepare_fake_registered_program(&storage.paths, &descriptor).await?;
        let app = tauri::test::mock_builder()
            .manage(DesktopState::from_storage(&storage))
            .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
        sync_modules_to_storage(app.state::<DesktopState>()).await?;
        let instance = create_fake_module_instance(
            app.state::<DesktopState>(),
            "astroneer",
            "Auto address fixture",
        )
        .await?;
        // Exercise an actual pre-mode document. Discovery must persist its
        // known Public choice before storage sees the newly nonempty address.
        let mut legacy: Value =
            serde_json::from_str(&fs::read_to_string(&instance.config_file_path)?)?;
        legacy["settings"]
            .as_object_mut()
            .unwrap()
            .remove("connection_mode");
        fs::write(
            &instance.config_file_path,
            serde_json::to_vec_pretty(&legacy)?,
        )?;
        let before = read_instance_details(&storage.paths, &instance.summary.id).await?;
        let state = app.state::<DesktopState>();
        let reservation = reserve(&state, &before.summary.id);
        let address = Ipv4Addr::new(1, 1, 1, 1);
        let mut expected: Value = serde_json::from_str(&before.settings_json)?;
        assert_eq!(expected["public_ip"], "");

        for value in [
            Value::Bool(false),
            Value::String("127.0.0.1".into()),
            Value::String("224.0.0.1".into()),
        ] {
            let mut invalid = before.clone();
            let mut values = expected.clone();
            values["public_ip"] = value;
            invalid.settings_json = values.to_string();
            assert!(
                prepare_with_sources(
                    &storage,
                    &reservation,
                    invalid,
                    async { panic!("invalid explicit address must not poll network") },
                    no_adapter_lookup(),
                )
                .await
                .is_err()
            );
        }
        let mut active = before.clone();
        active.summary.status = app_core::InstanceStatus::Running;
        assert!(
            prepare_with_sources(
                &storage,
                &reservation,
                active,
                async { panic!("active instance must not poll network") },
                no_adapter_lookup(),
            )
            .await
            .is_err()
        );
        let mut other = before.clone();
        other.summary.module_id = "another-game".into();
        assert!(
            prepare_with_sources(
                &storage,
                &reservation,
                other,
                async { panic!("other game must not poll network") },
                no_adapter_lookup(),
            )
            .await
            .is_ok()
        );

        // A failed/oversized/malformed/timeout network result never reaches a write.
        for reason in ["fixture timeout", "fixture invalid body"] {
            assert!(
                prepare_with_sources(
                    &storage,
                    &reservation,
                    before.clone(),
                    async { Err(reason.into()) },
                    async { Ok(Vec::new()) },
                )
                .await
                .is_err()
            );
            assert_eq!(
                read_instance_details(&storage.paths, &before.summary.id)
                    .await?
                    .settings_json,
                before.settings_json
            );
        }
        // Explicit private endpoints are allowed, but an Internet lookup
        // returning a private address must never be persisted.
        assert!(
            prepare_with_sources(
                &storage,
                &reservation,
                before.clone(),
                async { Ok(Ipv4Addr::new(192, 168, 1, 2)) },
                async { Ok(Vec::new()) }
            )
            .await
            .is_err()
        );
        assert_eq!(
            read_instance_details(&storage.paths, &before.summary.id)
                .await?
                .settings_json,
            before.settings_json
        );
        let prepared = prepare_with_sources(
            &storage,
            &reservation,
            before.clone(),
            async { Ok(address) },
            async { Ok(Vec::new()) },
        )
        .await?;
        expected["public_ip"] = Value::String(address.to_string());
        expected["connection_mode"] = Value::String("public".into());
        assert_eq!(
            serde_json::from_str::<Value>(&prepared.settings_json)?,
            expected
        );
        let persisted = read_instance_details(&storage.paths, &before.summary.id).await?;
        assert_eq!(persisted.settings_json, prepared.settings_json);
        let rendered = app_storage::materialize_instance_configuration_for_start(
            &storage.paths,
            &before.summary.id,
        )
        .await?;
        assert_eq!(rendered.settings_json, prepared.settings_json);
        let install = app_storage::resolve_instance_runtime_root(
            crate::commands::commands_program_storage::instance_root(&rendered)?,
        )?;
        // The actual module template and ordinary materializer are used, with an
        // inert program file that is never executed.
        let ini = fs::read_to_string(
            install.join("Astro/Saved/Config/WindowsServer/AstroServerSettings.ini"),
        )?;
        assert!(ini.lines().any(|line| line.trim() == "PublicIP=1.1.1.1"));
        let reused = prepare_with_sources(
            &storage,
            &reservation,
            persisted.clone(),
            async { panic!("explicit address must not poll network") },
            no_adapter_lookup(),
        )
        .await?;
        assert_eq!(reused.settings_json, persisted.settings_json);
        drop(reservation);

        // A setting changed while the external boundary was pending must survive.
        let mut blank = expected.clone();
        blank["public_ip"] = Value::String(String::new());
        let blank = update_instance(&storage.paths, update(&prepared, blank)).await?;
        let reservation = reserve(&state, &before.summary.id);
        let discovery = async {
            let mut changed: Value = serde_json::from_str(&blank.settings_json).unwrap();
            changed["server_name"] = Value::String("Concurrent edit".into());
            update_instance(&storage.paths, update(&blank, changed))
                .await
                .unwrap();
            Ok(address)
        };
        assert!(
            prepare_with_sources(&storage, &reservation, blank.clone(), discovery, async {
                Ok(Vec::new())
            },)
            .await
            .is_err()
        );
        let current = read_instance_details(&storage.paths, &before.summary.id).await?;
        let values: Value = serde_json::from_str(&current.settings_json)?;
        assert_eq!(values["server_name"], "Concurrent edit");
        assert_eq!(values["public_ip"], "");
        drop(reservation);

        let reservation = reserve(&state, &before.summary.id);
        let mut pending = Box::pin(prepare_with_sources(
            &storage,
            &reservation,
            current.clone(),
            std::future::pending(),
            async { Ok(Vec::new()) },
        ));
        std::future::poll_fn(|context| {
            assert!(std::future::Future::poll(pending.as_mut(), context).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        state.shutdown_in_progress.store(true, Ordering::SeqCst);
        assert!(
            state
                .drain_storage_operations_for_shutdown(Duration::ZERO)
                .await
                .is_err()
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(1), &mut pending)
                .await?
                .is_err()
        );
        drop(pending);
        assert_eq!(
            read_instance_details(&storage.paths, &before.summary.id)
                .await?
                .settings_json,
            current.settings_json
        );
        drop(reservation);
        drop(app);
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .await;
    drop(environment);
    fs::remove_dir_all(root)?;
    result
}

#[path = "commands_astroneer_connection_tests.rs"]
mod connection_tests;
