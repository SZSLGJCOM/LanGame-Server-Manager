use super::*;
use serde_json::json;
use std::fs;

struct Fixture {
    root: PathBuf,
    native: PathBuf,
    managed: PathBuf,
}

impl Fixture {
    fn new(native: &[u8], managed: &[u8]) -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-enshrouded-bans-{}", uuid::Uuid::new_v4()));
        let native_path = root.join("runtime").join(CONFIG_FILE);
        let managed_path = root.join("config").join(CONFIG_FILE);
        fs::create_dir_all(native_path.parent().unwrap()).unwrap();
        fs::create_dir_all(managed_path.parent().unwrap()).unwrap();
        fs::write(&native_path, native).unwrap();
        fs::write(&managed_path, managed).unwrap();
        Self {
            root,
            native: native_path,
            managed: managed_path,
        }
    }

    fn snapshot(&self) -> Result<EnshroudedBansSnapshot, StorageError> {
        EnshroudedBansSnapshot::read(
            "enshrouded",
            self.native.parent().unwrap(),
            self.managed.parent().unwrap(),
        )
        .map(Option::unwrap)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        if let Err(error) = fs::remove_dir_all(&self.root) {
            eprintln!(
                "could not remove owned Enshrouded ban fixture {}: {error}",
                self.root.display()
            );
        }
    }
}

#[test]
fn alias_migration_has_one_canonical_key_and_exact_recoverable_originals() {
    let native = br#"{"bans":[],"futureRoot":{"native":true},"gameSettings":{"futureNative":7}}"#;
    let managed = br#"{"bans":[],"futureManaged":true}"#;
    let fixture = Fixture::new(native, managed);
    let snapshot = fixture.snapshot().unwrap();
    snapshot.verify_unchanged("fixture").unwrap();
    let mut files = ManagedConfigMutation::new("enshrouded");
    files.write(&fixture.managed,
        br#"{"bannedAccounts":[],"gameSettings":{"futureRendered":8,"bans":{"keepNested":true}}}"#).unwrap();
    snapshot
        .materialize(
            &fixture.managed,
            json!({"banned_player_ids":""}).as_object().unwrap(),
            &mut files,
        )
        .unwrap();
    files.commit();
    let after: Value = serde_json::from_slice(&fs::read(&fixture.native).unwrap()).unwrap();
    assert_eq!(after["bannedAccounts"], json!([]));
    assert!(after.get("bans").is_none());
    assert_eq!(after["futureRoot"]["native"], true);
    assert_eq!(after["gameSettings"]["futureNative"], 7);
    assert_eq!(after["gameSettings"]["futureRendered"], 8);
    assert_eq!(after["gameSettings"]["bans"]["keepNested"], true);
    let recovery = fs::read_dir(
        fixture
            .managed
            .parent()
            .unwrap()
            .join("native-ban-recovery"),
    )
    .unwrap()
    .next()
    .unwrap()
    .unwrap()
    .path();
    assert_eq!(
        fs::read(recovery.join("native-before.json")).unwrap(),
        native
    );
    assert_eq!(
        fs::read(recovery.join("managed-before.json")).unwrap(),
        managed
    );
}

#[test]
fn invalid_native_document_does_not_change_either_original() {
    let fixture = Fixture::new(b"{invalid", br#"{"bans":[]}"#);
    assert!(fixture.snapshot().is_err());
    assert_eq!(fs::read(&fixture.native).unwrap(), b"{invalid");
    assert_eq!(fs::read(&fixture.managed).unwrap(), br#"{"bans":[]}"#);
    assert!(
        !fixture
            .managed
            .parent()
            .unwrap()
            .join("native-ban-recovery")
            .exists()
    );
}

#[test]
fn external_native_change_is_rejected_before_recovery_or_native_writes() {
    let fixture = Fixture::new(br#"{"bans":[]}"#, br#"{"bannedAccounts":[]}"#);
    let snapshot = fixture.snapshot().unwrap();
    let external = br#"{"bannedAccounts":[],"external":true}"#;
    fs::write(&fixture.native, external).unwrap();
    assert!(snapshot.verify_unchanged("fixture").is_err());
    let mut files = ManagedConfigMutation::new("enshrouded");
    assert!(
        snapshot
            .materialize(&fixture.managed, &Map::new(), &mut files)
            .is_err()
    );
    files.rollback().unwrap();
    assert_eq!(fs::read(&fixture.native).unwrap(), external);
    assert!(
        !fixture
            .managed
            .parent()
            .unwrap()
            .join("native-ban-recovery")
            .exists()
    );
}

#[test]
fn native_write_failure_can_roll_back_managed_configuration_and_recovery_files() {
    let native = br#"{"bans":[]}"#;
    let managed = br#"{"bans":[],"before":true}"#;
    let fixture = Fixture::new(native, managed);
    let snapshot = fixture.snapshot().unwrap();
    snapshot.verify_unchanged("fixture").unwrap();
    let mut files = ManagedConfigMutation::new("enshrouded");
    files
        .write(&fixture.managed, br#"{"bannedAccounts":[]}"#)
        .unwrap();
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.native);
    assert!(
        snapshot
            .materialize(&fixture.managed, &Map::new(), &mut files)
            .is_err()
    );
    files.rollback().unwrap();
    assert_eq!(fs::read(&fixture.native).unwrap(), native);
    assert_eq!(fs::read(&fixture.managed).unwrap(), managed);
    let recovery = fs::read_dir(
        fixture
            .managed
            .parent()
            .unwrap()
            .join("native-ban-recovery"),
    )
    .unwrap()
    .next()
    .unwrap()
    .unwrap()
    .path();
    assert_eq!(fs::read_dir(recovery).unwrap().count(), 0);
}

#[test]
fn unsupported_or_conflicting_native_records_preserve_both_files_without_recovery_writes() {
    let cases: &[(&str, &[u8], &[u8])] = &[
        ("non-array roster", br#"{"bannedAccounts":{}}"#, br#"{"bans":[]}"#),
        ("scalar record", br#"{"bannedAccounts":[1]}"#, br#"{"bans":[]}"#),
        ("string account id", br#"{"bannedAccounts":[{"accountId":"1"}]}"#, br#"{"bans":[]}"#),
        ("oversized account id", br#"{"bannedAccounts":[{"accountId":18446744073709551616}]}"#, br#"{"bans":[]}"#),
        ("fractional account id", br#"{"bannedAccounts":[{"accountId":1.5}]}"#, br#"{"bans":[]}"#),
        ("floating account id", br#"{"bannedAccounts":[{"accountId":1.0}]}"#, br#"{"bans":[]}"#),
        ("scalar date", br#"{"bannedAccounts":[{"accountId":1,"banDate":123}]}"#, br#"{"bans":[]}"#),
        ("null date", br#"{"bannedAccounts":[{"accountId":1,"banDate":null}]}"#, br#"{"bans":[]}"#),
        ("string date value", br#"{"bannedAccounts":[{"accountId":1,"banDate":{"value":"0"}}]}"#, br#"{"bans":[]}"#),
        ("oversized date value", br#"{"bannedAccounts":[{"accountId":1,"banDate":{"value":9223372036854775808}}]}"#, br#"{"bans":[]}"#),
        ("conflicting duplicate", br#"{"bannedAccounts":[{"accountId":1,"future":{"mode":"a"}},{"accountId":1,"future":{"mode":"b"}}]}"#, br#"{"bans":[]}"#),
        ("conflict between files", br#"{"bannedAccounts":[{"accountId":1,"displayName":"native"}]}"#, br#"{"bans":[{"accountId":1,"displayName":"managed"}]}"#),
    ];
    for (name, native, managed) in cases {
        let fixture = Fixture::new(native, managed);
        assert!(fixture.snapshot().is_err(), "case must be rejected: {name}");
        assert_eq!(
            fs::read(&fixture.native).unwrap(),
            *native,
            "native changed: {name}"
        );
        assert_eq!(
            fs::read(&fixture.managed).unwrap(),
            *managed,
            "managed changed: {name}"
        );
        assert!(
            !fixture
                .managed
                .parent()
                .unwrap()
                .join("native-ban-recovery")
                .exists(),
            "recovery output was written before validation: {name}"
        );
    }
}

#[test]
fn completely_identical_records_are_deduplicated_across_arrays_and_files() {
    let entry = json!({"accountId":1,"displayName":"native","future":{"retain":true}});
    let native =
        serde_json::to_vec(&json!({"bannedAccounts":[entry,entry],"bans":[entry]})).unwrap();
    let managed = serde_json::to_vec(&json!({"bans":[entry,entry]})).unwrap();
    let fixture = Fixture::new(&native, &managed);
    let snapshot = fixture.snapshot().unwrap();
    assert_eq!(snapshot.records.len(), 1);
    let mut settings = Map::new();
    snapshot.project(&mut settings).unwrap();
    assert_eq!(settings[roster::SETTINGS_KEY], "1");
    snapshot.verify_unchanged("fixture").unwrap();
    let mut files = ManagedConfigMutation::new("enshrouded");
    snapshot
        .materialize(&fixture.managed, &settings, &mut files)
        .unwrap();
    files.commit();
    let after: Value = serde_json::from_slice(&fs::read(&fixture.native).unwrap()).unwrap();
    assert_eq!(after["bannedAccounts"], json!([entry]));
    assert!(after.get("bans").is_none());
}

#[test]
fn an_external_source_change_after_noop_render_is_not_adopted() {
    let original = br#"{"bannedAccounts":[]}"#;
    let fixture = Fixture::new(original, original);
    let snapshot = fixture.snapshot().unwrap();
    let mut files = ManagedConfigMutation::new("enshrouded");
    files.write(&fixture.managed, original).unwrap();
    let external = br#"{"bannedAccounts":[{"accountId":1}]}"#;
    fs::write(&fixture.managed, external).unwrap();
    assert!(
        snapshot
            .materialize(&fixture.managed, &Map::new(), &mut files)
            .is_err()
    );
    files.rollback().unwrap();
    assert_eq!(fs::read(&fixture.native).unwrap(), original);
    assert_eq!(fs::read(&fixture.managed).unwrap(), external);
    assert!(
        !fixture
            .managed
            .parent()
            .unwrap()
            .join("native-ban-recovery")
            .exists()
    );
}
