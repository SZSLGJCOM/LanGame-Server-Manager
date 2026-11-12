use super::*;

async fn no_public_lookup() -> Result<Ipv4Addr, String> {
    panic!("this connection mode must not query public address services")
}

fn radmin(address: &str) -> BindAddressCandidate {
    BindAddressCandidate {
        address: address.into(),
        kind: "overlay".into(),
        adapter_name: Some("Fixture adapter".into()),
        family_name: Some("Radmin VPN".into()),
    }
}

fn assert_reason(error: &str, reason: &str) {
    let value: Value = serde_json::from_str(error).unwrap();
    assert_eq!(value["code"], "astroneer_network_preflight_failed");
    assert_eq!(value["reason"], reason);
    assert_eq!(value["failures"], json!([]));
    assert!(!error.contains("26.1.2.3"));
    assert!(!error.contains("Fixture adapter"));
}

#[tokio::test(flavor = "current_thread")]
async fn astroneer_unified_address_selection_persists_and_rechecks_radmin_membership()
-> Result<(), Box<dyn std::error::Error>> {
    let _lock = command_smoke_lock().lock().await;
    let root = temp_test_dir("astroneer-radmin-mode");
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
            "Radmin address fixture",
        )
        .await?;
        let before = read_instance_details(&storage.paths, &created.summary.id).await?;
        let mut values: Value = serde_json::from_str(&before.settings_json)?;
        values["connection_mode"] = Value::String("public".into());
        values["public_ip"] = Value::String(String::new());
        let before = update_instance(&storage.paths, update(&before, values)).await?;
        let state = app.state::<DesktopState>();
        let reservation = reserve(&state, &before.summary.id);
        let jobs_before = serde_json::to_value(&state.app_state.read().unwrap().jobs)?;

        for (candidates, reason) in [
            (Vec::new(), "radmin_unavailable"),
            (vec![radmin("0.0.0.0")], "radmin_unavailable"),
            (
                vec![radmin("26.1.2.3"), radmin("26.4.5.6")],
                "radmin_address_ambiguous",
            ),
        ] {
            // A saved Radmin selection never silently switches to public.
            let mut selected = before.clone();
            let mut values: Value = serde_json::from_str(&selected.settings_json)?;
            values["connection_mode"] = Value::String("radmin".into());
            selected.settings_json = values.to_string();
            let error = prepare_with_sources(
                &storage,
                &reservation,
                selected,
                no_public_lookup(),
                async { Ok(candidates) },
            )
            .await
            .unwrap_err();
            assert_reason(&error, reason);
            assert_eq!(
                read_instance_details(&storage.paths, &before.summary.id)
                    .await?
                    .settings_json,
                before.settings_json,
            );
        }
        for (mode, reason) in [
            (Value::String("manual".into()), "manual_address_required"),
            (Value::String("auto".into()), "invalid_connection_mode"),
            (Value::Null, "invalid_connection_mode"),
        ] {
            let mut invalid = before.clone();
            let mut values: Value = serde_json::from_str(&invalid.settings_json)?;
            values["connection_mode"] = mode;
            invalid.settings_json = values.to_string();
            let error = prepare_with_sources(
                &storage,
                &reservation,
                invalid,
                no_public_lookup(),
                no_adapter_lookup(),
            )
            .await
            .unwrap_err();
            assert_reason(&error, reason);
        }

        // Fresh automatic selection must not guess among multiple Radmin
        // addresses or use public lookup after an inventory failure.
        for candidates in [
            Ok(vec![radmin("26.1.2.3"), radmin("26.4.5.6")]),
            Err(String::from("fixture adapter query failed")),
        ] {
            assert!(
                prepare_with_sources(
                    &storage,
                    &reservation,
                    before.clone(),
                    no_public_lookup(),
                    async { candidates }
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
        let prepared = prepare_with_sources(
            &storage,
            &reservation,
            before.clone(),
            no_public_lookup(),
            async { Ok(vec![radmin("26.1.2.3")]) },
        )
        .await?;
        let mut expected: Value = serde_json::from_str(&before.settings_json)?;
        expected["public_ip"] = Value::String("26.1.2.3".into());
        expected["connection_mode"] = Value::String("radmin".into());
        assert_eq!(
            serde_json::from_str::<Value>(&prepared.settings_json)?,
            expected
        );
        assert_eq!(
            read_instance_details(&storage.paths, &before.summary.id)
                .await?
                .settings_json,
            prepared.settings_json,
        );
        let rendered = app_storage::materialize_instance_configuration_for_start(
            &storage.paths,
            &before.summary.id,
        )
        .await?;
        assert_eq!(rendered.settings_json, prepared.settings_json);
        let install = app_storage::resolve_instance_runtime_root(
            crate::commands::commands_program_storage::instance_root(&rendered)?,
        )?;
        let ini = fs::read_to_string(
            install.join("Astro/Saved/Config/WindowsServer/AstroServerSettings.ini"),
        )?;
        assert!(ini.lines().any(|line| line.trim() == "PublicIP=26.1.2.3"));
        assert!(!ini.contains("connection_mode"));

        let reused = prepare_with_sources(
            &storage,
            &reservation,
            prepared.clone(),
            no_public_lookup(),
            async { Ok(vec![radmin("26.1.2.3"), radmin("26.4.5.6")]) },
        )
        .await?;
        assert_eq!(reused.settings_json, prepared.settings_json);
        for candidates in [Vec::new(), vec![radmin("26.4.5.6")]] {
            let error = prepare_with_sources(
                &storage,
                &reservation,
                prepared.clone(),
                no_public_lookup(),
                async { Ok(candidates) },
            )
            .await
            .unwrap_err();
            assert_reason(&error, "radmin_address_unavailable");
        }

        // A valid explicit address keeps working for legacy, Public and Manual
        // settings, without inferring its route or polling either source.
        for mode in [None, Some("manual"), Some("public")] {
            let mut explicit = prepared.clone();
            let mut values: Value = serde_json::from_str(&explicit.settings_json)?;
            if let Some(mode) = mode {
                values["connection_mode"] = Value::String(mode.into());
            } else {
                values.as_object_mut().unwrap().remove("connection_mode");
            }
            explicit.settings_json = values.to_string();
            let reused = prepare_with_sources(
                &storage,
                &reservation,
                explicit.clone(),
                no_public_lookup(),
                no_adapter_lookup(),
            )
            .await?;
            assert_eq!(reused.settings_json, explicit.settings_json);
        }
        assert_eq!(
            read_instance_details(&storage.paths, &before.summary.id)
                .await?
                .settings_json,
            prepared.settings_json,
        );

        // A fresh adapter query must not let a concurrent edit escape the
        // startup snapshot, whether the address was blank or already saved.
        let mut current = prepared;
        // Exercise the same storage/render/final startup guard for private LAN
        // and shared VPN space. This verifies manager behavior, not client entry.
        for address in ["192.168.1.2", "10.1.2.3", "100.64.1.2"] {
            let mut values: Value = serde_json::from_str(&current.settings_json)?;
            values["public_ip"] = Value::String(address.into());
            values["connection_mode"] = Value::String("manual".into());
            current = update_instance(&storage.paths, update(&current, values)).await?;
            let selected = prepare_with_sources(
                &storage,
                &reservation,
                current.clone(),
                no_public_lookup(),
                no_adapter_lookup(),
            )
            .await?;
            assert_eq!(selected.settings_json, current.settings_json);
            crate::astroneer_console::validate_start_settings(&selected)?;
            let rendered = app_storage::materialize_instance_configuration_for_start(
                &storage.paths,
                &current.summary.id,
            )
            .await?;
            let install = app_storage::resolve_instance_runtime_root(
                crate::commands::commands_program_storage::instance_root(&rendered)?,
            )?;
            let ini = fs::read_to_string(
                install.join("Astro/Saved/Config/WindowsServer/AstroServerSettings.ini"),
            )?;
            assert!(
                ini.lines()
                    .any(|line| line.trim() == format!("PublicIP={address}"))
            );
        }
        for configured in [true, false] {
            let mut values: Value = serde_json::from_str(&current.settings_json)?;
            values["public_ip"] = Value::String(if configured { "26.1.2.3" } else { "" }.into());
            values["connection_mode"] =
                Value::String(if configured { "radmin" } else { "public" }.into());
            current = update_instance(&storage.paths, update(&current, values)).await?;
            let candidates = async {
                let mut values: Value = serde_json::from_str(&current.settings_json).unwrap();
                values["server_name"] =
                    Value::String(format!("Concurrent Radmin edit {configured}"));
                update_instance(&storage.paths, update(&current, values))
                    .await
                    .unwrap();
                Ok(vec![radmin("26.1.2.3")])
            };
            assert!(
                prepare_with_sources(
                    &storage,
                    &reservation,
                    current.clone(),
                    no_public_lookup(),
                    candidates,
                )
                .await
                .is_err()
            );
            current = read_instance_details(&storage.paths, &before.summary.id).await?;
            let values: Value = serde_json::from_str(&current.settings_json)?;
            assert_eq!(
                values["server_name"],
                format!("Concurrent Radmin edit {configured}")
            );
            assert_eq!(
                values["public_ip"],
                if configured { "26.1.2.3" } else { "" }
            );
            assert_eq!(
                values["connection_mode"],
                if configured { "radmin" } else { "public" }
            );
        }

        let mut pending = Box::pin(prepare_with_sources(
            &storage,
            &reservation,
            current.clone(),
            no_public_lookup(),
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
            current.settings_json,
        );
        assert_eq!(
            serde_json::to_value(&state.app_state.read().unwrap().jobs)?,
            jobs_before
        );
        assert!(
            read_active_instance_run(&storage.paths, &before.summary.id)
                .await?
                .is_none()
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
