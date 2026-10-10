use super::*;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "dragonwilds_world_fixture.rs"]
mod fixture;
use fixture::*;

#[tokio::test]
async fn public_write_backs_up_original_and_reads_back_without_losing_unknown_data() {
    let fixture = Fixture::new(true).await;
    let original = fs::read(&fixture.world).unwrap();
    let result = write_dragonwilds_world_settings(
        &fixture.paths,
        fixture.input(DragonwildsWorldMode::Custom, &[(BUILDING, 1.5)]),
    )
    .await
    .unwrap();
    assert_eq!(result.values.get(BUILDING), Some(&1.5));
    let bytes = fs::read(&fixture.world).unwrap();
    assert_eq!(
        decode_world_settings(&bytes).unwrap().overrides.get(FUTURE),
        Some(&2.25)
    );
    for marker in [
        b"retain-unknown-info".as_slice(),
        b"retain-unknown-object",
        b"retain-full-world-level-data",
    ] {
        assert!(bytes.windows(marker.len()).any(|v| v == marker));
    }
    let backups = crate::list_instance_backups(&fixture.paths, ID)
        .await
        .unwrap();
    assert_eq!(backups.len(), 1);
    assert_eq!(result.backup_id.as_ref(), Some(&backups[0].backup_id));
    assert_eq!(
        fs::read(Path::new(&backups[0].backup_path).join("saves/Fixture.sav")).unwrap(),
        original
    );
    let reopened = read_dragonwilds_world_settings(&fixture.paths, ID)
        .await
        .unwrap();
    assert_eq!(reopened.revision, result.revision);
    assert_eq!(reopened.values.get(BUILDING), Some(&1.5));
    assert!(reopened.writable);
}

#[tokio::test]
async fn normal_mode_friendly_fire_is_a_real_persisted_edit() {
    let fixture = Fixture::new(false).await;
    let result = write_dragonwilds_world_settings(
        &fixture.paths,
        fixture.input(DragonwildsWorldMode::Normal, &[(FRIENDLY, 1.0)]),
    )
    .await
    .unwrap();
    assert_eq!(result.world_mode, Some(DragonwildsWorldMode::Normal));
    assert_eq!(result.values.get(FRIENDLY), Some(&1.0));
    assert!(result.backup_id.is_some());
    let native = decode_world_settings(&fs::read(&fixture.world).unwrap()).unwrap();
    assert_eq!(native.world_mode, 0);
    assert_eq!(native.overrides.get(FRIENDLY), Some(&1.0));
}

#[tokio::test]
async fn creative_to_custom_preserves_all_effective_rules_without_unlocking_restricted_edits() {
    const MAX_SKILLS: &str = "Difficulty.Progression.AllSkillsMaxed";
    const PASSIVE_AI: &str = "Difficulty.AI.DisableAggressiveAI";
    let fixture = Fixture::new(false).await;
    let creative = patch_world_settings(&world_bytes(), 2, &BTreeMap::new()).unwrap();
    fs::write(&fixture.world, &creative).unwrap();
    let before = read_dragonwilds_world_settings(&fixture.paths, ID)
        .await
        .unwrap();
    assert_eq!(before.values.get(MAX_SKILLS), Some(&1.0));
    assert_eq!(before.values.get(PASSIVE_AI), Some(&1.0));
    let after = write_dragonwilds_world_settings(
        &fixture.paths,
        fixture.input(DragonwildsWorldMode::Custom, &[]),
    )
    .await
    .unwrap();
    assert_eq!(after.world_mode, Some(DragonwildsWorldMode::Custom));
    assert_eq!(after.values.len(), 66);
    for (tag, value) in &before.values {
        // Native gameplay stores these preset values as f32; JSON f64 rounding
        // must not turn equivalent native values into a false discrepancy.
        assert_eq!(
            (*value as f32).to_bits(),
            (after.values[tag] as f32).to_bits(),
            "changed {tag} during conversion"
        );
    }
    let saved = fs::read(&fixture.world).unwrap();
    let native = decode_world_settings(&saved).unwrap();
    assert_eq!(native.overrides.get(MAX_SKILLS), Some(&1.0));
    assert_eq!(native.overrides.get(PASSIVE_AI), Some(&1.0));
    for (tag, value) in [(MAX_SKILLS, 0.0), (PASSIVE_AI, 2.0)] {
        assert!(
            write_dragonwilds_world_settings(
                &fixture.paths,
                fixture.input(DragonwildsWorldMode::Custom, &[(tag, value)])
            )
            .await
            .is_err()
        );
        assert_eq!(fs::read(&fixture.world).unwrap(), saved);
    }
    assert_eq!(
        crate::list_instance_backups(&fixture.paths, ID)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn no_op_does_not_create_a_backup_or_rewrite_world() {
    let fixture = Fixture::new(true).await;
    let original = fs::read(&fixture.world).unwrap();
    let result = write_dragonwilds_world_settings(
        &fixture.paths,
        fixture.input(DragonwildsWorldMode::Custom, &[]),
    )
    .await
    .unwrap();
    assert!(result.backup_id.is_none());
    assert_eq!(fs::read(&fixture.world).unwrap(), original);
    assert!(
        crate::list_instance_backups(&fixture.paths, ID)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn selected_world_changed_after_snapshot_is_not_overwritten() {
    let fixture = Fixture::new(true).await;
    let original = fs::read(&fixture.world).unwrap();
    let input = fixture.input(DragonwildsWorldMode::Custom, &[(BUILDING, 1.5)]);
    let renamed = fixture.saves.join("Selected.sav");
    fs::rename(&fixture.world, &renamed).unwrap();
    assert!(
        write_dragonwilds_world_settings(&fixture.paths, input)
            .await
            .is_err()
    );
    assert_eq!(fs::read(renamed).unwrap(), original);
    assert!(
        crate::list_instance_backups(&fixture.paths, ID)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn backup_failure_and_atomic_write_failure_preserve_existing_save() {
    let fixture = Fixture::new(true).await;
    let original = fs::read(&fixture.world).unwrap();
    let input = fixture.input(DragonwildsWorldMode::Custom, &[(BUILDING, 1.5)]);
    fs::write(fixture.instance.join("backups"), b"not a directory").unwrap();
    assert!(
        write_dragonwilds_world_settings(&fixture.paths, input.clone())
            .await
            .is_err()
    );
    assert_eq!(fs::read(&fixture.world).unwrap(), original);
    fs::remove_file(fixture.instance.join("backups")).unwrap();
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.world);
    assert!(
        write_dragonwilds_world_settings(&fixture.paths, input)
            .await
            .is_err()
    );
    assert_eq!(fs::read(&fixture.world).unwrap(), original);
    assert_eq!(
        crate::list_instance_backups(&fixture.paths, ID)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn wrong_world_and_stale_revision_fail_before_backup_or_write() {
    let fixture = Fixture::new(true).await;
    let original = fs::read(&fixture.world).unwrap();
    let mut input = fixture.input(DragonwildsWorldMode::Custom, &[(BUILDING, 1.5)]);
    input.world_file = "Different.sav".into();
    assert!(
        write_dragonwilds_world_settings(&fixture.paths, input)
            .await
            .is_err()
    );
    let mut input = fixture.input(DragonwildsWorldMode::Custom, &[(BUILDING, 1.5)]);
    input.expected_revision = "stale".into();
    assert!(
        write_dragonwilds_world_settings(&fixture.paths, input)
            .await
            .is_err()
    );
    assert_eq!(fs::read(&fixture.world).unwrap(), original);
    assert!(
        crate::list_instance_backups(&fixture.paths, ID)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn active_run_and_wrong_module_refuse_writes_even_with_valid_world_bytes() {
    let fixture = Fixture::new(true).await;
    let original = fs::read(&fixture.world).unwrap();
    let input = fixture.input(DragonwildsWorldMode::Custom, &[(BUILDING, 1.5)]);
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("INSERT INTO instance_runs(instance_id,pid,status,log_path) VALUES(?1,999999,'running','fixture.log')")
        .bind(ID).execute(&pool).await.unwrap();
    // A stale stopped summary must not override the active-run record.
    assert!(
        write_dragonwilds_world_settings(&fixture.paths, input.clone())
            .await
            .is_err()
    );
    let readonly = read_dragonwilds_world_settings(&fixture.paths, ID)
        .await
        .unwrap();
    assert!(!readonly.writable);
    sqlx::query("DELETE FROM instance_runs WHERE instance_id=?1")
        .bind(ID)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO modules(id,name,version) VALUES('other-game','Other','1')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE instances SET module_id='other-game' WHERE id=?1")
        .bind(ID)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        write_dragonwilds_world_settings(&fixture.paths, input)
            .await
            .is_err()
    );
    pool.close().await;
    assert_eq!(fs::read(&fixture.world).unwrap(), original);
    assert!(!fixture.instance.join("backups").exists());
}

#[tokio::test]
async fn native_catalog_permissions_ranges_and_precision_are_enforced_before_writes() {
    let fixture = Fixture::new(true).await;
    let context = fixture.context();
    let original = fs::read(&fixture.world).unwrap();
    let definitions = definitions().unwrap();
    assert_eq!(
        definitions
            .iter()
            .filter(|d| d.editable_in(DragonwildsWorldMode::Custom))
            .count(),
        63
    );
    assert_eq!(
        definitions
            .iter()
            .filter(|d| d.editable_in(DragonwildsWorldMode::Normal))
            .count(),
        1
    );
    for definition in &definitions {
        for mode in [DragonwildsWorldMode::Normal, DragonwildsWorldMode::Custom] {
            let input = fixture.input(mode, &[(&definition.tag, definition.minimum)]);
            assert_eq!(
                prepare(&context, &input).is_ok(),
                definition.editable_in(mode),
                "{} in {mode:?}",
                definition.tag
            );
        }
    }
    for (tag, value) in [
        (BUILDING, 3.1),
        (BUILDING, -0.1),
        (BUILDING, 0.25),
        (BUILDING, f64::NAN),
        (BUILDING, f64::INFINITY),
        (FRIENDLY, 0.5),
        ("Difficulty.UnknownNewRule", 1.0),
    ] {
        assert!(
            prepare(
                &context,
                &fixture.input(DragonwildsWorldMode::Custom, &[(tag, value)])
            )
            .is_err()
        );
    }
    assert_eq!(fs::read(&fixture.world).unwrap(), original);
    assert!(!fixture.instance.join("backups").exists());
}

#[tokio::test]
async fn prepared_cas_refuses_an_external_save_change() {
    let fixture = Fixture::new(true).await;
    let prepared = prepare(
        &fixture.context(),
        &fixture.input(DragonwildsWorldMode::Custom, &[(BUILDING, 1.5)]),
    )
    .unwrap();
    fs::write(&fixture.world, b"external writer's newer save").unwrap();
    assert!(
        !compare_and_swap_file_atomically(
            &prepared.path,
            &prepared.original,
            &prepared.replacement
        )
        .unwrap()
    );
    assert_eq!(
        fs::read(&fixture.world).unwrap(),
        b"external writer's newer save"
    );
}

#[tokio::test]
async fn ordinary_save_file_and_linked_ancestor_policy_are_enforced() {
    let fixture = Fixture::new(true).await;
    fs::remove_file(&fixture.world).unwrap();
    fs::create_dir(&fixture.world).unwrap();
    assert!(latest_world(&fixture.saves, "Fixture World").is_err());
    assert!(read_world(&fixture.world).is_err());
    fs::remove_dir(&fixture.world).unwrap();
    let outside = fixture.root.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("Outside.sav"), world_bytes()).unwrap();
    let linked = fixture.root.join("linked");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &linked).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let status = std::process::Command::new("cmd")
            .creation_flags(0x08000000)
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&linked)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "junction fixture failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }
    assert!(latest_world(&linked, "Fixture World").is_err());
    assert!(read_world(&linked.join("Outside.sav")).is_err());
    assert_eq!(
        fs::read(outside.join("Outside.sav")).unwrap(),
        world_bytes()
    );
}

#[tokio::test]
async fn native_time_and_world_name_select_the_server_world_independently_of_mtime() {
    use std::time::{Duration, UNIX_EPOCH};
    let fixture = Fixture::new(false).await;
    let native_newer = fixture.saves.join("OlderOnDisk.sav");
    fs::write(
        &native_newer,
        world_bytes_with_metadata("FIXTURE WORLD", SAVED_AT_TICKS + 1000),
    )
    .unwrap();
    for (path, seconds) in [(&fixture.world, 2000), (&native_newer, 1000)] {
        fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(seconds)))
            .unwrap();
    }
    assert_eq!(
        latest_world(&fixture.saves, "Fixture World").unwrap(),
        Some(native_newer.clone())
    );
    let unrelated = fixture.saves.join("UnrelatedNewer.sav");
    fs::write(
        &unrelated,
        world_bytes_with_metadata("Another World", SAVED_AT_TICKS + 2000),
    )
    .unwrap();
    assert_eq!(
        latest_world(&fixture.saves, "Fixture World").unwrap(),
        Some(native_newer)
    );
    fs::write(
        fixture.saves.join("EqualLatest.sav"),
        world_bytes_with_metadata("Fixture World", SAVED_AT_TICKS + 1000),
    )
    .unwrap();
    assert!(latest_world(&fixture.saves, "Fixture World").is_err());
}

#[tokio::test]
#[ignore = "requires explicit isolated native fixture and a new output path outside the repository"]
async fn native_public_write_produces_a_server_reload_candidate_with_verified_backup() {
    let source = PathBuf::from(
        std::env::var_os("LGSM_DRAGONWILDS_SAVE_FIXTURE")
            .expect("supply the stopped isolated native golden save"),
    );
    let output = PathBuf::from(
        std::env::var_os("LGSM_DRAGONWILDS_SAVE_OUTPUT")
            .expect("supply a new absolute output path outside the repository"),
    );
    assert!(
        output.is_absolute()
            && !output
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
    );
    assert!(
        !output.try_exists().expect("inspect explicit output path"),
        "refuse to overwrite an existing output"
    );
    let parent = output.parent().expect("output must have a parent");
    let _output_guards =
        guard_directories(parent).expect("output ancestors must be ordinary directories");
    let repository =
        fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    assert!(
        !fs::canonicalize(parent).unwrap().starts_with(repository),
        "native output must remain outside the repository"
    );

    let golden = fs::read(&source).expect("read explicitly supplied isolated native fixture");
    let native = decode_world_settings(&golden).expect("decode actual native autosave");
    assert_eq!(native.world_mode, 3);
    assert_eq!(native.overrides.get(BUILDING), Some(&1.5));
    let fixture = Fixture::new(false).await;
    fs::write(&fixture.world, &golden).unwrap();
    let config = fixture.instance.join("config/instance.json");
    let mut settings: serde_json::Value =
        serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    settings["settings"]["default_world_name"] =
        serde_json::Value::String(native.world_name.clone());
    fs::write(&config, serde_json::to_vec(&settings).unwrap()).unwrap();

    let before = read_dragonwilds_world_settings(&fixture.paths, ID)
        .await
        .unwrap();
    assert_eq!(before.values.get(BUILDING), Some(&1.5));
    assert_eq!(
        before.world_name.as_deref(),
        Some(native.world_name.as_str())
    );
    assert!(before.writable);
    let changed = write_dragonwilds_world_settings(
        &fixture.paths,
        fixture.input(DragonwildsWorldMode::Custom, &[(BUILDING, 2.0)]),
    )
    .await
    .unwrap();
    assert_eq!(changed.values.get(BUILDING), Some(&2.0));
    let actual = fs::read(&fixture.world).unwrap();
    let reopened = read_dragonwilds_world_settings(&fixture.paths, ID)
        .await
        .unwrap();
    assert_eq!(reopened.values.get(BUILDING), Some(&2.0));
    assert_eq!(reopened.revision.as_deref(), Some(sha256(&actual).as_str()));
    assert_eq!(
        decode_world_settings(&actual)
            .unwrap()
            .overrides
            .get(BUILDING),
        Some(&2.0)
    );
    let backups = crate::list_instance_backups(&fixture.paths, ID)
        .await
        .unwrap();
    assert_eq!(backups.len(), 1);
    assert_eq!(changed.backup_id.as_ref(), Some(&backups[0].backup_id));
    assert_eq!(
        fs::read(Path::new(&backups[0].backup_path).join("saves/Fixture.sav")).unwrap(),
        golden
    );
    // The fixture is private test data. Only the caller-authorized new artifact
    // leaves it; neither the supplied golden nor any running server is modified.
    crate::atomic_file::create_file_atomically(&output, &actual)
        .expect("publish complete new native reload candidate");
    assert_eq!(fs::read(&output).unwrap(), actual);
    assert_eq!(fs::read(&source).unwrap(), golden);
}
