use super::*;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "langame-managed-config-merge-{}",
            Uuid::new_v4().as_simple()
        ));
        fs::create_dir_all(&path).expect("create test root");
        Self(path)
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn pending_configuration_rollback_preserves_a_concurrent_writer_and_restores_other_files() {
    let root = TestRoot::new();
    let first = root.0.join("first.json");
    let second = root.0.join("second.json");
    let original = br#"{"value":"original"}"#;
    let replacement = br#"{"value":"replacement"}"#;
    let external = br#"{"value":"external"}"#;
    for path in [&first, &second] {
        fs::write(path, original).unwrap();
    }
    let plans = [&first, &second]
        .into_iter()
        .map(|path| ManagedConfigMergePlan {
            destination_path: path.clone(),
            replacement: replacement.to_vec(),
            original: Some(original.to_vec()),
        })
        .collect();
    let mutation =
        managed_config_merge::apply_pending_managed_config_plans(plans, "enshrouded").unwrap();
    fs::write(&first, external).unwrap();
    let error = mutation.rollback_after(StorageError::InvalidSettingsRoot);
    assert!(matches!(
        error,
        StorageError::ModuleSupportMaterialization { .. }
    ));
    assert_eq!(fs::read(first).unwrap(), external);
    assert_eq!(fs::read(second).unwrap(), original);
}

#[test]
fn ini_merge_preserves_unknown_values_and_removes_obsolete_sections() {
    let root = TestRoot::new();
    let source = root.0.join("rendered.ini");
    let destination = root.0.join("install").join("server.ini");
    fs::write(
        &source,
        "[/Script/Astro.AstroServerSettings]\nServerName=New name\nMaxServerFramerate=30\n",
    )
    .expect("write source");
    fs::create_dir_all(destination.parent().expect("destination parent"))
        .expect("create destination parent");
    fs::write(
        &destination,
        "; package-owned comment\n[/Script/Astro.AstroServerSettings]\nServerGuid=stable-id\nServerName=Old name\nUnknownFuture=keep\nFutureArray=[one,two]\nServerName=duplicate\n\n[AstroServerSettings]\nPublicIPName=obsolete\n\n[Other.Section]\nEnabled=True\n",
    )
    .expect("write destination");

    merge_rendered_ini_file(
        &source,
        &destination,
        "astroneer",
        &["AstroServerSettings"],
        &mut ManagedConfigMutation::new("fixture"),
    )
    .expect("merge INI");

    let merged = fs::read_to_string(destination).expect("read merged INI");
    assert_eq!(merged.matches("ServerName=").count(), 1);
    assert!(merged.contains("ServerName=New name"));
    assert!(merged.contains("ServerGuid=stable-id"));
    assert!(merged.contains("UnknownFuture=keep"));
    assert!(merged.contains("FutureArray=[one,two]"));
    assert!(merged.contains("[Other.Section]"));
    assert!(!merged.contains("[AstroServerSettings]"));
}

#[test]
fn ini_merge_replaces_generated_comments_without_duplication() {
    let root = TestRoot::new();
    let source = root.0.join("rendered.ini");
    let destination = root.0.join("server.ini");
    let rendered = "[Sandbox]\n; Difficulty\nGameDifficulty=1\n\n; Spawn rate\nEnemySpawnRate=1\n";
    fs::write(&source, rendered).expect("write source");
    fs::write(
        &destination,
        "[Sandbox]\n; operator note\n; Difficulty\nGameDifficulty=3\n\n; Spawn rate\nEnemySpawnRate=2\nFutureOption=keep\n",
    )
    .expect("write destination");

    merge_rendered_ini_file(
        &source,
        &destination,
        "abioticfactor",
        &[],
        &mut ManagedConfigMutation::new("fixture"),
    )
    .expect("merge changed INI");
    let expected = "[Sandbox]\n; operator note\n; Difficulty\nGameDifficulty=1\n\n; Spawn rate\nEnemySpawnRate=1\nFutureOption=keep\n";
    assert_eq!(
        fs::read_to_string(&destination).expect("read first merge"),
        expected
    );

    merge_rendered_ini_file(
        &source,
        &destination,
        "abioticfactor",
        &[],
        &mut ManagedConfigMutation::new("fixture"),
    )
    .expect("merge unchanged INI");
    assert_eq!(
        fs::read_to_string(destination).expect("read second merge"),
        expected,
        "repeated saves must not append another copy of generated comments"
    );
}

#[test]
fn ini_merge_does_not_accumulate_metadata_between_sections() {
    let root = TestRoot::new();
    let source = root.0.join("rendered.ini");
    let destination = root.0.join("server.ini");
    let rendered = "[First]\nValue=1\n\n; second section\n[Second]\nEnabled=True\n";
    fs::write(&source, rendered).expect("write source");
    fs::write(&destination, rendered).expect("write destination");

    merge_rendered_ini_file(
        &source,
        &destination,
        "test",
        &[],
        &mut ManagedConfigMutation::new("fixture"),
    )
    .expect("first merge");
    merge_rendered_ini_file(
        &source,
        &destination,
        "test",
        &[],
        &mut ManagedConfigMutation::new("fixture"),
    )
    .expect("second merge");

    assert_eq!(
        fs::read_to_string(destination).expect("read merged INI"),
        rendered,
        "section-separating metadata must remain stable across repeated saves"
    );
}

#[test]
fn ini_merge_accepts_a_utf8_bom_before_preamble_comments() {
    let root = TestRoot::new();
    let source = root.0.join("rendered.ini");
    let destination = root.0.join("server.ini");
    fs::write(&source, "[Server]\nName=new\n").expect("write source");
    fs::write(
        &destination,
        "\u{feff}  ; package comment\n[Server]\nName=old\nFuture=keep\n",
    )
    .expect("write destination");

    merge_rendered_ini_file(
        &source,
        &destination,
        "test",
        &[],
        &mut ManagedConfigMutation::new("fixture"),
    )
    .expect("merge BOM INI");

    let merged = fs::read_to_string(destination).expect("read merged INI");
    assert!(merged.starts_with("\u{feff}  ; package comment\n"));
    assert!(merged.contains("Name=new"));
    assert!(merged.contains("Future=keep"));
}

#[test]
fn ini_merge_preserves_a_utf8_bom_directly_before_the_first_section() {
    let root = TestRoot::new();
    let source = root.0.join("rendered.ini");
    let destination = root.0.join("server.ini");
    fs::write(&source, "[Server]\nName=new\n").expect("write source");
    fs::write(&destination, "\u{feff}[Server]\nName=old\nFuture=keep\n")
        .expect("write destination");

    merge_rendered_ini_file(
        &source,
        &destination,
        "test",
        &[],
        &mut ManagedConfigMutation::new("fixture"),
    )
    .expect("first merge");
    let expected = "\u{feff}[Server]\nName=new\nFuture=keep\n";
    assert_eq!(
        fs::read_to_string(&destination).expect("read first merge"),
        expected
    );

    merge_rendered_ini_file(
        &source,
        &destination,
        "test",
        &[],
        &mut ManagedConfigMutation::new("fixture"),
    )
    .expect("second merge");
    assert_eq!(
        fs::read_to_string(destination).expect("read second merge"),
        expected,
        "a repeated merge must preserve exactly one leading UTF-8 BOM"
    );
}

#[test]
fn malformed_existing_ini_fails_without_rewriting() {
    let root = TestRoot::new();
    let source = root.0.join("rendered.ini");
    let destination = root.0.join("server.ini");
    fs::write(&source, "[Server]\nName=new\n").expect("write source");
    let original = b"[broken\nName=old\n";
    fs::write(&destination, original).expect("write destination");

    let error = merge_rendered_ini_file(
        &source,
        &destination,
        "test",
        &[],
        &mut ManagedConfigMutation::new("fixture"),
    )
    .expect_err("malformed INI must fail");

    assert!(matches!(
        error,
        StorageError::ModuleSupportMaterialization { .. }
    ));
    assert_eq!(fs::read(destination).expect("read destination"), original);
}

#[test]
fn batch_validation_finishes_before_any_destination_is_rewritten() {
    let root = TestRoot::new();
    let first_source = root.0.join("first-rendered.ini");
    let second_source = root.0.join("second-rendered.ini");
    let first_destination = root.0.join("first.ini");
    let second_destination = root.0.join("second.ini");
    fs::write(&first_source, "[Server]\nName=new\n").expect("write first source");
    fs::write(&second_source, "[Server]\nName=new\n").expect("write second source");
    let first_original = b"[Server]\nName=old\nUnknown=keep\n";
    fs::write(&first_destination, first_original).expect("write first destination");
    fs::write(&second_destination, b"[broken\nName=old\n").expect("write second destination");

    let error = merge_rendered_ini_files(
        &[
            ManagedIniFile {
                source_path: &first_source,
                destination_path: &first_destination,
                removed_sections: &[],
            },
            ManagedIniFile {
                source_path: &second_source,
                destination_path: &second_destination,
                removed_sections: &[],
            },
        ],
        "test",
        &mut ManagedConfigMutation::new("fixture"),
    )
    .expect_err("malformed second destination must reject the complete batch");

    assert!(matches!(
        error,
        StorageError::ModuleSupportMaterialization { .. }
    ));
    assert_eq!(
        fs::read(first_destination).expect("read first destination"),
        first_original
    );
}

#[test]
fn later_write_failure_rolls_back_an_earlier_destination() {
    let root = TestRoot::new();
    let first_destination = root.0.join("first.ini");
    let second_destination = root.0.join("second.ini");
    let first_original = b"[Server]\nName=old\n".to_vec();
    fs::write(&first_destination, &first_original).expect("write first destination");
    fs::write(&second_destination, b"[Server]\nName=old\n").expect("write second destination");
    let plans = vec![
        ManagedConfigMergePlan {
            destination_path: first_destination.clone(),
            replacement: b"[Server]\nName=new\n".to_vec(),
            original: Some(first_original.clone()),
        },
        ManagedConfigMergePlan {
            destination_path: second_destination,
            replacement: b"[Server]\nName=new\n".to_vec(),
            original: Some(b"[Server]\nName=old\n".to_vec()),
        },
    ];
    let mut writes = 0;

    let error = apply_managed_config_plans_with_writer(plans, "test", |plan| {
        writes += 1;
        if writes == 2 {
            return Err(materialization_error(
                "test",
                &plan.destination_path,
                String::from("injected second write failure"),
            ));
        }
        write_managed_config_plan(plan, "test")
    })
    .expect_err("second write failure must fail the batch");

    assert!(matches!(
        error,
        StorageError::ModuleSupportMaterialization { .. }
    ));
    assert_eq!(
        fs::read(first_destination).expect("read rolled back destination"),
        first_original
    );
}

#[test]
fn json_merge_preserves_unknown_keys_and_replaces_managed_keys() {
    let root = TestRoot::new();
    let source = root.0.join("rendered.json");
    let destination = root.0.join("install").join("config.json");
    fs::write(
        &source,
        r#"{"Port":27015,"AutoCreateWorldSeed":"world-seed"}"#,
    )
    .expect("write source");
    fs::create_dir_all(destination.parent().expect("destination parent"))
        .expect("create destination parent");
    fs::write(&destination, r#"{"Port":1,"UnknownFuture":{"keep":true}}"#)
        .expect("write destination");

    merge_rendered_json_object_file(
        &source,
        &destination,
        "romestead",
        &mut ManagedConfigMutation::new("fixture"),
    )
    .expect("merge JSON");

    let merged: Value = serde_json::from_slice(&fs::read(destination).expect("read destination"))
        .expect("parse merged JSON");
    assert_eq!(merged["Port"], 27015);
    assert_eq!(merged["AutoCreateWorldSeed"], "world-seed");
    assert_eq!(merged["UnknownFuture"]["keep"], true);
}

#[test]
fn json_merge_recursively_preserves_nested_unknown_keys() {
    let root = TestRoot::new();
    let source = root.0.join("rendered.json");
    let destination = root.0.join("config.json");
    fs::write(
        &source,
        r#"{"traders":{"armory":{"price":20,"rows":["new"]}}}"#,
    )
    .expect("write source");
    fs::write(
        &destination,
        r#"{"traders":{"armory":{"price":10,"future":"keep","rows":["old"]},"future-trader":{"enabled":true}}}"#,
    )
    .expect("write destination");

    merge_rendered_json_object_file(
        &source,
        &destination,
        "test",
        &mut ManagedConfigMutation::new("fixture"),
    )
    .expect("merge JSON");

    let merged: Value = serde_json::from_slice(&fs::read(destination).expect("read destination"))
        .expect("parse merged JSON");
    assert_eq!(merged["traders"]["armory"]["price"], 20);
    assert_eq!(merged["traders"]["armory"]["future"], "keep");
    assert_eq!(
        merged["traders"]["armory"]["rows"],
        serde_json::json!(["new"])
    );
    assert_eq!(merged["traders"]["future-trader"]["enabled"], true);
}

#[test]
fn mixed_batch_prevalidates_every_file_and_copies_text_bytes_exactly() {
    let root = TestRoot::new();
    let ini_source = root.0.join("rendered.ini");
    let json_source = root.0.join("rendered.json");
    let text_source = root.0.join("rendered.txt");
    let ini_destination = root.0.join("server.ini");
    let json_destination = root.0.join("config.json");
    let text_destination = root.0.join("roster.txt");
    fs::write(&ini_source, "[Server]\nName=new\n").expect("write INI source");
    fs::write(&json_source, r#"{"managed":true}"#).expect("write JSON source");
    let roster = b"first\r\nsecond\n\0binary-tail";
    fs::write(&text_source, roster).expect("write text source");
    let ini_original = b"[Server]\nName=old\nUnknown=keep\n";
    fs::write(&ini_destination, ini_original).expect("write INI destination");
    fs::write(&json_destination, b"not valid JSON").expect("write JSON destination");

    let files = [
        ManagedConfigFile::Ini {
            source_path: &ini_source,
            destination_path: &ini_destination,
            removed_sections: &[],
        },
        ManagedConfigFile::JsonObject {
            source_path: &json_source,
            destination_path: &json_destination,
        },
        ManagedConfigFile::Text {
            source_path: &text_source,
            destination_path: &text_destination,
        },
    ];
    merge_rendered_config_files(&files, "test", &mut ManagedConfigMutation::new("fixture"))
        .expect_err("malformed JSON must reject the complete mixed batch");
    assert_eq!(fs::read(&ini_destination).expect("read INI"), ini_original);
    assert!(!text_destination.exists());

    fs::write(&json_destination, r#"{"future":"keep"}"#).expect("repair JSON");
    merge_rendered_config_files(&files, "test", &mut ManagedConfigMutation::new("fixture"))
        .expect("merge mixed batch");
    assert_eq!(fs::read(text_destination).expect("read text"), roster);
}

#[test]
fn managed_config_journal_does_not_restore_an_older_write_after_external_recreation() {
    let root = TestRoot::new();
    let path = root.0.join("profile.json");
    fs::write(&path, b"original").unwrap();
    let mut files = ManagedConfigMutation::new("soulmask");
    files.write(&path, b"rendered").unwrap();
    files.remove(&path).unwrap();
    fs::write(&path, b"rendered").unwrap();
    assert!(files.rollback().is_err());
    assert!(fs::read(&path).unwrap() == b"rendered");
}

#[test]
fn managed_config_journal_keeps_partial_batch_ownership_after_failure() {
    let root = TestRoot::new();
    let first = root.0.join("first.ini");
    let second = root.0.join("second.ini");
    fs::write(&first, b"initial").unwrap();
    let mut files = ManagedConfigMutation::new("fixture");
    files.write(&first, b"rendered").unwrap();
    crate::atomic_file::fail_next_atomic_write_for_test(&second);
    let error = files
        .apply(vec![
            ManagedConfigMergePlan {
                destination_path: first.clone(),
                original: Some(b"rendered".to_vec()),
                replacement: b"merged".to_vec(),
            },
            ManagedConfigMergePlan {
                destination_path: second,
                original: None,
                replacement: b"new".to_vec(),
            },
        ])
        .unwrap_err();
    fs::write(&first, b"rendered").unwrap();
    let error = files.rollback_after(error);
    assert!(error.to_string().contains("changed concurrently"));
    assert!(fs::read(&first).unwrap() == b"rendered");
}
