use super::*;

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
    descriptor: ModuleDescriptor,
}

impl Fixture {
    async fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("langame-creation-source-{}", uuid::Uuid::new_v4()));
        let module_root = root.join("modules/fixture");
        fs::create_dir_all(&module_root).unwrap();
        fs::write(
            module_root.join("module.toml"),
            r#"
id = "fixture"
name = "Creation source fixture"
version = "1.0.0"
[install]
shared_game_dir = "fixture"
[process]
executable = "server.bin"
"#,
        )
        .unwrap();
        let descriptor = app_modules::discover_modules(root.join("modules"))
            .unwrap()
            .remove(0);
        let storage = crate::bootstrap_storage_with_paths(StoragePaths {
            app_data_root: root.join("app-data"),
            settings_path: root.join("app-data/settings.json"),
            database_path: root.join("app-data/db/test.db"),
            logs_root: root.join("logs"),
            modules_root: root.join("modules"),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances/.trash"),
        })
        .unwrap();
        initialize_database(&storage.paths).await.unwrap();
        sync_modules(&storage.paths, std::slice::from_ref(&descriptor))
            .await
            .unwrap();
        Self {
            root,
            paths: storage.paths,
            descriptor,
        }
    }

    fn package(&self, name: &str, content: &[u8]) -> PathBuf {
        let root = self.paths.games_root.join(name);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("server.bin"), content).unwrap();
        crate::record_library_program_baseline(&root, &self.descriptor, true, None).unwrap();
        root
    }

    async fn register(&self, root: &Path, module_id: &str, state: InstallState) {
        sync_game_installs(
            &self.paths,
            &[GameInstallSyncRecord {
                module_id: module_id.into(),
                install_root: root.to_string_lossy().into_owned(),
                install_state: state,
                current_version: Some("fixture-build".into()),
                mark_verified: true,
            }],
        )
        .await
        .unwrap();
    }

    async fn create(
        &self,
        selected: Option<PathBuf>,
        prefer_existing: bool,
    ) -> Result<CreateInstanceResult, StorageError> {
        create_instance_with_options(
            &self.paths,
            &self.descriptor,
            CreateInstanceInput {
                name: "Selected source".into(),
                module_id: self.descriptor.summary.id.clone(),
            },
            InstanceCreationOptions {
                program_install_root: selected,
                prefer_existing_install: prefer_existing,
                require_clean_program: true,
                program_mode: Some(InstanceProgramMode::Independent),
                ..Default::default()
            },
        )
        .await
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("remove owned source fixture");
    }
}

#[tokio::test]
async fn explicit_creation_source_survives_a_later_refresh_of_an_older_library() {
    let fixture = Fixture::new().await;
    let old = fixture.package("old-library", b"old verified package");
    let repaired = fixture.package("repaired-library", b"repaired verified package");
    fixture
        .register(&old, "fixture", InstallState::Installed)
        .await;
    fixture
        .register(&repaired, "fixture", InstallState::Installed)
        .await;
    let selected = crate::read_program_install_owner(&fixture.paths, &repaired)
        .await
        .unwrap()
        .unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    // A library scan may refresh the old record after repair. Fixed timestamps
    // reproduce the ordering without depending on clock resolution or sleeps.
    sqlx::query("UPDATE game_installs SET last_verified_at=NULL, updated_at=CASE WHEN id=?1 THEN '2000-01-01 00:00:00' ELSE '2100-01-01 00:00:00' END")
        .bind(selected.id).execute(&pool).await.unwrap();
    assert_eq!(
        resolve_installed_game_install_id_from_pool(&pool, "fixture")
            .await
            .unwrap(),
        crate::read_program_install_owner(&fixture.paths, &old)
            .await
            .unwrap()
            .map(|record| record.id)
    );
    pool.close().await;

    let default_created = fixture.create(None, true).await.unwrap();
    assert_eq!(
        fs::canonicalize(default_created.effective_install_root).unwrap(),
        fs::canonicalize(&old).unwrap()
    );
    let created = fixture.create(Some(repaired.clone()), true).await.unwrap();
    assert_eq!(
        fs::canonicalize(&created.effective_install_root).unwrap(),
        fs::canonicalize(&repaired).unwrap()
    );
    assert_eq!(
        fs::read(created.effective_install_root.join("server.bin")).unwrap(),
        b"repaired verified package"
    );
    let binding =
        crate::read_instance_program_install(&fixture.paths, &created.provisioning.summary.id)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(binding.install.id, selected.id);
    assert_eq!(
        fs::read(old.join("server.bin")).unwrap(),
        b"old verified package"
    );
    assert_eq!(list_instances(&fixture.paths).await.unwrap().len(), 2);
}

#[tokio::test]
async fn explicit_creation_source_rejects_unregistered_foreign_and_incomplete_libraries() {
    for source_kind in ["unregistered", "foreign", "incomplete"] {
        let fixture = Fixture::new().await;
        let root = fixture.package("selected-library", b"preserve selected program");
        if source_kind == "foreign" {
            let mut other = fixture.descriptor.clone();
            other.summary.id = "other".into();
            sync_modules(&fixture.paths, &[other]).await.unwrap();
            fixture
                .register(&root, "other", InstallState::Installed)
                .await;
        } else if source_kind == "incomplete" {
            fixture
                .register(&root, "fixture", InstallState::Incomplete)
                .await;
        }
        let error = fixture.create(Some(root.clone()), true).await.unwrap_err();
        assert!(
            error.to_string().contains("selected program source"),
            "{error}"
        );
        assert!(list_instances(&fixture.paths).await.unwrap().is_empty());
        assert_eq!(
            fs::read(root.join("server.bin")).unwrap(),
            b"preserve selected program"
        );
    }
}

#[tokio::test]
async fn explicit_creation_source_rejects_an_instance_owned_program() {
    let fixture = Fixture::new().await;
    let library = fixture.package("library", b"official program");
    fixture
        .register(&library, "fixture", InstallState::Installed)
        .await;
    let first = fixture.create(Some(library), false).await.unwrap();
    let binding =
        crate::read_instance_program_install(&fixture.paths, &first.provisioning.summary.id)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(binding.install.scope, crate::ProgramInstallScope::Instance);
    let error = fixture
        .create(Some(first.effective_install_root.clone()), true)
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("selected program source"),
        "{error}"
    );
    assert_eq!(list_instances(&fixture.paths).await.unwrap().len(), 1);
    assert_eq!(
        fs::read(first.effective_install_root.join("server.bin")).unwrap(),
        b"official program"
    );
}
