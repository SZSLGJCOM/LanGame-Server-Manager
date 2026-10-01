use super::*;
use serde_json::json;

struct Fixture {
    root: PathBuf,
    before: Vec<u8>,
    after: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-collection-removal-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(root.join("config")).unwrap();
        let runtime = root.join("runtime");
        fs::create_dir_all(runtime.join("SquadGame/Plugins/Mods/222222")).unwrap();
        fs::create_dir_all(runtime.join("SquadGame/Plugins/Mods/333333")).unwrap();
        fs::write(runtime.join(".langame-private-runtime"), b"managed\n").unwrap();
        fs::write(
            runtime.join("SquadGame/Plugins/Mods/222222/payload.pak"),
            b"selected package",
        )
        .unwrap();
        fs::write(
            runtime.join("SquadGame/Plugins/Mods/333333/payload.pak"),
            b"preserved package",
        )
        .unwrap();
        let document = json!({"instance_id": "fixture", "module_id": "squad", "ports": [],
            "settings": {"secret": "synthetic", "steam_workshop_collections": [{"id":"111111", "title":"One", "member_ids":["222222"]}]}});
        let before = serde_json::to_vec_pretty(&document).unwrap();
        let mut replacement = document;
        replacement["settings"]["steam_workshop_collections"] = json!([]);
        let after = serde_json::to_vec_pretty(&replacement).unwrap();
        fs::write(root.join("config/instance.json"), &before).unwrap();
        Self {
            root,
            before,
            after,
        }
    }
    fn source(&self) -> PathBuf {
        self.root.join("runtime/SquadGame/Plugins/Mods/222222")
    }
    fn stage(&self) -> PathBuf {
        let journal = Journal {
            version: 1,
            instance_id: "fixture".into(),
            operation_id: uuid::Uuid::new_v4().to_string(),
            original_sha256: digest(&self.before),
            replacement_sha256: digest(&self.after),
            members: vec!["222222".into()],
        };
        let operation = self.root.join(RETAINED).join(&journal.operation_id);
        fs::create_dir_all(&operation).unwrap();
        let bytes = serde_json::to_vec(&journal).unwrap();
        fs::write(operation.join("owner.json"), &bytes).unwrap();
        fs::write(self.root.join(JOURNAL), &bytes).unwrap();
        operation
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn removes_from_loading_directory_and_retains_bytes_without_touching_peer() {
    let f = Fixture::new();
    let old: Value = serde_json::from_slice(&f.before).unwrap();
    let new: Value = serde_json::from_slice(&f.after).unwrap();
    remove(
        &f.root,
        &f.root.join("runtime"),
        "fixture",
        &old["settings"],
        &new["settings"],
        vec!["222222".into()],
    )
    .unwrap();
    assert!(!f.source().exists());
    let operation = fs::read_dir(f.root.join(RETAINED))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        fs::read(operation.join("222222/payload.pak")).unwrap(),
        b"selected package"
    );
    assert_eq!(
        fs::read(
            f.root
                .join("runtime/SquadGame/Plugins/Mods/333333/payload.pak")
        )
        .unwrap(),
        b"preserved package"
    );
    assert_eq!(
        fs::read(f.root.join("config/instance.json")).unwrap(),
        f.after
    );
    assert!(!f.root.join(JOURNAL).exists());
}

#[test]
fn interrupted_precommit_rolls_back_and_postcommit_completes_without_reenabling() {
    for committed in [false, true] {
        let f = Fixture::new();
        let operation = f.stage();
        fs::rename(f.source(), operation.join("222222")).unwrap();
        if committed {
            fs::write(f.root.join("config/instance.json"), &f.after).unwrap();
        }
        assert!(ensure_no_pending(&f.root).is_err());
        recover(&f.root, false).unwrap();
        assert_eq!(f.source().exists(), !committed);
        assert_eq!(operation.join("222222/payload.pak").exists(), committed);
        assert!(!f.root.join(JOURNAL).exists());
        assert_eq!(
            fs::read(f.root.join("config/instance.json")).unwrap(),
            if committed {
                f.after.clone()
            } else {
                f.before.clone()
            }
        );
    }
}

#[test]
fn failed_config_publication_restores_all_moved_payloads() {
    for retain_collection in [false, true] {
        let f = Fixture::new();
        let old: Value = serde_json::from_slice(&f.before).unwrap();
        let new: Value = serde_json::from_slice(&f.after).unwrap();
        crate::atomic_file::fail_next_atomic_write_for_test(&f.root.join("config/instance.json"));
        assert!(
            remove(
                &f.root,
                &f.root.join("runtime"),
                "fixture",
                &old["settings"],
                if retain_collection {
                    &old["settings"]
                } else {
                    &new["settings"]
                },
                vec!["222222".into()]
            )
            .is_err()
        );
        assert_eq!(
            fs::read(f.source().join("payload.pak")).unwrap(),
            b"selected package"
        );
        assert_eq!(
            fs::read(f.root.join("config/instance.json")).unwrap(),
            f.before
        );
        assert!(!f.root.join(JOURNAL).exists());
    }
}

#[test]
fn unchanged_settings_recovery_uses_an_owned_commit_marker() {
    for committed in [false, true] {
        let mut f = Fixture::new();
        f.after = f.before.clone();
        let operation = f.stage();
        fs::rename(f.source(), operation.join("222222")).unwrap();
        if committed {
            fs::copy(
                operation.join("owner.json"),
                operation.join("committed.json"),
            )
            .unwrap();
        }
        recover(&f.root, false).unwrap();
        assert_eq!(f.source().exists(), !committed);
        assert_eq!(operation.join("222222/payload.pak").exists(), committed);
        assert_eq!(
            fs::read(f.root.join("config/instance.json")).unwrap(),
            f.before
        );
        assert!(!f.root.join(JOURNAL).exists());
    }
}

#[test]
fn single_member_removal_preserves_exact_configuration_bytes() {
    let f = Fixture::new();
    let old: Value = serde_json::from_slice(&f.before).unwrap();
    remove(
        &f.root,
        &f.root.join("runtime"),
        "fixture",
        &old["settings"],
        &old["settings"],
        vec!["222222".into()],
    )
    .unwrap();
    assert!(!f.source().exists());
    assert_eq!(
        fs::read(f.root.join("config/instance.json")).unwrap(),
        f.before
    );
    assert!(
        f.root
            .join("runtime/SquadGame/Plugins/Mods/333333/payload.pak")
            .exists()
    );
}

#[test]
fn unchanged_settings_foreign_commit_marker_is_preserved() {
    let mut f = Fixture::new();
    f.after = f.before.clone();
    let operation = f.stage();
    fs::rename(f.source(), operation.join("222222")).unwrap();
    fs::write(operation.join("committed.json"), b"foreign").unwrap();
    assert!(recover(&f.root, false).is_err());
    assert!(operation.join("222222/payload.pak").exists());
    assert!(f.root.join(JOURNAL).exists());
}

#[test]
fn unchanged_settings_failed_commit_marker_rolls_back() {
    let mut f = Fixture::new();
    f.after = f.before.clone();
    let operation = f.stage();
    fs::rename(f.source(), operation.join("222222")).unwrap();
    let marker = operation.join(COMMITTED);
    crate::atomic_file::fail_next_atomic_write_for_test(&marker);
    assert!(
        cas(
            &marker,
            None,
            Some(&fs::read(operation.join("owner.json")).unwrap())
        )
        .is_err()
    );
    recover(&f.root, false).unwrap();
    assert!(f.source().join("payload.pak").exists());
    assert!(!f.root.join(JOURNAL).exists());
}

#[test]
fn unchanged_settings_external_config_edits_never_trigger_payload_recovery() {
    for committed in [false, true] {
        let mut f = Fixture::new();
        f.after = f.before.clone();
        let operation = f.stage();
        fs::rename(f.source(), operation.join("222222")).unwrap();
        if committed {
            fs::copy(operation.join("owner.json"), operation.join(COMMITTED)).unwrap();
        }
        let config = f.root.join("config/instance.json");
        let changed =
            br#"{"instance_id":"fixture","module_id":"squad","settings":{"external":true}}"#;
        fs::write(&config, changed).unwrap();
        assert!(recover(&f.root, false).is_err());
        assert_eq!(fs::read(&config).unwrap(), changed);
        assert!(operation.join("222222/payload.pak").exists());
        assert!(f.root.join(JOURNAL).exists());
    }
}

#[test]
fn recovery_preserves_external_changes_collisions_and_running_state() {
    for conflict in ["settings", "collision", "active", "owner"] {
        let f = Fixture::new();
        let operation = f.stage();
        fs::rename(f.source(), operation.join("222222")).unwrap();
        match conflict {
            "settings" => fs::write(f.root.join("config/instance.json"), b"{\"instance_id\":\"fixture\",\"module_id\":\"squad\",\"settings\":{\"changed\":true}}").unwrap(),
            "collision" => { fs::create_dir(f.source()).unwrap(); fs::write(f.source().join("operator.pak"), b"external").unwrap(); },
            "owner" => fs::write(operation.join("owner.json"), b"{}").unwrap(),
            _ => (),
        }
        assert!(
            recover(&f.root, conflict == "active").is_err(),
            "{conflict}"
        );
        assert_eq!(
            fs::read(operation.join("222222/payload.pak")).unwrap(),
            b"selected package"
        );
        assert!(f.root.join(JOURNAL).exists());
    }
}

#[test]
fn rejects_non_numeric_journal_member_and_file_in_place_of_directory() {
    let f = Fixture::new();
    let operation = f.stage();
    let mut value: Value =
        serde_json::from_slice(&fs::read(f.root.join(JOURNAL)).unwrap()).unwrap();
    value["members"] = json!(["../333333"]);
    let bytes = serde_json::to_vec(&value).unwrap();
    fs::write(f.root.join(JOURNAL), &bytes).unwrap();
    fs::write(operation.join("owner.json"), bytes).unwrap();
    assert!(recover(&f.root, false).is_err());
    assert!(f.source().join("payload.pak").exists());
    assert!(plain_directory(&f.source().join("payload.pak")).is_err());
}

#[cfg(windows)]
#[test]
fn rejects_junction_ancestors_without_touching_the_destination() {
    let f = Fixture::new();
    let outside = f.root.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("preserve"), b"external").unwrap();
    let link = f.root.join(RETAINED);
    let output = std::process::Command::new("cmd.exe")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&outside)
        .output()
        .unwrap();
    assert!(output.status.success());
    let result = plain_directory(&link);
    // Remove only the junction entry before the fixture recursively cleans its own tree.
    fs::remove_dir(&link).unwrap();
    assert!(result.is_err());
    assert_eq!(fs::read(outside.join("preserve")).unwrap(), b"external");
}
