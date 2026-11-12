#![cfg(test)]
use super::*;
use tauri::Manager;

fn cached_ticket(state: &DesktopState, instance: &str) -> PreparedTicket {
    PreparedTicket {
        instance_id: instance.into(),
        module_id: "dontstarve".into(),
        config_path: "fixture/config/server.ini".into(),
        consumer_app_id: 322330,
        storage_root: PathBuf::new(),
        ids: vec!["123456789".into()],
        job_id: workshop_attempt_job_id(instance, None).unwrap(),
        prepared: None,
        operation: state
            .begin_storage_context_operation("Workshop ticket fixture")
            .unwrap(),
        workspace: None,
        finished: None,
    }
}

#[tokio::test]
async fn preparation_tickets_are_bound_consumed_once_and_cancelled_on_shutdown() {
    let state = DesktopState::default();
    let store = &state.workshop_preparations;
    let id = uuid::Uuid::new_v4().to_string();
    let reservation = store.reserve(&id, "a").unwrap();
    assert!(store.reserve(&id, "a").is_err());
    assert!(
        store.take(&id, "a").is_err(),
        "a running preparation cannot be consumed"
    );
    reservation
        .publish(cached_ticket(&state, "a"), state.app_state.clone())
        .unwrap();
    assert!(
        store.take(&id, "b").is_err(),
        "another instance cannot steal or discard this ticket"
    );
    assert!(state.begin_storage_context_transition().is_err());
    let ticket = store.take(&id, "a").unwrap().unwrap();
    assert!(store.take(&id, "a").unwrap().is_none());
    drop(ticket);
    tokio::task::yield_now().await;
    assert!(state.begin_storage_context_transition().is_ok());
    let id = uuid::Uuid::new_v4().to_string();
    store
        .reserve(&id, "a")
        .unwrap()
        .publish(cached_ticket(&state, "a"), state.app_state.clone())
        .unwrap();
    assert!(state.begin_app_shutdown(true).unwrap());
    state
        .drain_storage_operations_for_shutdown(Duration::from_secs(2))
        .await
        .unwrap();
    assert!(
        store.take(&id, "a").unwrap().is_none(),
        "shutdown releases prepared sources and storage leases"
    );
}

#[tokio::test]
async fn native_workers_are_bounded_and_cancel_waiters_without_leaking_slots() {
    let state = DesktopState::default();
    let store = &state.workshop_preparations;
    let operation = state
        .begin_storage_context_operation("worker fixture")
        .unwrap();
    let first = store.worker(&operation).await.unwrap();
    let second = store.worker(&operation).await.unwrap();
    assert_ne!(first.id, second.id);
    let blocked = state
        .begin_storage_context_operation("queued worker fixture")
        .unwrap();
    blocked.cancellation_token().store(true, Ordering::SeqCst);
    assert!(store.worker(&blocked).await.is_err());
    let freed = first.id;
    drop(first);
    let next = store.worker(&operation).await.unwrap();
    assert_eq!(next.id, freed);
    drop(next);
    drop(second);
}

#[tokio::test]
async fn abandoned_ready_tickets_expire_but_claimed_commits_keep_their_lease() {
    let state = DesktopState::default();
    let store = &state.workshop_preparations;
    let id = uuid::Uuid::new_v4().to_string();
    store
        .reserve(&id, "a")
        .unwrap()
        .publish(cached_ticket(&state, "a"), state.app_state.clone())
        .unwrap();
    assert!(store.retain(&id, "b").is_err());
    store.retain(&id, "a").unwrap();
    assert!(store.take_expired(&id, Instant::now()).unwrap().is_none());
    let expired = store
        .take_expired(&id, Instant::now() + Duration::from_secs(16 * 60))
        .unwrap();
    assert!(
        expired.is_some(),
        "a disconnected client cannot pin storage indefinitely"
    );
    drop(expired);
    tokio::task::yield_now().await;
    assert!(store.retain(&id, "a").is_err());
    assert!(state.begin_storage_context_transition().is_ok());

    let id = uuid::Uuid::new_v4().to_string();
    store
        .reserve(&id, "a")
        .unwrap()
        .publish(cached_ticket(&state, "a"), state.app_state.clone())
        .unwrap();
    let committed = store.take(&id, "a").unwrap().unwrap();
    assert!(
        store
            .take_expired(&id, Instant::now() + Duration::from_secs(16 * 60))
            .unwrap()
            .is_none()
    );
    committed
        .operation
        .cancellation_token()
        .store(true, Ordering::SeqCst);
    // Let the expiry watcher observe cancellation while commit still owns the
    // ticket. It must yield rather than starve the task that releases the lease.
    tokio::time::timeout(Duration::from_secs(1), tokio::task::yield_now())
        .await
        .unwrap();
    assert!(state.begin_storage_context_transition().is_err());
    drop(committed);
    tokio::task::yield_now().await;
    assert!(state.begin_storage_context_transition().is_ok());
}

#[tokio::test]
async fn workers_with_unconfirmed_process_exit_are_never_reused() {
    let state = DesktopState::default();
    let store = &state.workshop_preparations;
    let operation = state
        .begin_storage_context_operation("worker quarantine fixture")
        .unwrap();
    let mut first = store.worker(&operation).await.unwrap();
    let first_id = first.id;
    first.quarantine();
    drop(first);
    let mut remaining = store.worker(&operation).await.unwrap();
    assert_ne!(remaining.id, first_id);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), store.worker(&operation))
            .await
            .is_err()
    );
    remaining.quarantine();
    drop(remaining);
    assert!(
        tokio::time::timeout(Duration::from_secs(1), store.worker(&operation))
            .await
            .unwrap()
            .is_err(),
        "all quarantined workers must reject new work instead of waiting forever"
    );
}

#[tokio::test]
async fn preparation_admission_is_bounded_and_failed_reservations_release_capacity() {
    let state = DesktopState::default();
    let mut reservations = Vec::new();
    for i in 0..32 {
        reservations.push(
            state
                .workshop_preparations
                .reserve(&format!("attempt-{i}"), "a")
                .unwrap(),
        );
    }
    assert!(
        state
            .workshop_preparations
            .reserve("overflow", "a")
            .is_err()
    );
    reservations.pop();
    assert!(
        state
            .workshop_preparations
            .reserve("replacement", "a")
            .is_ok()
    );
}

#[test]
fn attempt_identity_is_exact_and_rejects_malformed_correlation() {
    let id = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        workshop_attempt_job_id("instance-a", Some(&id)).unwrap(),
        format!("workshop-download:instance-a:{id}")
    );
    for invalid in ["", "../runtime", "123456789", "existing-job"] {
        assert!(workshop_attempt_job_id("a", Some(invalid)).is_err());
    }
}

struct Fixture {
    root: PathBuf,
    storage: StorageBootstrap,
    instance: InstanceDetails,
    program: PathBuf,
}

impl Fixture {
    async fn new(module: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let root =
            std::env::temp_dir().join(format!("lg-workshop-{}", uuid::Uuid::new_v4().simple()));
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        let storage = app_storage::bootstrap_storage_with_paths(app_storage::StoragePaths {
            app_data_root: root.join("data"),
            settings_path: root.join("data/settings.json"),
            database_path: root.join("data/db/app.db"),
            logs_root: root.join("data/logs"),
            modules_root: repo.join("modules"),
            migrations_root: repo.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("archives"),
        })?;
        initialize_database(&storage.paths).await?;
        let modules = discover_modules(&storage.paths.modules_root)?;
        let descriptor = find_descriptor(&modules, module)?;
        app_storage::sync_modules(&storage.paths, std::slice::from_ref(descriptor)).await?;
        let library = storage.paths.games_root.join(module);
        let binary = match module {
            "dontstarve" => "bin64/dontstarve_dedicated_server_nullrenderer_x64.exe",
            "palworld" => "Pal/Binaries/Win64/PalServer-Win64-Shipping-Cmd.exe",
            _ => unreachable!(),
        };
        fs::create_dir_all(library.join(binary).parent().unwrap())?;
        fs::write(library.join(binary), b"inert fixture; never launched")?;
        app_storage::record_library_program_baseline(&library, descriptor, true, None)?;
        app_storage::sync_game_installs(
            &storage.paths,
            &[GameInstallSyncRecord {
                module_id: module.into(),
                install_root: library.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: None,
                mark_verified: true,
            }],
        )
        .await?;
        let created = app_storage::create_instance_with_options(
            &storage.paths,
            descriptor,
            CreateInstanceInput {
                name: "Workshop concurrency fixture".into(),
                module_id: module.into(),
            },
            app_storage::InstanceCreationOptions {
                program_mode: Some(app_core::InstanceProgramMode::Independent),
                prefer_existing_install: false,
                require_clean_program: true,
                ..Default::default()
            },
        )
        .await?;
        let instance =
            read_instance_details(&storage.paths, &created.provisioning.summary.id).await?;
        let program = PathBuf::from(
            super::super::super::commands_runtime_lifecycle::private_runtime_install_root(
                &instance,
            )?,
        );
        Ok(Self {
            root,
            storage,
            instance,
            program,
        })
    }

    fn ticket(&self, state: &DesktopState, ids: &[&str]) -> PreparedTicket {
        let mut ticket = cached_ticket(state, &self.instance.summary.id);
        ticket.module_id = self.instance.summary.module_id.clone();
        ticket.config_path = self.instance.config_file_path.clone();
        ticket.storage_root = self.storage.paths.app_data_root.clone();
        ticket.consumer_app_id = if ticket.module_id == "palworld" {
            1623730
        } else {
            322330
        };
        ticket.ids = ids.iter().map(|id| String::from(*id)).collect();
        ticket
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn cached_commit_deploys_requested_items_and_rechecks_the_instance()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("palworld").await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    let cache = fixture.program.join("steamapps/workshop");
    let info = r#"{"PackageName":"FixtureMod"}"#;
    fs::create_dir_all(cache.join("content/1623730/123456789"))?;
    fs::write(cache.join("content/1623730/123456789/Info.json"), info)?;
    fs::write(
        cache.join("content/1623730/123456789/test.pak"),
        "fixture-payload",
    )?;
    fs::write(
        cache.join("appworkshop_1623730.acf"),
        format!(
            "\"AppWorkshop\" {{ \"appid\" \"1623730\" \"WorkshopItemsInstalled\" {{ \"123456789\" {{ \"manifest\" \"1\" \"size\" \"{}\" }} }} }}",
            info.len() + "fixture-payload".len()
        ),
    )?;
    let target = fixture.program.join("Pal/Binaries/Win64/Mods/Workshop");
    fs::create_dir_all(target.join("987654321"))?;
    fs::write(target.join("987654321/keep.txt"), "operator data")?;
    let ticket = fixture.ticket(&state, &["123456789"]);
    let result = commit_inner(&state, &ticket).await?;
    assert!(result.items.iter().all(|item| item.expected_path_exists));
    assert_eq!(
        fs::read_to_string(target.join("123456789/test.pak"))?,
        "fixture-payload"
    );
    assert_eq!(
        fs::read_to_string(target.join("987654321/keep.txt"))?,
        "operator data"
    );
    let queued = fixture.ticket(&state, &["123456789"]);
    let held_instance = state
        .acquire_instance_mutation(&fixture.instance.summary.id)
        .await;
    let cancellation = queued.operation.cancellation_token();
    let (cancelled, ()) = tokio::join!(
        tokio::time::timeout(Duration::from_secs(2), commit_inner(&state, &queued)),
        async move {
            tokio::task::yield_now().await;
            cancellation.store(true, Ordering::SeqCst);
        },
    );
    assert_eq!(cancelled?.unwrap_err(), "installation_cancelled");
    drop(held_instance);
    let mut changed = fixture.ticket(&state, &["123456789"]);
    changed.config_path.push_str(".replaced");
    assert!(
        commit_inner(&state, &changed)
            .await
            .unwrap_err()
            .contains("instance changed")
    );
    assert_eq!(
        read_instance_details(&fixture.storage.paths, &fixture.instance.summary.id)
            .await?
            .settings_json,
        fixture.instance.settings_json
    );
    Ok(())
}

#[tokio::test]
#[ignore = "downloads real public Workshop items into isolated owned directories"]
async fn native_two_workshop_preparations_download_and_commit()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("dontstarve").await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let id = fixture.instance.summary.id.clone();
    let tokens = [
        uuid::Uuid::new_v4().to_string(),
        uuid::Uuid::new_v4().to_string(),
    ];
    let started = Instant::now();
    let (first, second) = tokio::join!(
        prepare_steam_workshop_items(
            app.handle().clone(),
            id.clone(),
            vec!["378160973".into()],
            tokens[0].clone(),
            Some("en-US".into())
        ),
        prepare_steam_workshop_items(
            app.handle().clone(),
            id.clone(),
            vec!["1207269058".into()],
            tokens[1].clone(),
            Some("en-US".into())
        ),
    );
    let first = first?;
    let second = second?;
    let prepared_ms = started.elapsed().as_millis();
    let a = commit_steam_workshop_items(app.handle().clone(), id.clone(), first.ticket_id).await?;
    let b = commit_steam_workshop_items(app.handle().clone(), id.clone(), second.ticket_id).await?;
    assert!(
        a.items
            .iter()
            .chain(&b.items)
            .all(|item| item.expected_path_exists)
    );
    let ugc = Path::new(&fixture.instance.config_file_path)
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .join("data/ugc");
    for shard in app_core::dst_shards::DST_SHARDS {
        for item in ["378160973", "1207269058"] {
            assert!(
                ugc.join(shard.directory)
                    .join("content/322330")
                    .join(item)
                    .join("modinfo.lua")
                    .is_file()
            );
        }
    }
    let state = app.state::<DesktopState>();
    let jobs = state.app_state.read().unwrap().jobs.clone();
    assert!(tokens.iter().all(|token| {
        jobs.iter().any(|job| {
            job.id == format!("workshop-download:{id}:{token}")
                && matches!(job.status, JobStatus::Completed)
        })
    }));
    assert_eq!(
        read_instance_details(&fixture.storage.paths, &id)
            .await?
            .settings_json,
        fixture.instance.settings_json,
    );
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if state.begin_storage_context_transition().is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await?;
    let request_root = fixture.root.join("langame-workshop-workers/requests");
    assert_eq!(
        fs::read_dir(request_root)?.count(),
        0,
        "consumed payloads must be reclaimed"
    );
    let workers = fixture.root.join("langame-workshop-workers");
    for index in 0..2 {
        assert!(
            !workers
                .join(format!("runtime-{index}/steamapps/workshop"))
                .exists(),
            "private workers must not accumulate duplicate payloads"
        );
    }
    if let Some(output) = std::env::var_os("LANGAME_WORKSHOP_CONCURRENCY_EVIDENCE_ROOT") {
        let output = PathBuf::from(output);
        fs::create_dir_all(&output)?;
        for index in 0..2 {
            let logs = workers.join(format!("runtime-{index}/logs/content_log.txt"));
            if logs.is_file() {
                fs::copy(logs, output.join(format!("worker-{index}-content.log")))?;
            }
        }
        fs::write(
            output.join("native-report.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "prepared_ms":prepared_ms, "total_ms":started.elapsed().as_millis(), "jobs":jobs,
                "results":[a,b], "all_dst_shards_verified":true,
                "original_settings_preserved":true, "temporary_payloads_reclaimed":true,
                "worker_payload_cache_reclaimed":true,
            }))?,
        )?;
    }
    Ok(())
}
