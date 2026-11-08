use super::*;
use crate::storage_migration;
use app_core::StorageMigrationPhase;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, atomic::AtomicBool};

#[tokio::test]
async fn external_archive_restores_after_migrating_cached_program_metadata() {
    for move_program in [false, true] {
        let (fixture, library) = external_programs::exclusive_fixture(true).await;
        let config = fixture.instance_root.join("config/instance.json");
        let mut value: Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        value["instance_id"] = json!("archive-instance");
        value["module_id"] = json!("archive-fixture");
        fs::write(&config, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        let mut manifests = BTreeMap::new();
        for name in [
            ".langame-clean-package.json",
            ".langame-initial-package.json",
        ] {
            let path = library.join(name);
            let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            // Platforms without file stamps still exercise cache invalidation.
            value
                .as_object_mut()
                .unwrap()
                .entry("verified_files")
                .or_insert(json!({}));
            let bytes = serde_json::to_vec_pretty(&value).unwrap();
            fs::write(&path, &bytes).unwrap();
            manifests.insert(name, (bytes, value));
        }
        let acquisition = ".langame-program-acquisition.json";
        fs::write(
            library.join(acquisition),
            serde_json::to_vec_pretty(&json!({
                "version":1, "id":uuid::Uuid::new_v4().to_string(),
                "module_id":"archive-fixture", "target":library,
            }))
            .unwrap(),
        )
        .unwrap();
        fs::create_dir_all(&fixture.paths.steamcmd_root).unwrap();
        fs::write(
            fixture.paths.steamcmd_root.join("retained.txt"),
            b"tool data",
        )
        .unwrap();
        let archived = fixture.archive().await.unwrap();
        let id = &archived.archive_id;
        let mut requested = fixture.paths.settings();
        let destination = fixture.root.join("migrated");
        requested.servers_root = destination.join("instances").to_string_lossy().into_owned();
        requested.archives_root = destination.join("archives").to_string_lossy().into_owned();
        requested.steamcmd_root = destination.join("steamcmd").to_string_lossy().into_owned();
        if move_program {
            requested.games_root = destination.join("games").to_string_lossy().into_owned();
        }
        let cancellation = Arc::new(AtomicBool::new(false));
        let progress: storage_migration::Progress = Arc::new(|_| {});
        let prepared = storage_migration::preview(
            &fixture.paths,
            requested,
            uuid::Uuid::new_v4().to_string(),
            Vec::new(),
            cancellation.clone(),
            progress.clone(),
        )
        .await
        .unwrap();
        assert_eq!(prepared.status().phase, StorageMigrationPhase::Ready);
        let completed =
            storage_migration::execute(&fixture.paths, prepared, cancellation, progress)
                .await
                .unwrap();
        assert_eq!(completed.phase, StorageMigrationPhase::Completed);
        assert!(completed.cleanup_warnings.is_empty());
        let persisted = crate::settings::load_persisted_app_settings(&fixture.paths)
            .unwrap()
            .unwrap();
        let next = fixture.paths.with_app_settings(&persisted);
        let program = next.games_root.join("archive-fixture");
        let pool = connect_pool(&next).await.unwrap();
        let archive = store::load(&pool, id).await.unwrap();
        let snapshot = store::snapshot(&archive).unwrap();
        pool.close().await;
        let inventory = snapshot.external_program.unwrap();
        let mut captured = BTreeMap::new();
        for name in manifests.keys().copied().chain([acquisition]) {
            let index = inventory.files.keys().position(|key| key == name).unwrap();
            let slot = next
                .archives_root
                .join(id)
                .join(external::PAYLOAD)
                .join(format!("{index:06}"));
            captured.insert(name, fs::read(slot).unwrap());
        }
        restore_instance_archive(&next, id).await.unwrap();
        for (name, (before, mut expected)) in manifests {
            if move_program {
                expected.as_object_mut().unwrap().remove("verified_files");
            }
            let actual = fs::read(program.join(name)).unwrap();
            assert_eq!(serde_json::from_slice::<Value>(&actual).unwrap(), expected);
            assert_eq!(
                captured[name], actual,
                "archive payload must match migrated metadata"
            );
            assert_eq!(inventory.files[name].sha256, store::digest(&actual));
            assert_eq!(inventory.files[name].bytes, actual.len() as u64);
            if !move_program {
                assert_eq!(actual, before, "unmoved source keeps its original cache");
            }
        }
        let acquisition_bytes = fs::read(program.join(acquisition)).unwrap();
        assert_eq!(captured[acquisition], acquisition_bytes);
        let acquisition_value: Value = serde_json::from_slice(&acquisition_bytes).unwrap();
        assert_eq!(
            crate::instance_isolation::paths::normalize_path(Path::new(
                acquisition_value["target"].as_str().unwrap()
            ))
            .unwrap(),
            crate::instance_isolation::paths::normalize_path(&program).unwrap(),
        );
        for (relative, expected) in [
            ("server.exe", b"official bytes".as_slice()),
            ("native/world.dat", b"world"),
            ("Mods/custom.dll", b"mod"),
            ("custom.dll", b"custom"),
            ("unowned/foreign.dat", b"foreign"),
        ] {
            assert_eq!(fs::read(program.join(relative)).unwrap(), expected);
        }
        assert_eq!(
            fs::read(next.steamcmd_root.join("retained.txt")).unwrap(),
            b"tool data"
        );
        let restored = next.instances_root.join("archive-instance");
        assert!(!restored.join(external::PAYLOAD).exists());
        assert!(
            !program
                .join(format!(
                    ".langame-archive-restore-{}",
                    inventory.restore_token
                ))
                .exists()
        );
        assert!(!fixture.instance_root.exists());
    }
}
