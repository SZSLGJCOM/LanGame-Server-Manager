use super::*;
use crate::commands_knowledge;
use app_storage::{StorageBootstrap, StoragePaths};
use std::future::Future;
use std::task::Poll;

struct Fixture {
    root: PathBuf,
    storage: StorageBootstrap,
    catalog: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-knowledge-scope-{}", uuid::Uuid::new_v4()));
        let app_data = root.join("state");
        let paths = StoragePaths {
            app_data_root: app_data.clone(),
            settings_path: app_data.join("settings.json"),
            database_path: app_data.join("db/lgs.db"),
            logs_root: app_data.join("logs"),
            modules_root: root.join("modules"),
            migrations_root: root.join("migrations"),
            instances_root: root.join("instances"),
            archives_root: root.join("archives"),
            games_root: root.join("games"),
            steamcmd_root: root.join("steamcmd"),
        };
        let catalog = paths.modules_root.join("fixture/knowledge-sources.toml");
        std::fs::create_dir_all(catalog.parent().unwrap()).unwrap();
        std::fs::write(
            &catalog,
            r#"
schema_version = 1
module_id = "fixture"
scope = "Fixture publisher reference"
gaps = []
[[sources]]
id = "publisher"
title = "Publisher manual"
authority = "Publisher"
kind = "official"
seeds = ["https://example.com/docs/start"]
allowed_prefixes = ["https://example.com/docs/"]
discover_links = false
reference_only = true
max_pages = 1
authority_evidence = "https://example.com/about"
license_note = "Reference links only"
reviewed_on = "2026-09-28"
"#,
        )
        .unwrap();
        // Do not bootstrap: none of the configurable roots exist. A knowledge
        // entrypoint that starts initializing them will make the test fail.
        let storage = StorageBootstrap {
            settings: paths.settings(),
            paths,
            storage_status: app_core::StorageStatus::default(),
        };
        Self {
            root,
            storage,
            catalog,
        }
    }

    fn state(&self) -> DesktopState {
        DesktopState::from_storage(&self.storage)
    }

    fn assert_runtime_storage_untouched(&self) {
        for path in [
            &self.storage.paths.instances_root,
            &self.storage.paths.archives_root,
            &self.storage.paths.games_root,
            &self.storage.paths.steamcmd_root,
            &self.storage.paths.settings_path,
            &self.storage.paths.database_path,
            &self.storage.paths.logs_root,
        ] {
            assert!(
                !path.exists(),
                "Knowledge initialized runtime storage: {}",
                path.display()
            );
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(self.root.starts_with(std::env::temp_dir()));
        assert!(
            self.root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("lgsm-knowledge-scope-")
        );
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn knowledge_commands_and_schedule_use_only_fixed_data_during_migration() {
    let fixture = Fixture::new();
    let catalog = std::fs::read(&fixture.catalog).unwrap();
    let app = tauri::test::mock_builder()
        .manage(fixture.state())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let state = app.state::<DesktopState>();
    let transition = state.begin_storage_context_transition().unwrap();
    commands_knowledge::update_knowledge_settings(
        state.clone(),
        KnowledgeSettings {
            auto_update: false,
            interval_hours: 48,
        },
    )
    .await
    .unwrap();
    let status = commands_knowledge::read_knowledge_status(state.clone())
        .await
        .unwrap();
    assert!(!status.library.settings.auto_update);
    assert_eq!(status.library.settings.interval_hours, 48);
    assert_eq!(status.library.games[0].module_id, "fixture");
    state
        .knowledge
        .check_schedule(&state, app_knowledge::unix_seconds())
        .await
        .unwrap();
    assert!(state.knowledge.lock().unwrap().worker.is_none());
    assert!(
        fixture
            .storage
            .paths
            .app_data_root
            .join("knowledge/library.sqlite3")
            .is_file()
    );
    assert_eq!(std::fs::read(&fixture.catalog).unwrap(), catalog);
    fixture.assert_runtime_storage_untouched();
    drop(transition);
    state.knowledge.shutdown().await;
}

#[tokio::test]
async fn starting_knowledge_sync_holds_only_a_fixed_data_lease() {
    let fixture = Fixture::new();
    let state = fixture.state();
    let owner = state.knowledge.clone();
    let library = owner.library.lock().await;
    let transition = state.begin_storage_context_transition().unwrap();
    let mut start = Box::pin(owner.start(&state, None, false));
    // Pause at actual library admission, before any model or network work.
    std::future::poll_fn(|context| {
        assert!(start.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(transition);
    assert!(state.begin_storage_context_transition().is_ok());
    state.shutdown_in_progress.store(true, Ordering::SeqCst);
    assert!(
        state.begin_storage_shutdown_exclusive().is_err(),
        "The fixed-data lease still belongs to the admitted start"
    );
    drop(start);
    assert!(state.begin_storage_shutdown_exclusive().is_ok());
    drop(library);
    fixture.assert_runtime_storage_untouched();
    assert!(
        !fixture
            .storage
            .paths
            .app_data_root
            .join("knowledge")
            .exists()
    );
    owner.shutdown().await;
}
