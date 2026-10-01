use super::*;
use crate::instance_creation_io::test_gate::count_hash_reads;

#[test]
fn seed_fallback_uses_a_complete_source_without_mixing_package_versions() {
    let fixture = Fixture::new();
    let damaged = fixture.source();
    put(&damaged, "obsolete.bin", b"only in the older package");
    fixture.record(&damaged);
    put(&damaged, "server.bin", b"operator modified executable");
    let healthy = fixture.root.join("healthy");
    put(&healthy, "server.bin", b"verified newer executable");
    put(&healthy, "assets/required.bin", b"verified newer assets");
    fixture.record(&healthy);
    put(&healthy, "personal.cfg", b"never import unknown data");
    let before = crate::test_file_snapshot::tree_snapshot(&fixture.root).unwrap();
    let cancellation = Arc::new(AtomicBool::new(false));
    let reads = count_hash_reads(&cancellation);

    let seed = prepare_seed_at(
        &fixture.target(),
        &fixture.descriptor,
        &[
            SeedSource {
                root: damaged,
                current_version: Some("older-build".into()),
            },
            SeedSource {
                root: healthy,
                current_version: Some("newer-build".into()),
            },
        ],
        Some(&cancellation),
    )
    .unwrap();

    assert!(
        !seed.requires_validation,
        "a complete local source must avoid official repair"
    );
    assert_eq!(seed.current_version.as_deref(), Some("newer-build"));
    assert_eq!(
        fs::read(seed.install_root.join("server.bin")).unwrap(),
        b"verified newer executable"
    );
    assert!(!seed.install_root.join("obsolete.bin").exists());
    assert!(!seed.install_root.join("personal.cfg").exists());
    assert_eq!(
        reads.chunks(),
        5,
        "each candidate payload must be hashed only while copying"
    );
    let after = crate::test_file_snapshot::tree_snapshot(&fixture.root).unwrap();
    for (path, bytes) in before {
        assert_eq!(
            after.get(&path),
            Some(&bytes),
            "source changed: {}",
            path.display()
        );
    }
    fixture.no_staging();
}

async fn initialize(fixture: &Fixture) -> StoragePaths {
    let paths = fixture.paths();
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    crate::initialize_database(&paths).await.unwrap();
    crate::sync_modules(&paths, std::slice::from_ref(&fixture.descriptor))
        .await
        .unwrap();
    paths
}

async fn register(paths: &StoragePaths, root: &Path) {
    crate::sync_game_installs(
        paths,
        &[crate::GameInstallSyncRecord {
            module_id: "fixture".into(),
            install_root: root.to_string_lossy().into_owned(),
            install_state: app_core::InstallState::Installed,
            current_version: Some("official-build".into()),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn seed_fallback_finds_an_older_healthy_library_behind_the_selected_damaged_variant() {
    let fixture = Fixture::new();
    let paths = initialize(&fixture).await;
    let healthy = fixture.source();
    fixture.record(&healthy);
    register(&paths, &healthy).await;
    let damaged = paths.games_root.join("fixture-original");
    put(&damaged, "server.bin", b"official executable");
    fixture.record(&damaged);
    register(&paths, &damaged).await;
    put(&damaged, "server.bin", b"operator modification");
    assert_eq!(
        crate::read_library_program_install(&paths, "fixture")
            .await
            .unwrap()
            .unwrap()
            .install_root,
        damaged
    );
    let _catalog = crate::instance_archive::inventory_lock(&paths).unwrap();

    let seed = prepare_clean_library_seed_at(&paths, &fixture.descriptor, &fixture.target(), None)
        .await
        .unwrap();

    assert!(!seed.requires_validation);
    assert_eq!(
        fs::read(seed.install_root.join("server.bin")).unwrap(),
        b"official executable"
    );
    assert_eq!(
        fs::read(damaged.join("server.bin")).unwrap(),
        b"operator modification"
    );
    fixture.no_staging();
}

#[tokio::test]
async fn seed_fallback_uses_the_second_instance_when_the_first_library_program_is_modified() {
    let fixture = Fixture::new();
    let paths = initialize(&fixture).await;
    let library = fixture.source();
    fixture.record(&library);
    register(&paths, &library).await;
    let create = |name: &str| {
        crate::create_instance_with_options(
            &paths,
            &fixture.descriptor,
            app_core::CreateInstanceInput {
                name: name.into(),
                module_id: "fixture".into(),
            },
            crate::InstanceCreationOptions {
                program_install_root: Some(library.clone()),
                prefer_existing_install: true,
                require_clean_program: true,
                ..Default::default()
            },
        )
    };
    let first = create("First").await.unwrap();
    let second = create("Second").await.unwrap();
    assert_eq!(
        fs::canonicalize(first.effective_install_root).unwrap(),
        fs::canonicalize(&library).unwrap()
    );
    assert_ne!(second.effective_install_root, library);
    put(&library, "server.bin", b"operator modification");
    put(
        &second.effective_install_root,
        "personal.cfg",
        b"second instance data",
    );
    let second_before =
        crate::test_file_snapshot::tree_snapshot(&second.effective_install_root).unwrap();
    let _catalog = crate::instance_archive::inventory_lock(&paths).unwrap();
    let acquisition = crate::new_instance_program_acquisition(&paths, "fixture").unwrap();

    let seed = prepare_instance_program_seed_at(&paths, &fixture.descriptor, &acquisition, None)
        .await
        .unwrap();

    assert!(!seed.requires_validation);
    assert_eq!(
        fs::read(seed.install_root.join("server.bin")).unwrap(),
        b"official executable"
    );
    assert!(!seed.install_root.join("personal.cfg").exists());
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&second.effective_install_root).unwrap(),
        second_before
    );
    assert_eq!(
        fs::read(library.join("server.bin")).unwrap(),
        b"operator modification"
    );
}

#[test]
fn seed_fallback_dropped_waiter_cleans_on_a_worker_and_holds_its_archive_lease() {
    let fixture = Fixture::new();
    let paths = fixture.paths();
    let source = fixture.source();
    fixture.record(&source);
    put(&source, "server.bin", b"modified executable");
    let target = fixture.target();
    let descriptor = fixture.descriptor.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    let stage_path = runtime.block_on(async {
        let mut selected = tokio::task::spawn_blocking(move || {
            sources::select_candidates(
                &target,
                &descriptor,
                &[SeedSource {
                    root: source,
                    current_version: Some("official-build".into()),
                }],
                None,
            )
            .unwrap()
            .unwrap()
        })
        .await
        .unwrap();
        assert!(!selected.complete);
        let stage_path = selected.stage.as_ref().unwrap().path.clone();
        selected._archive_lease = Some(
            crate::instance_settings_lock::acquire_instance_settings_read_lock(
                &paths,
                "seed-fallback-archive",
            )
            .unwrap(),
        );
        let (started, started_rx) = tokio::sync::oneshot::channel();
        let (release, release_rx) = std::sync::mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            started.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        started_rx.await.unwrap();
        let (entered, entered_rx) = tokio::sync::oneshot::channel();
        let waiter = tokio::spawn(async move {
            let _selected = selected;
            entered.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        entered_rx.await.unwrap();
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        assert!(
            stage_path.exists(),
            "async cancellation must queue cleanup on the occupied worker"
        );
        assert!(
            crate::instance_settings_lock::acquire_instance_settings_mutation_lock(
                &paths,
                "seed-fallback-archive",
            )
            .is_err(),
            "cleanup must retain its archive source lease"
        );
        release.send(()).unwrap();
        blocker.await.unwrap();
        stage_path
    });
    // Runtime shutdown joins its owned blocking cleanup without timing sleeps.
    drop(runtime);
    assert!(!stage_path.exists());
    let _released = crate::instance_settings_lock::acquire_instance_settings_mutation_lock(
        &paths,
        "seed-fallback-archive",
    )
    .unwrap();
    assert_eq!(
        fs::read(fixture.root.join("source/server.bin")).unwrap(),
        b"modified executable"
    );
    fixture.no_staging();
}

#[tokio::test]
async fn seed_fallback_keeps_only_one_owned_stage_while_copying_the_next_candidate() {
    let fixture = Fixture::new();
    let damaged = fixture.source();
    fixture.record(&damaged);
    put(&damaged, "server.bin", b"operator modified executable");
    let target = fixture.target();
    let descriptor = fixture.descriptor.clone();
    let first_target = target.clone();
    let first_descriptor = descriptor.clone();
    let selected = tokio::task::spawn_blocking(move || {
        sources::select_candidates(
            &first_target,
            &first_descriptor,
            &[SeedSource {
                root: damaged,
                current_version: None,
            }],
            None,
        )
        .unwrap()
        .unwrap()
    })
    .await
    .unwrap();
    assert!(!selected.complete);
    let first_stage = selected.stage.as_ref().unwrap().path.clone();
    assert!(first_stage.join("assets/required.bin").is_file());
    let healthy = fixture.root.join("healthy");
    put(&healthy, "server.bin", b"official executable");
    fixture.record(&healthy);
    let cancellation = Arc::new(AtomicBool::new(false));
    let mut pause = pause_at(&cancellation, PausePoint::Copy);
    let worker = tokio::task::spawn_blocking(move || {
        let selected = sources::continue_selection(
            &target,
            &descriptor,
            &[SeedSource {
                root: healthy,
                current_version: Some("healthy-build".into()),
            }],
            Some(&cancellation),
            Some(selected),
            None,
        )?;
        publish_seed_at(&target, &descriptor, selected, Some(&cancellation), false)
    });
    pause.reached().await;
    let stages = fs::read_dir(fixture.target().parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join(STAGE_OWNER).is_file())
        .collect::<Vec<_>>();
    assert_eq!(
        stages.len(),
        1,
        "fallback must fit the existing one-copy disk estimate"
    );
    assert!(
        !first_stage.exists(),
        "the previous partial must be removed before copying"
    );
    drop(pause);
    let seed = worker.await.unwrap().unwrap();
    assert!(!seed.requires_validation);
    fixture.no_staging();
}

#[test]
fn seed_fallback_uses_a_healthy_source_when_a_package_directory_became_a_file() {
    let fixture = Fixture::new();
    let damaged = fixture.source();
    fixture.record(&damaged);
    fs::remove_dir_all(damaged.join("assets")).unwrap();
    put(&damaged, "assets", b"operator replaced the directory");
    let healthy = fixture.root.join("healthy");
    put(&healthy, "server.bin", b"official executable");
    put(&healthy, "assets/required.bin", b"official assets");
    fixture.record(&healthy);
    let seed = prepare_seed_at(
        &fixture.target(),
        &fixture.descriptor,
        &[
            SeedSource {
                root: damaged.clone(),
                current_version: None,
            },
            SeedSource {
                root: healthy,
                current_version: Some("healthy-build".into()),
            },
        ],
        None,
    )
    .unwrap();
    assert!(!seed.requires_validation);
    assert_eq!(
        fs::read(seed.install_root.join("assets/required.bin")).unwrap(),
        b"official assets"
    );
    assert_eq!(
        fs::read(damaged.join("assets")).unwrap(),
        b"operator replaced the directory"
    );
    fixture.no_staging();
}
