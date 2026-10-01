use super::*;
use crate::instance_archive::test_gate::{self, Point};
use crate::test_file_snapshot::tree_snapshot;

async fn pending_archive(fixture: &Fixture) -> store::Archive {
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT archive_id FROM instance_archives WHERE instance_id='archive-instance' AND state='archiving'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        ids.len(),
        1,
        "failed capture must retain one recovery journal"
    );
    let active: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM instances WHERE id='archive-instance'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(active, 1, "failed capture must preserve the instance rows");
    let archive = store::load(&pool, &ids[0]).await.unwrap();
    pool.close().await;
    archive
}

async fn recover_archive(fixture: &Fixture, archive: &store::Archive) {
    recover_instance_archives(&fixture.paths).await.unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let recovered = store::load(&pool, &archive.id).await.unwrap();
    pool.close().await;
    assert_eq!(recovered.state, "archived", "{:?}", recovered.problem);
}

async fn restored_snapshot(fixture: &Fixture) -> store::Snapshot {
    let mut snapshot = fixture.snapshot().await;
    // Restoration intentionally disables automatic startup.
    snapshot.tables.get_mut("instances").unwrap()[0]
        .insert("autostart".into(), serde_json::json!(0));
    snapshot
}

#[tokio::test]
async fn external_capture_rejects_changed_source_without_publishing_or_deleting_it() {
    for changed in [b"other".to_vec(), vec![b'x'; 2 * 1024 * 1024]] {
        let (fixture, library) = external_programs::exclusive_fixture(true).await;
        let before_instance = tree_snapshot(&fixture.instance_root).unwrap();
        let before_library = tree_snapshot(&library).unwrap();
        let before_rows = restored_snapshot(&fixture).await;
        let source = library.join("native/world.dat");
        let original = fs::read(&source).unwrap();
        let mut pause = test_gate::register(&fixture.paths.database_path, Point::Archiving);
        let paths = fixture.paths.clone();
        let worker =
            tokio::spawn(async move { crate::archive_instance(&paths, "archive-instance").await });
        pause.reached().await;
        fs::write(&source, &changed).unwrap();
        let changed_library = tree_snapshot(&library).unwrap();
        pause.resume();
        let error = worker
            .await
            .unwrap()
            .expect_err("changed source must fail capture");
        assert!(
            matches!(&error, StorageError::InvalidInstancePath { .. }),
            "{error:?}"
        );

        let archive = pending_archive(&fixture).await;
        let root = files::archive_path(&fixture.paths, &archive.leaf).unwrap();
        let snapshot = store::snapshot(&archive).unwrap();
        let plan = snapshot.external_program.unwrap();
        let index = plan
            .files
            .keys()
            .position(|key| key == "native/world.dat")
            .unwrap();
        assert!(plan.files["native/world.dat"].stored);
        assert!(plan.files["native/world.dat"].owned);
        assert!(
            !root
                .join(external::PAYLOAD)
                .join(format!("{index:06}"))
                .exists(),
            "unverified bytes must never be published as a completed payload slot"
        );
        if changed.len() != original.len() {
            assert!(
                !root
                    .join(external::PAYLOAD)
                    .join(format!("{index:06}.partial"))
                    .exists(),
                "source growth must be rejected before staging its extra bytes"
            );
        }
        assert_eq!(fs::read(&source).unwrap(), changed);
        assert_eq!(tree_snapshot(&library).unwrap(), changed_library);
        assert_eq!(
            fs::read(root.join("saves/world.dat")).unwrap(),
            b"world state"
        );

        fs::write(&source, original).unwrap();
        recover_archive(&fixture, &archive).await;
        assert!(
            !source.exists(),
            "successful recovery retires owned native data"
        );
        restore_instance_archive(&fixture.paths, &archive.id)
            .await
            .unwrap();
        assert_eq!(
            tree_snapshot(&fixture.instance_root).unwrap(),
            before_instance
        );
        assert_eq!(tree_snapshot(&library).unwrap(), before_library);
        let mut after_rows = fixture.snapshot().await;
        // Rebinding the external library refreshes only these verification
        // timestamps; every saved business field must retain its original value.
        let install = &mut after_rows.tables.get_mut("game_installs").unwrap()[0];
        assert!(
            install["last_verified_at"]
                .as_str()
                .is_some_and(|timestamp| !timestamp.is_empty())
        );
        assert_eq!(install["updated_at"], install["last_verified_at"]);
        for field in ["last_verified_at", "updated_at"] {
            install.insert(
                field.into(),
                before_rows.tables["game_installs"][0][field].clone(),
            );
        }
        assert_eq!(after_rows, before_rows);
    }
}

#[tokio::test]
async fn compact_capture_rechecks_library_after_move_before_omitting_instance_program() {
    for (change_manifest, already_omitted) in [(false, false), (true, false), (false, true)] {
        let (fixture, library) = operations::with_program().await;
        let before_instance = tree_snapshot(&fixture.instance_root).unwrap();
        let before_library = tree_snapshot(&library).unwrap();
        let before_rows = restored_snapshot(&fixture).await;
        let source = library.join(if change_manifest {
            ".langame-clean-package.json"
        } else {
            "assets/content.bin"
        });
        let original = fs::read(&source).unwrap();
        let changed = if change_manifest {
            let mut manifest: serde_json::Value = serde_json::from_slice(&original).unwrap();
            manifest["files"]["server.exe"] = serde_json::json!("0".repeat(64));
            serde_json::to_vec(&manifest).unwrap()
        } else {
            let mut bytes = original.clone();
            bytes[0] ^= 1;
            bytes
        };
        let mut pause = test_gate::register(&fixture.paths.database_path, Point::ArchiveMoved);
        let paths = fixture.paths.clone();
        let worker =
            tokio::spawn(async move { crate::archive_instance(&paths, "archive-instance").await });
        pause.reached().await;
        assert!(
            !fixture.instance_root.exists(),
            "gate must run after the instance move"
        );
        let mut retained_instance = before_instance.clone();
        if already_omitted {
            // Resume can encounter an earlier completed omission. Losing that
            // target must never exempt its reconstruction source from validation.
            let archive = pending_archive(&fixture).await;
            let root = files::archive_path(&fixture.paths, &archive.leaf).unwrap();
            let relative = PathBuf::from("runtime/assets/content.bin");
            assert_eq!(fs::read(root.join(&relative)).unwrap(), original);
            fs::remove_file(root.join(&relative)).unwrap();
            assert!(retained_instance.remove(&relative).is_some());
        }
        fs::write(&source, &changed).unwrap();
        pause.resume();
        let error = worker
            .await
            .unwrap()
            .expect_err("changed library must prevent omission");
        assert!(
            matches!(&error, StorageError::InvalidInstancePath { .. }),
            "{error:?}"
        );

        let archive = pending_archive(&fixture).await;
        let root = files::archive_path(&fixture.paths, &archive.leaf).unwrap();
        assert!(store::snapshot(&archive).unwrap().program.is_some());
        assert_eq!(fs::read(&source).unwrap(), changed);
        assert_eq!(
            tree_snapshot(&root).unwrap(),
            retained_instance,
            "failed reconstruction checks must preserve every remaining instance file"
        );

        fs::write(&source, original).unwrap();
        recover_archive(&fixture, &archive).await;
        assert!(!root.join("runtime/assets/content.bin").exists());
        assert!(!root.join("runtime/server.exe").exists());
        restore_instance_archive(&fixture.paths, &archive.id)
            .await
            .unwrap();
        assert_eq!(
            tree_snapshot(&fixture.instance_root).unwrap(),
            before_instance
        );
        assert_eq!(tree_snapshot(&library).unwrap(), before_library);
        assert_eq!(fixture.snapshot().await, before_rows);
    }
}
