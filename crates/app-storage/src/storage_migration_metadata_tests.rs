use super::*;
use serde_json::json;

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-migration-metadata-{}", uuid::Uuid::new_v4()));
        let state = root.join("state");
        let instances = root.join("instances");
        let paths = StoragePaths {
            app_data_root: state.clone(),
            settings_path: state.join("settings.json"),
            database_path: state.join("db/lgs.db"),
            logs_root: state.join("logs"),
            modules_root: root.join("modules"),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            archives_root: instances.join(".trash"),
            instances_root: instances,
        };
        for directory in [
            &state,
            &paths.instances_root,
            &paths.archives_root,
            &paths.games_root,
            &paths.steamcmd_root,
            &paths.logs_root,
            &paths.modules_root,
            &paths.migrations_root,
        ] {
            fs::create_dir_all(directory).unwrap();
        }
        Self { root, paths }
    }

    fn movement(&self, source: &Path, leaf: &str) -> RootMove {
        RootMove {
            source: source.to_owned(),
            target: self.root.join(leaf),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn write_json(path: &Path, value: &Value) -> Vec<u8> {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bytes = serde_json::to_vec_pretty(value).unwrap();
    fs::write(path, &bytes).unwrap();
    bytes
}

fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).unwrap();
        }
    }
}

#[tokio::test]
async fn steamcmd_plan_rebinds_only_validated_copy_without_writing_any_store() {
    let fixture = Fixture::new();
    let marker = fixture
        .paths
        .steamcmd_root
        .join(".langame-steamcmd-owner.json");
    let original = write_json(
        &marker,
        &json!({
            "product": "langame-server-manager",
            "canonical_root": fs::canonicalize(&fixture.paths.steamcmd_root).unwrap(),
        }),
    );
    let movement = fixture.movement(&fixture.paths.steamcmd_root, "moved-steamcmd");
    preflight_metadata(&fixture.paths, std::slice::from_ref(&movement))
        .await
        .unwrap();
    assert!(
        !movement.target.exists(),
        "preflight must not create destination storage"
    );
    copy_tree(&movement.source, &movement.target);
    let plan = prepare_metadata(&fixture.paths, std::slice::from_ref(&movement))
        .await
        .unwrap();
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].before_sha256, digest(&original));
    let changed: Value = serde_json::from_slice(&plan.files[0].after).unwrap();
    assert_eq!(
        PathBuf::from(changed["canonical_root"].as_str().unwrap()),
        fs::canonicalize(&movement.target).unwrap()
    );
    assert_eq!(fs::read(&marker).unwrap(), original);
    assert_eq!(
        fs::read(movement.target.join(marker.file_name().unwrap())).unwrap(),
        original
    );
    assert!(!fixture.paths.database_path.exists());
    assert!(!fixture.paths.settings_path.exists());
}

#[tokio::test]
async fn changed_destination_ownership_is_rejected_before_a_patch_is_returned() {
    let fixture = Fixture::new();
    let marker = fixture
        .paths
        .steamcmd_root
        .join(".langame-steamcmd-owner.json");
    let original = write_json(
        &marker,
        &json!({
            "product":"langame-server-manager", "canonical_root":fs::canonicalize(&fixture.paths.steamcmd_root).unwrap(),
        }),
    );
    let movement = fixture.movement(&fixture.paths.steamcmd_root, "moved-steamcmd");
    copy_tree(&movement.source, &movement.target);
    let target_marker = movement.target.join(marker.file_name().unwrap());
    fs::write(&target_marker, b"unrelated ownership").unwrap();
    let error = prepare_metadata(&fixture.paths, &[movement])
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("differs from its validated source")
    );
    assert_eq!(fs::read(&marker).unwrap(), original);
    assert_eq!(fs::read(&target_marker).unwrap(), b"unrelated ownership");
}

#[test]
fn managed_console_copy_retains_owner_and_sequence_but_rebinds_actual_file_identity() {
    let fixture = Fixture::new();
    let directory = fixture
        .paths
        .instances_root
        .join("one/logs/managed-console");
    fs::create_dir_all(&directory).unwrap();
    let log = directory.join("run-one.log");
    fs::write(&log, b"preserved console contents\n").unwrap();
    let owner = uuid::Uuid::new_v4().to_string();
    let original = write_json(
        &directory.join("ownership.json"),
        &json!({
            "format":"langame-managed-console", "owner":owner, "next_sequence":2,
            "runs":{"run-one.log":[{"file":"run-one.log", "identity":serde_json::from_str::<Value>(&identity(&log, false).unwrap()).unwrap(),"start":0,"sequence":1}]},
            "moves":[],"deletions":[],
        }),
    );
    let movement = fixture.movement(&fixture.paths.instances_root, "moved-instances");
    copy_tree(&movement.source, &movement.target);
    let mut plan = MetadataPlan::default();
    console::prepare(&directory, std::slice::from_ref(&movement), true, &mut plan).unwrap();
    let changed: Value = serde_json::from_slice(&plan.files[0].after).unwrap();
    let next_log = remap_path(&log, &[movement]).unwrap();
    assert_eq!(changed["owner"], owner);
    assert_eq!(changed["runs"]["run-one.log"][0]["start"], 0);
    assert_eq!(changed["runs"]["run-one.log"][0]["sequence"], 1);
    assert_eq!(
        changed["runs"]["run-one.log"][0]["identity"],
        serde_json::from_str::<Value>(&identity(&next_log, false).unwrap()).unwrap()
    );
    assert_eq!(
        fs::read(directory.join("ownership.json")).unwrap(),
        original
    );
    assert_eq!(fs::read(&plan.files[0].path).unwrap(), original);
}

#[test]
fn equal_log_bytes_cannot_authorize_a_forged_source_identity() {
    let fixture = Fixture::new();
    let directory = fixture
        .paths
        .instances_root
        .join("one/logs/managed-console");
    fs::create_dir_all(&directory).unwrap();
    let log = directory.join("run-one.log");
    let unrelated = fixture.root.join("unrelated.log");
    fs::write(&log, b"same content").unwrap();
    fs::write(&unrelated, b"same content").unwrap();
    write_json(
        &directory.join("ownership.json"),
        &json!({
            "format":"langame-managed-console","owner":uuid::Uuid::new_v4().to_string(),"next_sequence":2,
            "runs":{"run-one.log":[{"file":"run-one.log","identity":serde_json::from_str::<Value>(&identity(&unrelated,false).unwrap()).unwrap(),"start":0,"sequence":1}]},
            "moves":[],"deletions":[],
        }),
    );
    let movement = fixture.movement(&fixture.paths.instances_root, "moved-instances");
    copy_tree(&movement.source, &movement.target);
    let error =
        console::prepare(&directory, &[movement], true, &mut MetadataPlan::default()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Source filesystem identity differs")
    );
}

#[tokio::test]
async fn pending_program_and_cluster_restores_are_preflight_blockers() {
    let fixture = Fixture::new();
    let movement = fixture.movement(&fixture.paths.instances_root, "moved-instances");
    let journal = fixture
        .paths
        .instances_root
        .join("one/.langame-program-adoption.json");
    write_json(&journal, &json!({}));
    let error = preflight_metadata(&fixture.paths, std::slice::from_ref(&movement))
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unfinished data or program recovery")
    );
    fs::remove_file(journal).unwrap();
    let pending = fixture
        .paths
        .app_data_root
        .join("cluster-backups/group/restore-pending.json");
    write_json(&pending, &json!({}));
    let error = preflight_metadata(&fixture.paths, &[movement])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("pending ARK cluster restore"));
}

#[test]
fn free_form_absolute_path_references_are_reported_without_rewriting_text() {
    let fixture = Fixture::new();
    let movement = fixture.movement(&fixture.paths.games_root, "moved-games");
    let value = json!({"custom_launch_flags":format!("-modsdir=\"{}\"", fixture.paths.games_root.join("mods").display())});
    let original = value.clone();
    let error =
        preflight::reject_references(&value, Path::new("instance.json"), "settings", &[movement])
            .unwrap_err();
    assert!(error.to_string().contains("settings.custom_launch_flags"));
    assert_eq!(value, original);
}

#[tokio::test]
async fn interrupted_windrose_bootstrap_is_rejected_before_destination_creation() {
    let fixture = Fixture::new();
    let runtime = fixture.paths.instances_root.join("windrose/runtime");
    let journal = runtime.join(".langame-windrose-bootstrap.json");
    let original = write_json(
        &journal,
        &json!({
            "version":1,"instance_id":"windrose","install_root":runtime,"original":null,
        }),
    );
    let movement = fixture.movement(&fixture.paths.instances_root, "moved-instances");
    let error = preflight_metadata(&fixture.paths, std::slice::from_ref(&movement))
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unfinished data or program recovery")
    );
    assert!(!movement.target.exists());
    assert_eq!(fs::read(&journal).unwrap(), original);
}

#[path = "storage_migration_metadata_archive_tests.rs"]
mod archive_tests;

#[cfg(windows)]
#[tokio::test]
async fn migration_rebinds_mod_state_owner_without_renaming_or_rewriting_recovery_payloads() {
    let fixture = Fixture::new();
    let target = fixture.paths.instances_root.join("one/runtime/Mods");
    fs::create_dir_all(&target).unwrap();
    let owner = crate::mod_management_state::target_identity(&fs::canonicalize(&target).unwrap());
    let leaf = crate::mod_management_state::initial_directory_name(&owner);
    let state = target.parent().unwrap().join(&leaf);
    let original = write_json(&state.join("owner.json"), &Value::String(owner));
    let pending = write_json(&state.join("pending.json"), &json!({"action":"disable"}));
    fs::create_dir_all(state.join("disabled")).unwrap();
    fs::write(state.join("disabled/Example.dll"), b"disabled plugin").unwrap();
    let movement = fixture.movement(&fixture.paths.instances_root, "moved-instances");
    preflight_metadata(&fixture.paths, std::slice::from_ref(&movement))
        .await
        .unwrap();
    assert!(!movement.target.exists());
    copy_tree(&movement.source, &movement.target);
    let plan = prepare_metadata(&fixture.paths, std::slice::from_ref(&movement))
        .await
        .unwrap();
    assert_eq!(plan.files.len(), 1);
    let expected_state = movement.target.join("one/runtime").join(&leaf);
    let patch = &plan.files[0];
    assert!(same_path(&patch.path, &expected_state.join("owner.json")).unwrap());
    assert_eq!(patch.before, original);
    let expected_owner = crate::mod_management_state::target_identity(
        &fs::canonicalize(movement.target.join("one/runtime/Mods")).unwrap(),
    );
    assert_eq!(
        serde_json::from_slice::<String>(&patch.after).unwrap(),
        expected_owner
    );
    assert_ne!(
        leaf,
        crate::mod_management_state::initial_directory_name(&expected_owner)
    );
    assert_eq!(
        fs::read(expected_state.join("pending.json")).unwrap(),
        pending
    );
    assert_eq!(
        fs::read(expected_state.join("disabled/Example.dll")).unwrap(),
        b"disabled plugin"
    );
    assert_eq!(
        fs::read(expected_state.join("owner.json")).unwrap(),
        original
    );
    assert_eq!(fs::read(state.join("owner.json")).unwrap(), original);
}
