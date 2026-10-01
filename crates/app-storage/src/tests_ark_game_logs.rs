use super::*;

#[tokio::test]
async fn ark_game_log_sources_are_map_scoped_and_reject_historical_runs() {
    for edition in ["arksurvivalascended", "arksurvivalevolved"] {
        let mut fixture = MapFixture::new(edition).await;
        fixture.add_two_maps().await;
        let id = &fixture.created.summary.id;
        let plans = app_core::ark_maps::processes(&fixture.details).unwrap();
        let identity = ProcessIdentity {
            creation_time: 134_353_044_000_000_000,
            image_path: "synthetic.exe".into(),
        };
        let mut runs = Vec::new();
        for (index, plan) in plans.iter().enumerate() {
            let console = fixture.root.join(format!("run-{index}.log"));
            fs::write(&console, format!("{} console output\n", plan.process_key)).unwrap();
            let run = mark_instance_process_started_with_identity(
                &fixture.paths,
                &StartedInstanceProcess {
                    instance_id: id,
                    session_id: Some("maps"),
                    process_key: &plan.process_key,
                    display_name: &plan.display_name,
                    pid: 9_990_000 + index as u32,
                    log_path: &console.to_string_lossy(),
                    is_primary: index == 0,
                },
                Some(&identity),
            )
            .await
            .unwrap();
            let missing = read_instance_game_log_document(&fixture.paths, id, 400, run.run_id)
                .await
                .unwrap();
            assert_eq!(
                missing.snapshot.source_path.as_deref(),
                Some(plan.native_log_path.as_str())
            );
            assert!(missing.snapshot.lines.is_empty());
            fs::write(
                &plan.native_log_path,
                format!(
                    "[2026.10.01-05.00.01:000][0] {} has successfully started\n",
                    plan.process_key
                ),
            )
            .unwrap();
            let game = read_instance_game_log_document(&fixture.paths, id, 400, run.run_id)
                .await
                .unwrap();
            assert_eq!(game.process_key, plan.process_key);
            assert_eq!(game.run_id, run.run_id);
            assert_eq!(
                game.console_log_path.as_deref(),
                Some(console.to_string_lossy().as_ref())
            );
            assert_eq!(
                game.snapshot.lines,
                [format!(
                    "[2026.10.01-05.00.01:000][0] {} has successfully started",
                    plan.process_key
                )]
            );
            let retained = read_instance_log_document(&fixture.paths, id, 400, Some(run.run_id))
                .await
                .unwrap();
            assert_eq!(
                retained.lines,
                [format!("{} console output", plan.process_key)]
            );
            runs.push(run);
        }
        let foreign = create_instance(
            &fixture.paths,
            &fixture.descriptor,
            CreateInstanceInput {
                name: "Other ARK instance".into(),
                module_id: edition.into(),
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            read_instance_game_log_document(
                &fixture.paths,
                &foreign.summary.id,
                400,
                runs[0].run_id
            )
            .await,
            Err(StorageError::MissingInstanceRun { .. })
        ));
        let first = &runs[0];
        mark_instance_process_stopped(&fixture.paths, id, first.run_id, Some(0), false)
            .await
            .unwrap();
        let pool = crate::storage_db::connect_pool(&fixture.paths)
            .await
            .unwrap();
        sqlx::query("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n WHERE x < 100) INSERT INTO instance_runs (instance_id, process_key, display_name, is_primary, status, started_at, log_path) SELECT ?, 'map-center', 'Center', 0, 'stopped', CURRENT_TIMESTAMP, ? FROM n")
            .bind(id).bind(fixture.root.join("retained.log").to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        pool.close().await;
        assert!(
            read_instance_game_log_document(&fixture.paths, id, 400, runs[0].run_id)
                .await
                .is_ok(),
            "A map's latest run can be older than 96 other map runs"
        );
        assert!(
            read_instance_log_document(&fixture.paths, id, 400, Some(runs[0].run_id))
                .await
                .is_ok(),
            "An exact retained run must not be limited by the recent-run window"
        );
        // The most recent stopped run may still display its shutdown output.
        assert!(
            read_instance_game_log_document(&fixture.paths, id, 400, first.run_id)
                .await
                .is_ok()
        );
        let next_console = fixture.root.join("next-run.log");
        fs::write(&next_console, "next console\n").unwrap();
        let next_identity = ProcessIdentity {
            creation_time: identity.creation_time + 20_000_000,
            image_path: "synthetic.exe".into(),
        };
        let next = mark_instance_process_started_with_identity(
            &fixture.paths,
            &StartedInstanceProcess {
                instance_id: id,
                session_id: Some("next"),
                process_key: "main",
                display_name: "Island",
                pid: 9_991_000,
                log_path: &next_console.to_string_lossy(),
                is_primary: true,
            },
            Some(&next_identity),
        )
        .await
        .unwrap();
        assert!(
            read_instance_game_log_document(&fixture.paths, id, 400, next.run_id)
                .await
                .unwrap()
                .snapshot
                .lines
                .is_empty(),
            "Registering a new process must not relabel its unchanged previous native file"
        );
        fs::write(
            &plans[0].native_log_path,
            "[2026.10.01-05.00.03:000][0] next server has successfully started\n",
        )
        .unwrap();
        assert!(matches!(
            read_instance_game_log_document(&fixture.paths, id, 400, first.run_id).await,
            Err(StorageError::InvalidGameLogSource { .. })
        ));
        assert_eq!(
            read_instance_log_document(&fixture.paths, id, 400, Some(first.run_id))
                .await
                .unwrap()
                .lines,
            ["main console output"]
        );
        assert_eq!(
            read_instance_game_log_document(&fixture.paths, id, 400, next.run_id)
                .await
                .unwrap()
                .snapshot
                .lines,
            ["[2026.10.01-05.00.03:000][0] next server has successfully started"]
        );
        assert!(
            read_instance_game_log_document(&fixture.paths, id, 400, runs[1].run_id)
                .await
                .is_ok(),
            "A newer primary run must not invalidate another map's current file"
        );
        cleanup_root(&fixture.root);
    }
}

#[test]
fn ark_game_log_paths_reject_foreign_modules_and_unsafe_process_keys() {
    for key in [
        "map-../outside",
        "../main",
        "map-",
        "map-UPPER",
        "other",
        "map-/root",
    ] {
        assert!(
            app_core::ark_maps::native_log_path(
                "arksurvivalascended",
                Path::new("owned/logs"),
                key
            )
            .is_err()
        );
    }
    assert!(
        app_core::ark_maps::native_log_path("minecraft", Path::new("owned/logs"), "main").is_err()
    );
}
