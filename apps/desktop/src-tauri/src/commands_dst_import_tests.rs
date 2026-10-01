use super::*;

fn dst_import_test_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "lg-dst-import-{label}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir(&root).expect("create DST import test root");
    root
}

fn write_valid_dst_shard(cluster_root: &Path, shard: &str, session: &str) {
    let save_root = cluster_root.join(shard).join("save");
    fs::create_dir_all(save_root.join("session").join(session))
        .expect("create synthetic DST save tree");
    fs::write(
        save_root.join("shardindex"),
        format!("return {{ session_id = \"{session}\", enabled_mods = {{}} }}"),
    )
    .expect("write synthetic shardindex");
    fs::write(
        save_root.join("session").join(session).join("0000000001"),
        b"\x00synthetic DST world snapshot",
    )
    .expect("write synthetic world session");
    fs::write(
        save_root
            .join("session")
            .join(session)
            .join("0000000001.meta"),
        b"\x00synthetic DST snapshot metadata",
    )
    .expect("write synthetic world session metadata");
}

fn write_managed_shard_configuration(cluster_root: &Path, shard: &str, value: &str) {
    let shard_root = cluster_root.join(shard);
    fs::create_dir_all(&shard_root).unwrap();
    for name in [
        "server.ini",
        "worldgenoverride.lua",
        "leveldataoverride.lua",
        "modoverrides.lua",
    ] {
        fs::write(shard_root.join(name), format!("{value}-{name}")).unwrap();
    }
}

#[test]
fn dst_source_resolution_rejects_empty_and_configuration_only_shards() {
    let root = dst_import_test_root("invalid-source");
    let empty_cluster = root.join("empty");
    fs::create_dir_all(empty_cluster.join("Master")).unwrap();

    let config_only_cluster = root.join("config-only");
    let config_only_master = config_only_cluster.join("Master");
    fs::create_dir_all(&config_only_master).unwrap();
    for name in ["server.ini", "worldgenoverride.lua", "modoverrides.lua"] {
        fs::write(config_only_master.join(name), "managed configuration").unwrap();
    }

    let fake_cluster = root.join("fake-save");
    let fake_save = fake_cluster.join("Master/save");
    fs::create_dir_all(fake_save.join("session/not-a-world")).unwrap();
    fs::write(fake_save.join("shardindex"), "synthetic index").unwrap();
    fs::write(
        fake_save.join("session/not-a-world/README.txt"),
        "not a DST world snapshot",
    )
    .unwrap();
    fs::write(
        fake_save.join("session/not-a-world/0000000001"),
        b"\x00snapshot without its required metadata pair",
    )
    .unwrap();

    assert_eq!(resolve_dontstarve_cluster_source_root(&empty_cluster), None);
    assert_eq!(
        resolve_dontstarve_cluster_source_root(&config_only_cluster),
        None
    );
    assert_eq!(resolve_dontstarve_cluster_source_root(&fake_cluster), None);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn dst_source_resolution_accepts_a_real_master_save() {
    let root = dst_import_test_root("valid-source");
    let cluster_root = root.join("config").join("clusters").join("main");
    write_valid_dst_shard(&cluster_root, "Master", "A1B2C3");

    assert_eq!(
        resolve_dontstarve_cluster_source_root(&cluster_root),
        Some(cluster_root.clone())
    );
    assert_eq!(
        resolve_dontstarve_cluster_source_root(&root.join("config")),
        Some(cluster_root.clone())
    );
    assert_eq!(
        resolve_dontstarve_cluster_source_root(&cluster_root.join("Master")),
        Some(cluster_root.clone())
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn dst_import_rejects_orphan_snapshot_not_referenced_by_shardindex() {
    let root = dst_import_test_root("orphan-session");
    let cluster_root = root.join("cluster");
    write_valid_dst_shard(&cluster_root, "Master", "ORPHAN");
    fs::write(
        cluster_root.join("Master/save/shardindex"),
        "KLEI     1 return { session_id = \"REFERENCED\" }",
    )
    .unwrap();

    let error = validate_dontstarve_import_source(&cluster_root).unwrap_err();
    assert!(error.contains("REFERENCED"), "{error}");
    assert!(error.contains("snapshot"), "{error}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn dst_import_rejects_clusters_with_additional_shard_directories() {
    let root = dst_import_test_root("extra-shard");
    let cluster_root = root.join("cluster");
    write_valid_dst_shard(&cluster_root, "Master", "MASTER");
    write_valid_dst_shard(&cluster_root, "Caves", "CAVES");
    write_valid_dst_shard(&cluster_root, "Moon", "MOON");

    let error = validate_dontstarve_import_source(&cluster_root).unwrap_err();
    assert!(error.contains("Moon"), "{error}");
    assert!(error.contains("Master and Caves"), "{error}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn dst_import_rejects_target_with_additional_shard_without_touching_it() {
    let root = dst_import_test_root("extra-target-shard");
    let source_root = root.join("source");
    let target_root = root.join("target");
    write_valid_dst_shard(&source_root, "Master", "SOURCE");
    write_valid_dst_shard(&target_root, "Master", "TARGET");
    write_valid_dst_shard(&target_root, "Moon", "MOON");
    let retained = target_root.join("Moon/save/session/MOON/0000000001");

    let source = validate_dontstarve_import_source(&source_root).unwrap();
    let error = import_dontstarve_world_transaction(&source, &target_root, false).unwrap_err();
    assert!(error.contains("Moon"), "{error}");
    assert!(retained.is_file());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn dst_import_rejects_oversized_shardindex_before_parsing() {
    let root = dst_import_test_root("oversized-shardindex");
    let cluster_root = root.join("cluster");
    write_valid_dst_shard(&cluster_root, "Master", "SESSION");
    fs::write(
        cluster_root.join("Master/save/shardindex"),
        vec![b'x'; 1024 * 1024 + 1],
    )
    .unwrap();

    let error = validate_dontstarve_import_source(&cluster_root).unwrap_err();
    assert!(error.contains("1 MiB"), "{error}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn dst_import_validation_budgets_nested_trees() {
    let root = dst_import_test_root("deep-save-tree");
    let mut directory = root.clone();
    for _ in 0..=64 {
        directory = directory.join("d");
        fs::create_dir(&directory).unwrap();
    }

    let error = validate_plain_tree(&root, &root).unwrap_err();
    assert!(error.contains("depth limit"), "{error}");

    let _ = fs::remove_dir_all(root);
}

#[test]
#[ignore = "requires LANGAME_DST_CLUSTER_ROOT pointing to an operator-selected native cluster"]
fn dst_import_accepts_an_external_native_cluster() {
    let cluster_root = std::env::var_os("LANGAME_DST_CLUSTER_ROOT")
        .map(PathBuf::from)
        .expect("LANGAME_DST_CLUSTER_ROOT must point to a native cluster directory");
    let source = validate_dontstarve_import_source(&cluster_root).unwrap();
    assert_eq!(source.cluster_root, cluster_root);
    assert!(source.master_root.is_dir());
}

#[test]
fn dst_import_rejects_overlapping_source_and_target_paths() {
    let root = dst_import_test_root("overlap");
    let target = root.join("target");
    let source_inside_target = target.join("source");
    write_valid_dst_shard(&source_inside_target, "Master", "SOURCE");

    let error = validate_dontstarve_import_paths(&source_inside_target, &target).unwrap_err();
    assert!(error.contains("contain one another"));

    let source = root.join("outer-source");
    write_valid_dst_shard(&source, "Master", "SOURCE");
    let target_inside_source = source.join("nested-target");
    let error = validate_dontstarve_import_paths(&source, &target_inside_source).unwrap_err();
    assert!(error.contains("contain one another"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn dst_import_rejects_master_only_save_when_caves_are_enabled_without_touching_target() {
    let root = dst_import_test_root("missing-caves");
    let source_root = root.join("source");
    let target_root = root.join("target");
    write_valid_dst_shard(&source_root, "Master", "SOURCE");
    write_valid_dst_shard(&target_root, "Master", "TARGET-MASTER");
    write_valid_dst_shard(&target_root, "Caves", "TARGET-CAVES");

    let source = validate_dontstarve_import_source(&source_root).unwrap();
    let error = import_dontstarve_world_transaction(&source, &target_root, true).unwrap_err();
    assert!(error.contains("caves enabled"));
    assert_eq!(
        fs::read(target_root.join("Caves/save/session/TARGET-CAVES/0000000001")).unwrap(),
        b"\x00synthetic DST world snapshot"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn dst_import_master_only_clears_disabled_stale_caves_and_preserves_target_configuration() {
    let root = dst_import_test_root("master-only");
    let source_root = root.join("source");
    let target_root = root.join("target");
    write_valid_dst_shard(&source_root, "Master", "SOURCE");
    write_managed_shard_configuration(&source_root, "Master", "source-config");
    write_valid_dst_shard(&target_root, "Master", "TARGET-MASTER");
    write_valid_dst_shard(&target_root, "Caves", "TARGET-CAVES");
    write_managed_shard_configuration(&target_root, "Master", "target-config");
    write_managed_shard_configuration(&target_root, "Caves", "target-caves-config");

    let source = validate_dontstarve_import_source(&source_root).unwrap();
    let result = import_dontstarve_world_transaction(&source, &target_root, false)
        .unwrap()
        .commit();

    assert!(result.imported_master);
    assert!(!result.imported_caves);
    assert!(
        target_root
            .join("Master/save/session/SOURCE/0000000001")
            .is_file()
    );
    assert!(
        !target_root
            .join("Master/save/session/TARGET-MASTER")
            .exists()
    );
    assert!(!target_root.join("Caves/save").exists());
    assert_eq!(
        fs::read_to_string(target_root.join("Master/leveldataoverride.lua")).unwrap(),
        "target-config-leveldataoverride.lua"
    );
    assert_eq!(
        fs::read_to_string(target_root.join("Master/modoverrides.lua")).unwrap(),
        "target-config-modoverrides.lua"
    );
    assert_eq!(
        fs::read_to_string(target_root.join("Caves/server.ini")).unwrap(),
        "target-caves-config-server.ini"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn dst_import_recognizes_complete_island_adventures_and_imports_each_world() {
    let root = dst_import_test_root("four-shards");
    let source_root = root.join("source");
    let target_root = root.join("target");
    for spec in app_core::dst_shards::DST_SHARDS {
        write_valid_dst_shard(&source_root, spec.directory, spec.directory);
        write_valid_dst_shard(
            &target_root,
            spec.directory,
            &format!("OLD-{}", spec.directory),
        );
    }
    let source = validate_dontstarve_import_source(&source_root.join("Islands")).unwrap();
    assert!(source.is_island_adventures());
    let plan = super::dst_import_mods::read_import_mods(&source.shards()).unwrap();
    assert!(plan.workshop_ids.is_empty());
    let result = import_dontstarve_world_transaction(&source, &target_root, true)
        .unwrap()
        .commit();
    assert_eq!(
        result.imported_shards,
        ["Master", "Caves", "Islands", "Volcano"]
    );
    for spec in app_core::dst_shards::DST_SHARDS {
        assert!(
            target_root
                .join(spec.directory)
                .join("save/session")
                .join(spec.directory)
                .join("0000000001")
                .is_file()
        );
        assert!(
            !target_root
                .join(spec.directory)
                .join("save/session")
                .join(format!("OLD-{}", spec.directory))
                .exists()
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dst_import_rejects_incomplete_island_adventures_without_regenerating_shards() {
    for omitted in ["Caves", "Islands", "Volcano"] {
        let root = dst_import_test_root("missing-island-shard");
        for shard in ["Master", "Caves", "Islands", "Volcano"] {
            if shard != omitted {
                write_valid_dst_shard(&root, shard, shard);
            }
        }
        let error = validate_dontstarve_import_source(&root).unwrap_err();
        assert!(error.contains("all four valid shard saves"), "{error}");
        assert!(!root.join(omitted).exists());
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn dst_import_standard_cluster_ignores_prepared_inactive_shard_configuration() {
    let root = dst_import_test_root("inactive-config-shards");
    write_valid_dst_shard(&root, "Master", "MASTER");
    for shard in ["Caves", "Islands", "Volcano"] {
        write_managed_shard_configuration(&root, shard, "prepared");
    }
    let source = validate_dontstarve_import_source(&root).unwrap();
    assert_eq!(
        source
            .shards()
            .iter()
            .map(|(key, _)| *key)
            .collect::<Vec<_>>(),
        ["master"]
    );
    assert!(!source.is_island_adventures());
    assert!(
        validate_dontstarve_import_policy(&source, true)
            .unwrap_err()
            .contains("caves enabled")
    );
    write_valid_dst_shard(&root, "Caves", "CAVES");
    let source = validate_dontstarve_import_source(&root).unwrap();
    assert_eq!(source.shards().len(), 2);
    assert!(!source.is_island_adventures());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dst_import_never_ignores_partial_world_data_in_a_prepared_shard() {
    for shard in ["Caves", "Islands", "Volcano"] {
        let root = dst_import_test_root("inactive-partial-world");
        write_valid_dst_shard(&root, "Master", "MASTER");
        write_managed_shard_configuration(&root, shard, "prepared");
        fs::create_dir_all(root.join(shard).join("save/session")).unwrap();
        let error = validate_dontstarve_import_source(&root).unwrap_err();
        assert!(error.contains("shardindex"), "{shard}: {error}");
        fs::remove_dir_all(root).unwrap();
    }
    let root = dst_import_test_root("partial-generated-islands");
    for shard in ["Master", "Caves", "Islands"] {
        write_valid_dst_shard(&root, shard, shard);
    }
    write_managed_shard_configuration(&root, "Volcano", "prepared");
    let error = validate_dontstarve_import_source(&root).unwrap_err();
    assert!(error.contains("all four valid shard saves"), "{error}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dst_import_configuration_failure_restores_original_world_and_native_configuration() {
    let root = dst_import_test_root("configuration-rollback");
    let source_root = root.join("source");
    let target_root = root.join("target");
    write_valid_dst_shard(&source_root, "Master", "SOURCE");
    write_valid_dst_shard(&target_root, "Master", "ORIGINAL");
    write_managed_shard_configuration(&target_root, "Master", "original");
    let source = validate_dontstarve_import_source(&source_root).unwrap();
    let published = import_dontstarve_world_transaction(&source, &target_root, false).unwrap();
    assert!(target_root.join("Master/save/session/SOURCE").exists());
    // A settings publisher may have written files before rejecting the save.
    fs::write(
        target_root.join("Master/modoverrides.lua"),
        "failed-import-config",
    )
    .unwrap();
    published.rollback().unwrap();
    assert!(target_root.join("Master/save/session/ORIGINAL").exists());
    assert!(!target_root.join("Master/save/session/SOURCE").exists());
    assert_eq!(
        fs::read_to_string(target_root.join("Master/modoverrides.lua")).unwrap(),
        "original-modoverrides.lua"
    );
    assert!(!fs::read_dir(&root).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".dst-import-")
    }));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dst_import_rejects_world_changes_after_initial_validation() {
    let root = dst_import_test_root("changed-source");
    let source_root = root.join("source");
    let target_root = root.join("target");
    write_valid_dst_shard(&source_root, "Master", "SOURCE");
    write_valid_dst_shard(&target_root, "Master", "ORIGINAL");
    let source = validate_dontstarve_import_source(&source_root).unwrap();
    fs::write(
        source_root.join("Master/save/session/SOURCE/0000000001"),
        "updated source snapshot",
    )
    .unwrap();
    let error = import_dontstarve_world_transaction(&source, &target_root, false).unwrap_err();
    assert!(error.contains("changed during import"), "{error}");
    assert!(target_root.join("Master/save/session/ORIGINAL").exists());
    assert!(!target_root.join("Master/save/session/SOURCE").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dst_import_publish_failure_restores_the_original_world() {
    let root = dst_import_test_root("rollback");
    let source_root = root.join("source");
    let target_root = root.join("target");
    write_valid_dst_shard(&source_root, "Master", "SOURCE");
    write_valid_dst_shard(&target_root, "Master", "TARGET");

    let source = validate_dontstarve_import_source(&source_root).unwrap();
    let error = import_dontstarve_world_transaction_with_forced_publish_failure(
        &source,
        &target_root,
        false,
    )
    .unwrap_err();
    assert!(error.contains("forced publish failure"));
    assert!(
        target_root
            .join("Master/save/session/TARGET/0000000001")
            .is_file()
    );
    assert!(!target_root.join("Master/save/session/SOURCE").exists());
    assert_eq!(
        fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".dst-import-"))
            .count(),
        0
    );

    let _ = fs::remove_dir_all(root);
}

#[cfg(windows)]
#[test]
fn dst_source_resolution_rejects_reparse_points() {
    use std::os::windows::fs::symlink_dir;

    let root = dst_import_test_root("reparse");
    let actual = root.join("actual");
    write_valid_dst_shard(&actual, "Master", "SOURCE");
    let linked = root.join("linked");
    if symlink_dir(&actual, &linked).is_ok() {
        assert_eq!(resolve_dontstarve_cluster_source_root(&linked), None);
    }

    let _ = fs::remove_dir_all(root);
}
