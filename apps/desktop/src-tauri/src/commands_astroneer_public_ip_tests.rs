use super::*;
use crate::astroneer_public_ip::prepare_with_discovery;
use std::net::Ipv4Addr;

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
        let before = read_instance_details(&storage.paths, &instance.summary.id).await?;
        let state = app.state::<DesktopState>();
        let reservation = reserve(&state, &before.summary.id);
        let address = Ipv4Addr::new(1, 1, 1, 1);
        let mut expected: Value = serde_json::from_str(&before.settings_json)?;
        assert_eq!(expected["public_ip"], "");

        for value in [
            Value::Bool(false),
            Value::String("10.1.2.3".into()),
            Value::String("224.0.0.1".into()),
        ] {
            let mut invalid = before.clone();
            let mut values = expected.clone();
            values["public_ip"] = value;
            invalid.settings_json = values.to_string();
            assert!(
                prepare_with_discovery(&storage, &reservation, invalid, async {
                    panic!("invalid explicit address must not poll network")
                })
                .await
                .is_err()
            );
        }
        let mut active = before.clone();
        active.summary.status = app_core::InstanceStatus::Running;
        assert!(
            prepare_with_discovery(&storage, &reservation, active, async {
                panic!("active instance must not poll network")
            })
            .await
            .is_err()
        );
        let mut other = before.clone();
        other.summary.module_id = "another-game".into();
        assert!(
            prepare_with_discovery(&storage, &reservation, other, async {
                panic!("other game must not poll network")
            })
            .await
            .is_ok()
        );

        // A failed/oversized/malformed/timeout network result never reaches a write.
        for reason in ["fixture timeout", "fixture invalid body"] {
            assert!(
                prepare_with_discovery(&storage, &reservation, before.clone(), async {
                    Err(reason.into())
                })
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
        let prepared = prepare_with_discovery(&storage, &reservation, before.clone(), async {
            Ok(address)
        })
        .await?;
        expected["public_ip"] = Value::String(address.to_string());
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
        let reused = prepare_with_discovery(&storage, &reservation, persisted.clone(), async {
            panic!("explicit address must not poll network")
        })
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
            prepare_with_discovery(&storage, &reservation, blank.clone(), discovery)
                .await
                .is_err()
        );
        let current = read_instance_details(&storage.paths, &before.summary.id).await?;
        let values: Value = serde_json::from_str(&current.settings_json)?;
        assert_eq!(values["server_name"], "Concurrent edit");
        assert_eq!(values["public_ip"], "");
        drop(reservation);

        let reservation = reserve(&state, &before.summary.id);
        let mut pending = Box::pin(prepare_with_discovery(
            &storage,
            &reservation,
            current.clone(),
            std::future::pending(),
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
        drop(state);
        drop(app);
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .await;
    drop(environment);
    fs::remove_dir_all(root)?;
    result
}
