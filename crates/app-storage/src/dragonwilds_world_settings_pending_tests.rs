use super::*;
use crate::{DragonwildsBootstrap, prepare_dragonwilds_bootstrap};

const CONFIG: &str = "runtime/RSDragonwilds/Saved/Config/WindowsServer/DedicatedServer.ini";
const PENDING: &str = "config/dragonwilds-world-settings.pending.json";
const JOURNAL: &str = "config/.langame-dragonwilds-bootstrap.json";

async fn fresh() -> Fixture {
    let fixture = Fixture::new(false).await;
    fs::remove_file(&fixture.world).unwrap();
    let config = fixture.instance.join(CONFIG);
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(config, b"[/Script/Dominion.DedicatedServerSettings]\r\nDefaultWorldName=Fixture World\r\nWorldPassword=\r\nUnknownNative=keep\r\n").unwrap();
    fixture
}

async fn save_draft(
    fixture: &Fixture,
    mode: DragonwildsWorldMode,
    values: &[(&str, f64)],
) -> DragonwildsWorldSettingsSnapshot {
    let snapshot = read_dragonwilds_world_settings(&fixture.paths, ID)
        .await
        .unwrap();
    write_dragonwilds_world_settings(
        &fixture.paths,
        WriteDragonwildsWorldSettingsInput {
            instance_id: ID.into(),
            world_file: None,
            expected_revision: snapshot.revision.unwrap(),
            world_mode: mode,
            values: values
                .iter()
                .map(|(tag, value)| ((*tag).to_owned(), *value))
                .collect(),
        },
    )
    .await
    .unwrap()
}

async fn staged(fixture: &Fixture) -> DragonwildsBootstrap {
    save_draft(fixture, DragonwildsWorldMode::Custom, &[(BUILDING, 1.5)]).await;
    prepare_dragonwilds_bootstrap(&fixture.paths, ID)
        .await
        .unwrap()
        .unwrap()
}

fn generate(fixture: &Fixture, session: &DragonwildsBootstrap) {
    fs::write(&fixture.world, world_bytes()).unwrap();
    assert!(session.inspect_native_state().unwrap().world_ready);
}

fn assert_password_restored(fixture: &Fixture) {
    let config = fs::read_to_string(fixture.instance.join(CONFIG)).unwrap();
    assert!(
        config.contains("WorldPassword=\r\n"),
        "temporary password was not restored"
    );
}

#[tokio::test]
async fn new_world_draft_round_trip_preserves_preset_conversion_without_creating_a_save() {
    let fixture = fresh().await;
    let initial = read_dragonwilds_world_settings(&fixture.paths, ID)
        .await
        .unwrap();
    assert_eq!(initial.status, "new");
    assert!(initial.writable);
    assert!(initial.world_file.is_none());
    assert_eq!(initial.world_mode, Some(DragonwildsWorldMode::Normal));
    assert_eq!(initial.values.len(), 66);
    assert!(initial.revision.is_some());
    let creative = save_draft(&fixture, DragonwildsWorldMode::Creative, &[]).await;
    let custom = save_draft(&fixture, DragonwildsWorldMode::Custom, &[(BUILDING, 1.5)]).await;
    assert_ne!(initial.revision, custom.revision);
    for (tag, value) in &creative.values {
        if tag != BUILDING {
            assert_eq!(
                (*value as f32).to_bits(),
                (custom.values[tag] as f32).to_bits(),
                "{tag}"
            );
        }
    }
    let reopened = read_dragonwilds_world_settings(&fixture.paths, ID)
        .await
        .unwrap();
    assert_eq!(reopened.revision, custom.revision);
    assert_eq!(reopened.values.get(BUILDING), Some(&1.5));
    assert!(!fixture.world.exists());
    assert!(
        crate::list_instance_backups(&fixture.paths, ID)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn draft_revision_survives_basic_world_rename_but_not_another_rule_save() {
    let fixture = fresh().await;
    let first = save_draft(&fixture, DragonwildsWorldMode::Hard, &[]).await;
    let config_path = fixture.instance.join("config/instance.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["settings"]["default_world_name"] = "Renamed".into();
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let renamed = read_dragonwilds_world_settings(&fixture.paths, ID)
        .await
        .unwrap();
    assert_eq!(renamed.world_name.as_deref(), Some("Renamed"));
    assert_eq!(renamed.revision, first.revision);
    let input = WriteDragonwildsWorldSettingsInput {
        instance_id: ID.into(),
        world_file: None,
        expected_revision: first.revision.unwrap(),
        world_mode: DragonwildsWorldMode::Custom,
        values: BTreeMap::from([(BUILDING.into(), 1.5)]),
    };
    write_dragonwilds_world_settings(&fixture.paths, input.clone())
        .await
        .unwrap();
    assert!(
        write_dragonwilds_world_settings(&fixture.paths, input)
            .await
            .is_err()
    );
    let ini = fs::read_to_string(fixture.instance.join(CONFIG))
        .unwrap()
        .replace("DefaultWorldName=Fixture World", "DefaultWorldName=Renamed");
    fs::write(fixture.instance.join(CONFIG), ini).unwrap();
    let session = prepare_dragonwilds_bootstrap(&fixture.paths, ID)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(session.world_name(), "Renamed");
    session.abort_after_stopped().await.unwrap();
}

#[tokio::test]
async fn new_world_rule_validation_and_instance_lock_preserve_the_saved_draft() {
    let fixture = fresh().await;
    let snapshot = save_draft(&fixture, DragonwildsWorldMode::Custom, &[(BUILDING, 1.5)]).await;
    let original = fs::read(fixture.instance.join(PENDING)).unwrap();
    for (tag, value) in [
        (BUILDING, 999.0),
        ("Difficulty.Player.NoBuildingStability", 1.0),
        ("Difficulty.Progression.AllSkillsMaxed", 1.0),
    ] {
        let input = WriteDragonwildsWorldSettingsInput {
            instance_id: ID.into(),
            world_file: None,
            expected_revision: snapshot.revision.clone().unwrap(),
            world_mode: DragonwildsWorldMode::Custom,
            values: BTreeMap::from([(tag.into(), value)]),
        };
        assert!(
            write_dragonwilds_world_settings(&fixture.paths, input)
                .await
                .is_err()
        );
        assert_eq!(fs::read(fixture.instance.join(PENDING)).unwrap(), original);
    }
    let session = prepare_dragonwilds_bootstrap(&fixture.paths, ID)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !read_dragonwilds_world_settings(&fixture.paths, ID)
            .await
            .unwrap()
            .writable
    );
    assert!(
        prepare_dragonwilds_bootstrap(&fixture.paths, ID)
            .await
            .is_err()
    );
    session.abort_after_stopped().await.unwrap();
    assert_password_restored(&fixture);
    assert_eq!(fs::read(fixture.instance.join(PENDING)).unwrap(), original);
}

#[tokio::test]
async fn default_native_start_is_unchanged_and_imported_worlds_are_never_claimed() {
    let fixture = fresh().await;
    assert!(
        prepare_dragonwilds_bootstrap(&fixture.paths, ID)
            .await
            .unwrap()
            .is_none()
    );
    save_draft(&fixture, DragonwildsWorldMode::Hard, &[]).await;
    let imported = world_bytes_with_metadata("Different", SAVED_AT_TICKS);
    fs::write(&fixture.world, &imported).unwrap();
    assert!(
        prepare_dragonwilds_bootstrap(&fixture.paths, ID)
            .await
            .is_err()
    );
    assert!(
        read_dragonwilds_world_settings(&fixture.paths, ID)
            .await
            .is_err()
    );
    assert_eq!(fs::read(&fixture.world).unwrap(), imported);
    assert!(!fixture.instance.join(JOURNAL).exists());
    assert_password_restored(&fixture);
}

#[tokio::test]
async fn finish_backs_up_generated_world_and_preserves_native_ini_additions() {
    let fixture = fresh().await;
    let session = staged(&fixture).await;
    let temporary = fs::read(fixture.instance.join(CONFIG)).unwrap();
    assert!(!String::from_utf8_lossy(&temporary).contains("WorldPassword=\r\n"));
    assert!(!session.inspect_native_state().unwrap().world_ready);
    generate(&fixture, &session);
    let original = fs::read(&fixture.world).unwrap();
    let mut config = temporary;
    config.extend_from_slice(b"ServerGuid=native-stable-guid\r\nbAllowSendingCrashDumps=True\r\n");
    fs::write(fixture.instance.join(CONFIG), config).unwrap();
    session.finish_after_stopped().await.unwrap();
    assert_password_restored(&fixture);
    let config = fs::read_to_string(fixture.instance.join(CONFIG)).unwrap();
    assert!(config.contains("ServerGuid=native-stable-guid"));
    assert!(config.contains("UnknownNative=keep"));
    let saved = read_dragonwilds_world_settings(&fixture.paths, ID)
        .await
        .unwrap();
    assert_eq!(saved.status, "ready");
    assert_eq!(saved.values.get(BUILDING), Some(&1.5));
    assert!(!fixture.instance.join(PENDING).exists());
    assert!(!fixture.instance.join(JOURNAL).exists());
    let backups = crate::list_instance_backups(&fixture.paths, ID)
        .await
        .unwrap();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        fs::read(Path::new(&backups[0].backup_path).join("saves/Fixture.sav")).unwrap(),
        original
    );
}

#[tokio::test]
async fn unobserved_world_after_a_crash_is_preserved_but_cannot_be_claimed() {
    let fixture = fresh().await;
    let session = staged(&fixture).await;
    fs::write(&fixture.world, world_bytes()).unwrap();
    drop(session);
    assert!(
        prepare_dragonwilds_bootstrap(&fixture.paths, ID)
            .await
            .err()
            .unwrap()
            .to_string()
            .contains("before its identity was recorded")
    );
    assert_eq!(fs::read(&fixture.world).unwrap(), world_bytes());
    assert!(fixture.instance.join(PENDING).exists());
    assert!(fixture.instance.join(JOURNAL).exists());
}

#[tokio::test]
async fn observed_world_resumes_after_native_exit_without_another_generation() {
    let fixture = fresh().await;
    let session = staged(&fixture).await;
    generate(&fixture, &session);
    session.abort_after_stopped().await.unwrap();
    assert_password_restored(&fixture);
    let session = prepare_dragonwilds_bootstrap(&fixture.paths, ID)
        .await
        .unwrap()
        .unwrap();
    assert!(session.inspect_native_state().unwrap().world_ready);
    session.finish_after_stopped().await.unwrap();
    assert_eq!(
        read_dragonwilds_world_settings(&fixture.paths, ID)
            .await
            .unwrap()
            .values
            .get(BUILDING),
        Some(&1.5)
    );
}

#[tokio::test]
async fn backup_failure_restores_password_and_keeps_generated_world_and_draft() {
    let fixture = fresh().await;
    let session = staged(&fixture).await;
    generate(&fixture, &session);
    fs::write(fixture.instance.join("backups"), b"block backup directory").unwrap();
    assert!(session.finish_after_stopped().await.is_err());
    assert_password_restored(&fixture);
    assert_eq!(fs::read(&fixture.world).unwrap(), world_bytes());
    assert!(fixture.instance.join(PENDING).exists());
    assert!(fixture.instance.join(JOURNAL).exists());
}

#[tokio::test]
async fn failed_world_commit_reuses_verified_backup_and_exact_result_on_retry() {
    let fixture = fresh().await;
    let session = staged(&fixture).await;
    generate(&fixture, &session);
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.world);
    assert!(session.finish_after_stopped().await.is_err());
    assert_password_restored(&fixture);
    assert_eq!(fs::read(&fixture.world).unwrap(), world_bytes());
    assert_eq!(
        crate::list_instance_backups(&fixture.paths, ID)
            .await
            .unwrap()
            .len(),
        1
    );
    let session = prepare_dragonwilds_bootstrap(&fixture.paths, ID)
        .await
        .unwrap()
        .unwrap();
    assert!(session.inspect_native_state().unwrap().world_ready);
    session.finish_after_stopped().await.unwrap();
    assert_eq!(
        crate::list_instance_backups(&fixture.paths, ID)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        read_dragonwilds_world_settings(&fixture.paths, ID)
            .await
            .unwrap()
            .values
            .get(BUILDING),
        Some(&1.5)
    );
}

#[tokio::test]
async fn applied_world_with_uncleared_journal_blocks_edits_and_recovers_after_pending_removal() {
    let fixture = fresh().await;
    let session = staged(&fixture).await;
    generate(&fixture, &session);
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.world);
    assert!(session.finish_after_stopped().await.is_err());
    let draft: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.instance.join(PENDING)).unwrap()).unwrap();
    let overrides: BTreeMap<String, f32> =
        serde_json::from_value(draft["overrides"].clone()).unwrap();
    let replacement = patch_world_settings(&world_bytes(), 3, &overrides).unwrap();
    fs::write(&fixture.world, &replacement).unwrap();
    fs::remove_file(fixture.instance.join(PENDING)).unwrap();
    assert!(
        read_dragonwilds_world_settings(&fixture.paths, ID)
            .await
            .is_err()
    );
    assert!(
        write_dragonwilds_world_settings(
            &fixture.paths,
            fixture.input(DragonwildsWorldMode::Custom, &[(BUILDING, 2.0)])
        )
        .await
        .is_err()
    );
    let session = prepare_dragonwilds_bootstrap(&fixture.paths, ID)
        .await
        .unwrap()
        .unwrap();
    assert!(session.inspect_native_state().unwrap().world_ready);
    session.finish_after_stopped().await.unwrap();
    assert_eq!(fs::read(&fixture.world).unwrap(), replacement);
    assert_eq!(
        crate::list_instance_backups(&fixture.paths, ID)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(!fixture.instance.join(JOURNAL).exists());
}

#[tokio::test]
async fn externally_changed_password_is_not_overwritten_during_recovery() {
    let fixture = fresh().await;
    let session = staged(&fixture).await;
    generate(&fixture, &session);
    let config = fs::read_to_string(fixture.instance.join(CONFIG)).unwrap();
    let altered = config
        .lines()
        .map(|line| {
            if line.starts_with("WorldPassword=") {
                "WorldPassword=fixture-server-password"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\r\n")
        + "\r\n";
    fs::write(fixture.instance.join(CONFIG), &altered).unwrap();
    assert!(session.abort_after_stopped().await.is_err());
    assert_eq!(
        fs::read_to_string(fixture.instance.join(CONFIG)).unwrap(),
        altered
    );
    assert_eq!(fs::read(&fixture.world).unwrap(), world_bytes());
    assert!(fixture.instance.join(JOURNAL).exists());
}

#[tokio::test]
async fn observed_world_cannot_be_regenerated_after_its_save_disappears() {
    let fixture = fresh().await;
    let session = staged(&fixture).await;
    generate(&fixture, &session);
    session.abort_after_stopped().await.unwrap();
    fs::rename(&fixture.world, fixture.saves.join("preserved-original.bin")).unwrap();
    assert!(
        prepare_dragonwilds_bootstrap(&fixture.paths, ID)
            .await
            .err()
            .unwrap()
            .to_string()
            .contains("previously recorded world is missing")
    );
    assert_password_restored(&fixture);
    assert_eq!(
        fs::read(fixture.saves.join("preserved-original.bin")).unwrap(),
        world_bytes()
    );
}

#[tokio::test]
async fn native_selection_ignores_old_timestamp_ties_but_rejects_tied_latest_saves() {
    let fixture = fresh().await;
    let session = staged(&fixture).await;
    fs::write(fixture.saves.join("A.sav"), world_bytes()).unwrap();
    fs::write(fixture.saves.join("B.sav"), world_bytes()).unwrap();
    fs::write(
        &fixture.world,
        world_bytes_with_metadata("Fixture World", SAVED_AT_TICKS + 1),
    )
    .unwrap();
    assert!(session.inspect_native_state().unwrap().world_ready);
    fs::write(
        fixture.saves.join("TiedNewest.sav"),
        world_bytes_with_metadata("Fixture World", SAVED_AT_TICKS + 1),
    )
    .unwrap();
    assert!(session.inspect_native_state().is_err());
    session.abort_after_stopped().await.unwrap();
    assert_eq!(
        fs::read(fixture.saves.join("A.sav")).unwrap(),
        world_bytes()
    );
}

#[tokio::test]
async fn failed_guid_publication_is_not_acknowledged_until_retry_persists_it() {
    let fixture = fresh().await;
    let session = staged(&fixture).await;
    fs::write(&fixture.world, world_bytes()).unwrap();
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.instance.join(JOURNAL));
    assert!(session.inspect_native_state().is_err());
    let before: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.instance.join(JOURNAL)).unwrap()).unwrap();
    assert!(before["observed_guid"].is_null());
    assert!(session.inspect_native_state().unwrap().world_ready);
    let after: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.instance.join(JOURNAL)).unwrap()).unwrap();
    assert_eq!(after["observed_guid"], serde_json::json!([11, 22, 33, 44]));
    session.abort_after_stopped().await.unwrap();
}

#[tokio::test]
async fn chinese_world_and_quoted_password_round_trip_through_native_initialization() {
    let fixture = fresh().await;
    let config_path = fixture.instance.join("config/instance.json");
    let mut settings: serde_json::Value =
        serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    settings["settings"]["default_world_name"] = "青岚世界".into();
    settings["settings"]["world_password"] = "  \"fixture-password\"  ".into();
    fs::write(config_path, serde_json::to_vec(&settings).unwrap()).unwrap();
    let original = "\u{feff}[/Script/Dominion.DedicatedServerSettings]\r\nDefaultWorldName=青岚世界\r\nWorldPassword=  \"fixture-password\"  \r\nUnknownNative=keep\r\n";
    fs::write(fixture.instance.join(CONFIG), original).unwrap();
    let session = staged(&fixture).await;
    fs::write(
        &fixture.world,
        world_bytes_with_metadata("青岚世界", SAVED_AT_TICKS),
    )
    .unwrap();
    assert!(session.inspect_native_state().unwrap().world_ready);
    let mut current = fs::read(fixture.instance.join(CONFIG)).unwrap();
    current.extend_from_slice(b"ServerGuid=generated-identity\r\n");
    fs::write(fixture.instance.join(CONFIG), current).unwrap();
    session.finish_after_stopped().await.unwrap();
    let restored = fs::read_to_string(fixture.instance.join(CONFIG)).unwrap();
    assert!(restored.starts_with(original));
    assert!(restored.ends_with("ServerGuid=generated-identity\r\n"));
    assert_eq!(
        read_dragonwilds_world_settings(&fixture.paths, ID)
            .await
            .unwrap()
            .world_name
            .as_deref(),
        Some("青岚世界")
    );
}
