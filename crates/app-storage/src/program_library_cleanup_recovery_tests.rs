use super::*;
use crate::program_removals_db::{self as removals, ProgramRemovalRecord};

#[tokio::test]
async fn library_cleanup_removed_records_do_not_hide_active_repair_sources() {
    let fixture = Fixture::new().await;
    let base = fixture.directory("palworld");
    let repair = fixture.directory("palworld-repair");
    let removed = fixture.directory("palworld-removed");
    let base_id = fixture
        .register("palworld", &base, InstallState::Installed)
        .await;
    let repair_id = fixture
        .register("palworld", &repair, InstallState::Incomplete)
        .await;
    let removed_id = fixture
        .register("palworld", &removed, InstallState::Installed)
        .await;
    let pool = crate::storage_db::connect_pool(&fixture.paths)
        .await
        .unwrap();
    sqlx::query("UPDATE game_installs SET last_verified_at=NULL, updated_at='2001-01-01 00:00:00'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE game_installs SET install_state='not_installed' WHERE id=?1")
        .bind(removed_id)
        .execute(&pool)
        .await
        .unwrap();
    let selected = crate::read_library_program_install(&fixture.paths, "palworld")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        selected.id, repair_id,
        "a newer incomplete repair still takes precedence"
    );
    sqlx::query("UPDATE game_installs SET install_state='not_installed' WHERE id=?1")
        .bind(repair_id)
        .execute(&pool)
        .await
        .unwrap();
    let selected = crate::read_library_program_install(&fixture.paths, "palworld")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        selected.id, base_id,
        "retained base wins over removed copies"
    );
    sqlx::query("UPDATE game_installs SET install_state='not_installed' WHERE id=?1")
        .bind(base_id)
        .execute(&pool)
        .await
        .unwrap();
    let selected = crate::read_library_program_install(&fixture.paths, "palworld")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        selected.install_root, removed,
        "all removed preserves the previous latest fallback"
    );
    pool.close().await;
}

async fn prepare_removal(fixture: &Fixture, root: &Path, id: i64) -> ProgramRemovalRecord {
    let removal = ProgramRemovalRecord {
        operation_id: uuid::Uuid::new_v4().to_string(),
        module_id: fixture.descriptor.summary.id.clone(),
        install_id: id,
        source_root: root.canonicalize().unwrap(),
        phase: "prepared".into(),
        // This test exercises the storage transaction boundary, not the desktop
        // file journal decoder. The serialized payload must remain byte exact.
        journal_json: "{\"fixture\":\"unchanged recovery intent\"}".into(),
    };
    removals::begin(&fixture.paths, &removal).await.unwrap();
    removal
}

async fn install_row(
    fixture: &Fixture,
    id: i64,
) -> (String, Option<String>, Option<String>, String) {
    let pool = crate::storage_db::connect_pool(&fixture.paths)
        .await
        .unwrap();
    let row = sqlx::query("SELECT install_state,current_version,last_verified_at,updated_at FROM game_installs WHERE id=?1")
        .bind(id).fetch_one(&pool).await.unwrap();
    pool.close().await;
    (
        row.get("install_state"),
        row.get("current_version"),
        row.get("last_verified_at"),
        row.get("updated_at"),
    )
}

#[tokio::test]
async fn library_cleanup_pending_removal_owns_state_during_probe_sync() {
    for committed in [false, true] {
        let fixture = Fixture::new().await;
        let root = fixture.directory("palworld");
        fs::write(root.join("PalServer.exe"), b"preserved program").unwrap();
        let id = fixture
            .register("palworld", &root, InstallState::Installed)
            .await;
        let mut removal = prepare_removal(&fixture, &root, id).await;
        if committed {
            removals::commit(&fixture.paths, &removal).await.unwrap();
            removal.phase = "committed".into();
        }
        let pool = crate::storage_db::connect_pool(&fixture.paths)
            .await
            .unwrap();
        sqlx::query("UPDATE game_installs SET updated_at='2001-01-01 00:00:00',last_verified_at='2001-01-01 00:00:00' WHERE id=?1")
            .bind(id).execute(&pool).await.unwrap();
        pool.close().await;
        let before = install_row(&fixture, id).await;
        assert_eq!(
            before.0,
            if committed {
                "not_installed"
            } else {
                "installed"
            }
        );
        for probed in [
            InstallState::NotInstalled,
            InstallState::Incomplete,
            InstallState::Installed,
        ] {
            sync_game_installs(
                &fixture.paths,
                &[GameInstallSyncRecord {
                    module_id: "palworld".into(),
                    install_root: root.canonicalize().unwrap().to_string_lossy().into_owned(),
                    install_state: probed,
                    current_version: Some("untrusted new probe version".into()),
                    mark_verified: true,
                }],
            )
            .await
            .unwrap();
            assert_eq!(install_row(&fixture, id).await, before);
        }
        let pending = removals::list(&fixture.paths, "palworld").await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].operation_id, removal.operation_id);
        assert_eq!(pending[0].phase, removal.phase);
        assert_eq!(pending[0].journal_json, removal.journal_json);
        assert_eq!(
            fs::read(root.join("PalServer.exe")).unwrap(),
            b"preserved program"
        );
    }
}

#[tokio::test]
async fn library_cleanup_pending_removal_blocks_installer_and_shared_creation_until_recovered() {
    let mut fixture = Fixture::new().await;
    fixture.descriptor = app_modules::discover_modules(&fixture.paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|module| module.summary.id == "minecraft")
        .unwrap();
    let root = fixture.directory("minecraft");
    fs::write(root.join("server.jar"), b"official fixture jar").unwrap();
    fs::create_dir_all(root.join("jre/bin")).unwrap();
    fs::write(root.join("jre/bin/java.exe"), b"official fixture runtime").unwrap();
    fixture.baseline(&root);
    let id = fixture
        .register("minecraft", &root, InstallState::Installed)
        .await;
    let removal = prepare_removal(&fixture, &root, id).await;
    crate::ensure_library_program_path_isolated(&fixture.paths, &root)
        .await
        .unwrap();
    for target in [
        &root,
        &root.canonicalize().unwrap(),
        &root.join("nested-install"),
    ] {
        let error = crate::ensure_library_program_target_available(&fixture.paths, target)
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("再次卸载以完成或恢复"),
            "{error}"
        );
    }
    crate::ensure_library_program_target_available(
        &fixture.paths,
        &fixture.paths.games_root.join("minecraft-neighbor"),
    )
    .await
    .unwrap();
    let create = || {
        crate::create_instance_with_options(
            &fixture.paths,
            &fixture.descriptor,
            app_core::CreateInstanceInput {
                name: "Recovered library".into(),
                module_id: "minecraft".into(),
            },
            crate::InstanceCreationOptions {
                program_install_root: Some(root.clone()),
                program_mode: Some(crate::InstanceProgramMode::Shared),
                require_clean_program: true,
                ..Default::default()
            },
        )
    };
    let error = create().await.unwrap_err();
    assert!(
        error.to_string().contains("再次卸载以完成或恢复"),
        "{error}"
    );
    assert!(
        crate::list_instances(&fixture.paths)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fs::read(root.join("server.jar")).unwrap(),
        b"official fixture jar"
    );
    let owner = crate::read_program_install_owner(&fixture.paths, &root)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(owner.id, id);
    assert_eq!(owner.scope, ProgramInstallScope::Library);
    assert_eq!(owner.install_state, InstallState::Installed);
    removals::finish(&fixture.paths, &removal).await.unwrap();
    crate::ensure_library_program_target_available(&fixture.paths, &root)
        .await
        .unwrap();
    let instance = create().await.unwrap();
    let bound =
        crate::read_instance_program_install(&fixture.paths, &instance.provisioning.summary.id)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(bound.install.id, id);
    assert_eq!(bound.install.scope, ProgramInstallScope::Library);
}
