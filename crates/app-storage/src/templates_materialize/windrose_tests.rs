use super::*;
use serde_json::json;
pub(super) struct TestRoot(PathBuf);

impl TestRoot {
    pub(super) fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "lsgm-windrose-{label}-{}",
            uuid::Uuid::new_v4().as_simple()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn server_document(world_id: &str) -> Value {
    json!({
        "Version": 1,
        "DeploymentId": "build-owned",
        "UnknownServerRoot": { "preserve": true },
        "ServerDescription_Persistent": {
            "PersistentServerId": "SERVER-ID",
            "WorldIslandId": world_id,
            "ServerName": "Before",
            "UnknownPersistent": "keep-me"
        }
    })
}

fn world_document(world_id: &str) -> Value {
    json!({
        "Version": 1,
        "UnknownWorldRoot": [1, 2, 3],
        "WorldDescription": {
            "islandId": world_id,
            "WorldName": "Before",
            "CreationTime": 639118875766649980_i64,
            "WorldPresetType": "Medium",
            "UnknownWorldDescription": { "nested": "keep-me" },
            "WorldSettings": {
                "UnknownSettings": true,
                "BoolParameters": {
                    (COOP_QUESTS_TAG): true,
                    (EASY_EXPLORE_TAG): false,
                    "UnknownBool": true
                },
                "FloatParameters": {
                    (MOB_HEALTH_TAG): 1.0,
                    (MOB_DAMAGE_TAG): 1.0,
                    (SHIP_HEALTH_TAG): 1.0,
                    (SHIP_DAMAGE_TAG): 1.0,
                    (BOARDING_DIFFICULTY_TAG): 1.0,
                    (COOP_STATS_TAG): 1.0,
                    (COOP_SHIP_STATS_TAG): 0.0,
                    "UnknownFloat": 9.25
                },
                "TagParameters": {
                    (COMBAT_DIFFICULTY_TAG): {
                        "TagName": "WDS.Parameter.CombatDifficulty.Normal",
                        "UnknownCombat": "keep-me"
                    },
                    "UnknownTag": { "TagName": "Unknown.Value" }
                }
            }
        }
    })
}

fn write_json(path: &Path, document: &Value) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec_pretty(document).unwrap()).unwrap();
}

pub(super) fn server_path(root: &Path) -> PathBuf {
    root.join("R5").join(WINDROSE_SERVER_DESCRIPTION_FILE)
}

pub(super) fn world_path(root: &Path, version: &str, world_id: &str) -> PathBuf {
    root.join("R5")
        .join("Saved")
        .join("SaveProfiles")
        .join("Default")
        .join("RocksDB_v2")
        .join(version)
        .join("Worlds")
        .join(world_id)
        .join(WORLD_DESCRIPTION_FILE)
}

pub(super) fn seed_world(
    root: &Path,
    version: &str,
    folder_id: &str,
    document_id: &str,
) -> PathBuf {
    let path = world_path(root, version, folder_id);
    write_json(&path, &world_document(document_id));
    path
}

pub(super) fn seed_server(root: &Path, world_id: &str) {
    write_json(&server_path(root), &server_document(world_id));
}

pub(super) fn world_settings(world_id: &str) -> Map<String, Value> {
    json!({
        "world_island_id": world_id,
        "world_name": "海风群岛 ⚓",
        "world_preset_type": "Medium",
        "coop_quests": false,
        "easy_explore": true,
        "mob_health_multiplier": 5.0,
        "mob_damage_multiplier": 0.2,
        "ship_health_multiplier": 0.4,
        "ship_damage_multiplier": 2.5,
        "boarding_difficulty_multiplier": 3.25,
        "coop_stats_correction_modifier": 2.0,
        "coop_ship_stats_correction_modifier": 0.0,
        "combat_difficulty": "Hard"
    })
    .as_object()
    .unwrap()
    .clone()
}

fn rendered_server(world_id: &str, name: &str) -> Value {
    json!({
        "Version": 1,
        "ServerDescription_Persistent": {
            "PersistentServerId": "SERVER-ID",
            "WorldIslandId": world_id,
            "ServerName": name,
            "MaxPlayerCount": 8
        }
    })
}

pub(super) fn seed_rendered_server(config_dir: &Path, world_id: &str, name: &str) {
    write_json(
        &config_dir.join(WINDROSE_SERVER_DESCRIPTION_FILE),
        &rendered_server(world_id, name),
    );
}

#[test]
fn resolver_rejects_empty_and_unsafe_world_selections() {
    let root = TestRoot::new("unsafe-selection");
    assert!(matches!(
        resolve_world_description(root.path(), "  "),
        Err(WindroseWorldTargetError::EmptySelection)
    ));
    for selection in ["../WORLD", "version/WORLD", "version\\WORLD", "."] {
        assert!(matches!(
            resolve_world_description(root.path(), selection),
            Err(WindroseWorldTargetError::UnsafeSelection { .. })
        ));
    }
}

#[test]
fn resolver_never_guesses_zero_or_multiple_matches() {
    let root = TestRoot::new("cardinality");
    let world_id = "E24A22C9C8D3448951AFD002162576D5";
    fs::create_dir_all(root.path().join("R5/Saved/SaveProfiles/Default/RocksDB_v2")).unwrap();
    assert!(matches!(
        resolve_world_description(root.path(), world_id),
        Err(WindroseWorldTargetError::NoMatch { .. })
    ));

    seed_server(root.path(), world_id);
    seed_world(root.path(), "0.10.0", world_id, world_id);
    seed_world(root.path(), "0.10.1", world_id, world_id);
    assert!(matches!(
        resolve_world_description(root.path(), world_id),
        Err(WindroseWorldTargetError::Ambiguous { matches: 2, .. })
    ));
}

#[test]
fn resolver_rejects_malformed_world_json_without_writing() {
    let root = TestRoot::new("malformed");
    let world_id = "E24A22C9C8D3448951AFD002162576D5";
    seed_server(root.path(), world_id);
    let path = seed_world(root.path(), "0.10.0", world_id, world_id);
    fs::write(&path, b"{ definitely-not-json").unwrap();
    let before = fs::read(&path).unwrap();
    assert!(matches!(
        resolve_world_description(root.path(), world_id),
        Err(WindroseWorldTargetError::MalformedJson { .. })
    ));
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn resolver_requires_folder_file_selection_and_server_identity_agreement() {
    let root = TestRoot::new("identity");
    let selected = "E24A22C9C8D3448951AFD002162576D5";
    let other = "26C14DC8A78D4AF69E9C77527C934CF3";
    seed_server(root.path(), selected);
    let path = seed_world(root.path(), "0.10.0", selected, other);
    assert!(matches!(
        resolve_world_description(root.path(), selected),
        Err(WindroseWorldTargetError::IdentityMismatch {
            evidence: "WorldDescription islandId",
            ..
        })
    ));

    write_json(&path, &world_document(selected));
    seed_server(root.path(), other);
    assert!(matches!(
        resolve_world_description(root.path(), selected),
        Err(WindroseWorldTargetError::IdentityMismatch {
            evidence: "ServerDescription WorldIslandId",
            ..
        })
    ));
}

#[test]
fn resolver_requires_case_sensitive_native_world_identity_without_writing() {
    let root = TestRoot::new("native-identity-key");
    let world_id = "NATIVE-WORLD";
    seed_server(root.path(), world_id);
    let path = seed_world(root.path(), "0.10.0", world_id, world_id);
    let resolved = resolve_world_description(root.path(), world_id).unwrap();
    assert_eq!(resolved.document["WorldDescription"]["islandId"], world_id);
    let server_before = fs::read(server_path(root.path())).unwrap();
    let mut document = resolved.document;
    let description = document["WorldDescription"].as_object_mut().unwrap();
    let identity = description.remove("islandId").unwrap();
    description.insert("IslandId".into(), identity);
    write_json(&path, &document);
    let world_before = fs::read(&path).unwrap();
    assert!(matches!(
        resolve_world_description(root.path(), world_id),
        Err(WindroseWorldTargetError::InvalidShape { field, .. })
            if field == "WorldDescription.islandId"
    ));
    assert_eq!(fs::read(&path).unwrap(), world_before);
    assert_eq!(fs::read(server_path(root.path())).unwrap(), server_before);
}

#[test]
fn containment_guard_rejects_targets_outside_install_root() {
    let inside = TestRoot::new("inside");
    let outside = TestRoot::new("outside");
    let canonical_inside = fs::canonicalize(inside.path()).unwrap();
    let canonical_outside = fs::canonicalize(outside.path()).unwrap();
    assert!(matches!(
        ensure_within_root(&canonical_inside, &canonical_outside),
        Err(WindroseWorldTargetError::OutsideInstallRoot { .. })
    ));
}

#[test]
fn world_patch_preserves_unknown_fields_and_writes_exact_native_tags() {
    let root = TestRoot::new("unknown-fields");
    let world_id = "E24A22C9C8D3448951AFD002162576D5";
    seed_server(root.path(), world_id);
    let path = seed_world(root.path(), "0.10.0", world_id, world_id);
    patch_world_description(
        resolve_world_description(root.path(), world_id).unwrap(),
        &world_settings(world_id),
    )
    .unwrap();

    let document: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let description = &document["WorldDescription"];
    assert_eq!(document["UnknownWorldRoot"], json!([1, 2, 3]));
    assert_eq!(description["UnknownWorldDescription"]["nested"], "keep-me");
    assert_eq!(description["islandId"], world_id);
    assert!(description.get("IslandId").is_none());
    assert_eq!(description["WorldName"], "海风群岛 ⚓");
    assert_eq!(description["WorldPresetType"], "Medium");
    assert_eq!(
        description["WorldSettings"]["BoolParameters"][COOP_QUESTS_TAG],
        false
    );
    assert_eq!(
        description["WorldSettings"]["FloatParameters"][SHIP_DAMAGE_TAG],
        2.5
    );
    assert_eq!(
        description["WorldSettings"]["FloatParameters"]["UnknownFloat"],
        9.25
    );
    assert_eq!(
        description["WorldSettings"]["TagParameters"][COMBAT_DIFFICULTY_TAG]["TagName"],
        "WDS.Parameter.CombatDifficulty.Hard"
    );
    assert_eq!(
        description["WorldSettings"]["TagParameters"][COMBAT_DIFFICULTY_TAG]["UnknownCombat"],
        "keep-me"
    );
}

#[test]
fn world_patch_populates_documented_parameter_groups_for_a_preset_world() {
    let root = TestRoot::new("preset-world");
    let world_id = "E24A22C9C8D3448951AFD002162576D5";
    seed_server(root.path(), world_id);
    let path = seed_world(root.path(), "0.10.0", world_id, world_id);
    let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    document["WorldDescription"]["WorldSettings"] = json!({});
    fs::write(&path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();

    let resolved = resolve_world_description(root.path(), world_id).unwrap();
    let replacement = render_world_description(&resolved, &world_settings(world_id)).unwrap();
    let updated: Value = serde_json::from_slice(&replacement).unwrap();
    let world_settings = &updated["WorldDescription"]["WorldSettings"];
    assert_eq!(world_settings["BoolParameters"][COOP_QUESTS_TAG], false);
    assert_eq!(world_settings["FloatParameters"][MOB_HEALTH_TAG], 5.0);
    assert_eq!(
        world_settings["TagParameters"][COMBAT_DIFFICULTY_TAG]["TagName"],
        "WDS.Parameter.CombatDifficulty.Hard"
    );
}

#[test]
fn failed_atomic_replacement_leaves_world_byte_for_byte_unchanged() {
    let root = TestRoot::new("replacement-failure");
    let world_id = "E24A22C9C8D3448951AFD002162576D5";
    seed_server(root.path(), world_id);
    let path = seed_world(root.path(), "0.10.0", world_id, world_id);
    let before = fs::read(&path).unwrap();
    let result = patch_world_description_with(
        resolve_world_description(root.path(), world_id).unwrap(),
        &world_settings(world_id),
        |_path, _expected, _replacement| Err(io::Error::other("injected replacement failure")),
    );
    assert!(matches!(
        result,
        Err(WindroseWorldTargetError::Replacement { .. })
    ));
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn validation_failure_happens_before_either_native_file_is_written() {
    let root = TestRoot::new("no-write");
    let config = TestRoot::new("no-write-config");
    let world_id = "E24A22C9C8D3448951AFD002162576D5";
    seed_server(root.path(), world_id);
    let world_path = seed_world(root.path(), "0.10.0", world_id, world_id);
    seed_rendered_server(config.path(), world_id, "After");
    let server_before = fs::read(server_path(root.path())).unwrap();
    let world_before = fs::read(&world_path).unwrap();
    let mut invalid_settings = world_settings(world_id);
    invalid_settings.remove("world_name");

    assert!(matches!(
        materialize_windrose_documents(root.path(), config.path(), &invalid_settings, false),
        Err(WindroseWorldTargetError::InvalidSetting { key: "world_name" })
    ));
    assert_eq!(fs::read(server_path(root.path())).unwrap(), server_before);
    assert_eq!(fs::read(world_path).unwrap(), world_before);
}

#[test]
fn save_stage_writes_a_pending_plan_without_touching_native_documents() {
    let root = TestRoot::new("materialize");
    let config = TestRoot::new("materialize-config");
    let world_id = "E24A22C9C8D3448951AFD002162576D5";
    seed_server(root.path(), world_id);
    let world_path = seed_world(root.path(), "0.10.0", world_id, world_id);
    seed_rendered_server(config.path(), world_id, "After");

    let server_before = fs::read(server_path(root.path())).unwrap();
    let world_before = fs::read(&world_path).unwrap();
    materialize_windrose_documents(root.path(), config.path(), &world_settings(world_id), false)
        .unwrap();

    assert_eq!(fs::read(server_path(root.path())).unwrap(), server_before);
    assert_eq!(fs::read(&world_path).unwrap(), world_before);
    let plan: Value = serde_json::from_slice(
        &fs::read(config.path().join(WINDROSE_WORLD_UPDATE_PLAN_FILE)).unwrap(),
    )
    .unwrap();
    assert_eq!(plan["pending"], true);
    assert_eq!(plan["world_island_id"], world_id);
    assert_eq!(plan["world_parameters"]["world_name"], "海风群岛 ⚓");
    assert_eq!(
        plan["world_description_relative_path"],
        world_path
            .strip_prefix(root.path())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/")
    );
}

#[test]
fn server_description_remains_editable_without_a_selected_world() {
    let root = TestRoot::new("server-only");
    let config = TestRoot::new("server-only-config");
    fs::create_dir_all(root.path().join("R5")).unwrap();
    seed_rendered_server(config.path(), "", "Server only");
    let settings = json!({ "world_island_id": "" })
        .as_object()
        .unwrap()
        .clone();

    materialize_windrose_documents(root.path(), config.path(), &settings, true).unwrap();

    let server: Value =
        serde_json::from_slice(&fs::read(server_path(root.path())).unwrap()).unwrap();
    assert_eq!(
        server["ServerDescription_Persistent"]["ServerName"],
        "Server only"
    );
    assert!(
        server["ServerDescription_Persistent"]
            .get("WorldIslandId")
            .is_none()
    );
}

#[test]
fn running_instance_rejects_a_world_change_before_native_writes() {
    let root = TestRoot::new("running-rejection");
    let config = TestRoot::new("running-rejection-config");
    let world_id = "E24A22C9C8D3448951AFD002162576D5";
    seed_server(root.path(), world_id);
    let world_path = seed_world(root.path(), "0.10.0", world_id, world_id);
    seed_rendered_server(config.path(), world_id, "After");
    let server_before = fs::read(server_path(root.path())).unwrap();
    let world_before = fs::read(&world_path).unwrap();

    assert!(matches!(
        materialize_windrose_documents(root.path(), config.path(), &world_settings(world_id), true,),
        Err(WindroseWorldTargetError::RunningWorldMutation)
    ));
    assert_eq!(fs::read(server_path(root.path())).unwrap(), server_before);
    assert_eq!(fs::read(world_path).unwrap(), world_before);
    assert!(!config.path().join(WINDROSE_WORLD_UPDATE_PLAN_FILE).exists());
}

#[test]
fn running_instance_allows_server_only_change_when_world_is_unchanged() {
    let root = TestRoot::new("running-server-only");
    let config = TestRoot::new("running-server-only-config");
    let world_id = "E24A22C9C8D3448951AFD002162576D5";
    seed_server(root.path(), world_id);
    let world_path = seed_world(root.path(), "0.10.0", world_id, world_id);
    seed_rendered_server(config.path(), world_id, "Server changed while running");
    let world_before = fs::read(&world_path).unwrap();
    let mut settings = world_settings(world_id);
    settings.extend(
        json!({
            "world_name": "Before",
            "world_preset_type": "Medium",
            "coop_quests": true,
            "easy_explore": false,
            "mob_health_multiplier": 1.0,
            "mob_damage_multiplier": 1.0,
            "ship_health_multiplier": 1.0,
            "ship_damage_multiplier": 1.0,
            "boarding_difficulty_multiplier": 1.0,
            "coop_stats_correction_modifier": 1.0,
            "coop_ship_stats_correction_modifier": 0.0,
            "combat_difficulty": "Normal"
        })
        .as_object()
        .unwrap()
        .clone(),
    );

    materialize_windrose_documents(root.path(), config.path(), &settings, true).unwrap();
    assert_eq!(fs::read(world_path).unwrap(), world_before);
    let server: Value =
        serde_json::from_slice(&fs::read(server_path(root.path())).unwrap()).unwrap();
    assert_eq!(
        server["ServerDescription_Persistent"]["ServerName"],
        "Server changed while running"
    );
}
