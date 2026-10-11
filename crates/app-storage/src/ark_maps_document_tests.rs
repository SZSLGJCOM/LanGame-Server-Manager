use super::*;
use crate::templates::{
    InstanceConfigInput, ModuleSupportMaterializationContext, write_pending_instance_configuration,
};
use serde_json::json;
use std::fs;
use std::path::PathBuf;

fn legacy_document(module_id: &str) -> Value {
    json!({
        "module_id": module_id, "instance_id": "names", "instance_name": "Fallback room",
        "settings": {
            "server_name": "Original room", "map_name": "OriginalMain",
            "untouched": {"value": 7},
            "additional_maps": [
                {"id": "one", "map_name": "SecondMap", "name": "Second", "enabled": true},
                {"id": "paused", "map_name": "PausedMap", "name": "Paused", "enabled": false}
            ]
        }
    })
}

#[test]
fn ark_legacy_document_retains_published_names_for_both_editions_and_disabled_maps() {
    for edition in ["arksurvivalascended", "arksurvivalevolved"] {
        for version in [None, Some(json!(0))] {
            let mut document = legacy_document(edition);
            if let Some(version) = version {
                document["ark_server_names_version"] = version;
            }
            let original = document.clone();
            let settings = decode_document_settings(&document).unwrap();
            assert_eq!(settings["server_name"], "Original room | OriginalMain");
            assert_eq!(
                settings["additional_maps"][0]["name"],
                "Original room | Second"
            );
            assert_eq!(
                settings["additional_maps"][1]["name"],
                "Original room | Paused"
            );
            assert_eq!(settings["additional_maps"][1]["enabled"], false);
            assert_eq!(settings["untouched"], document["settings"]["untouched"]);
            assert_eq!(
                document, original,
                "decoding must not mutate its source document"
            );
        }
    }
}

#[test]
fn ark_legacy_document_uses_instance_name_and_edition_main_package_when_absent() {
    for (edition, main) in [
        ("arksurvivalascended", "TheIsland_WP"),
        ("arksurvivalevolved", "TheIsland"),
    ] {
        let mut document = legacy_document(edition);
        let settings = document["settings"].as_object_mut().unwrap();
        settings.remove("server_name");
        settings.remove("map_name");
        settings["additional_maps"].as_array_mut().unwrap()[0]["enabled"] = json!(false);
        let settings = decode_document_settings(&document).unwrap();
        assert_eq!(settings["server_name"], format!("Fallback room | {main}"));
        assert_eq!(
            settings["additional_maps"][0]["name"],
            "Fallback room | Second"
        );
    }
}

#[test]
fn ark_legacy_document_preserves_long_and_untrimmed_published_base() {
    let mut document = legacy_document("arksurvivalascended");
    let base = format!("  {}  ", "Long room ".repeat(12));
    document["settings"]["server_name"] = json!(base);
    let settings = decode_document_settings(&document).unwrap();
    assert_eq!(settings["server_name"], format!("{base} | OriginalMain"));
    assert_eq!(
        settings["additional_maps"][0]["name"],
        format!("{base} | Second")
    );
    assert!(
        settings["additional_maps"][0]["name"]
            .as_str()
            .unwrap()
            .chars()
            .count()
            > 80
    );
    document["settings"] = settings.clone();
    document["ark_server_names_version"] = json!(1);
    assert_eq!(decode_document_settings(&document).unwrap(), settings);
}

#[test]
fn ark_document_decode_is_repeatable_and_current_version_does_not_compose_names_again() {
    let document = legacy_document("arksurvivalevolved");
    let settings = decode_document_settings(&document).unwrap();
    assert_eq!(decode_document_settings(&document).unwrap(), settings);
    let mut current = document.clone();
    current["settings"] = settings.clone();
    current["ark_server_names_version"] = json!(1);
    assert_eq!(decode_document_settings(&current).unwrap(), settings);
    current["settings"]["server_name"] = json!("Main complete name");
    current["settings"]["additional_maps"][0]["name"] = json!("Independent complete name");
    assert_eq!(
        decode_document_settings(&current).unwrap(),
        current["settings"]
    );
}

#[test]
fn ark_documents_without_additional_maps_and_non_ark_documents_keep_settings() {
    for maps in [None, Some(json!([]))] {
        let mut document = legacy_document("arksurvivalascended");
        let settings = document["settings"].as_object_mut().unwrap();
        settings.remove("additional_maps");
        if let Some(maps) = maps {
            settings.insert("additional_maps".into(), maps);
        }
        assert_eq!(
            decode_document_settings(&document).unwrap(),
            document["settings"]
        );
    }
    let mut document = legacy_document("palworld");
    document["ark_server_names_version"] = json!(99);
    assert_eq!(
        decode_document_settings(&document).unwrap(),
        document["settings"]
    );
    document["module_id"] = json!("ARKSurvivalAscended");
    assert_eq!(
        decode_document_settings(&document).unwrap(),
        document["settings"]
    );
}

#[test]
fn ark_document_unknown_or_invalid_name_versions_are_rejected() {
    for version in [
        json!(2),
        json!(-1),
        json!("1"),
        json!(null),
        json!(1.5),
        json!(true),
    ] {
        let mut document = legacy_document("arksurvivalascended");
        document["ark_server_names_version"] = version;
        assert!(
            matches!(decode_document_settings(&document), Err(StorageError::InvalidModuleSetting { field, .. }) if field == "ark_server_names_version")
        );
    }
}

#[test]
fn ark_document_malformed_settings_and_legacy_name_fields_are_rejected() {
    assert!(decode_document_settings(&json!([])).is_err());
    for settings in [json!(null), json!([]), json!("settings")] {
        let mut document = legacy_document("arksurvivalascended");
        document["settings"] = settings;
        assert!(decode_document_settings(&document).is_err());
    }
    for (field, value) in [
        ("additional_maps", json!({})),
        ("additional_maps", json!([{}])),
        ("server_name", json!(23)),
        ("map_name", json!(null)),
    ] {
        let mut document = legacy_document("arksurvivalascended");
        document["settings"][field] = value;
        assert!(
            decode_document_settings(&document).is_err(),
            "invalid {field} must not be silently renamed"
        );
    }
    let mut document = legacy_document("arksurvivalascended");
    document["settings"]
        .as_object_mut()
        .unwrap()
        .remove("server_name");
    document["instance_name"] = json!(null);
    assert!(decode_document_settings(&document).is_err());
}

struct ConfigFixture {
    root: PathBuf,
    paths: crate::StoragePaths,
    config: PathBuf,
}

impl ConfigFixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("ark-document-names-{}", uuid::Uuid::new_v4()));
        let config = root.join("instances/names/config");
        fs::create_dir_all(&config).unwrap();
        let paths = crate::StoragePaths {
            app_data_root: root.join("appdata"),
            settings_path: root.join("appdata/settings.json"),
            database_path: root.join("appdata/db/lgs.db"),
            logs_root: root.join("logs"),
            modules_root: root.join("modules"),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances/.trash"),
        };
        Self {
            root,
            paths,
            config,
        }
    }

    fn write(&self, module_id: &str, settings: &Value) -> crate::templates::ManagedConfigMutation {
        let _lease = crate::instance_settings_lock::acquire_instance_settings_mutation_lock(
            &self.paths,
            "names",
        )
        .unwrap();
        let runtime = self.root.join("instances/names/runtime");
        let saves = runtime.join("ShooterGame/Saved");
        let settings = settings.as_object().unwrap().clone();
        write_pending_instance_configuration(
            &self.root.join("templates"),
            &crate::templates::ModuleTemplateRenderInput {
                config_dir: &self.config,
                install_root: &runtime,
                saves_dir: &saves,
                instance_id: "names",
                instance_name: "Fallback room",
                module_id,
                bind_ip: "0.0.0.0",
                autostart: false,
                settings: &settings,
                ports: &[],
            },
            &ModuleSupportMaterializationContext {
                storage_paths: &self.paths,
                module_id,
                install_root: &runtime,
                shared_install_root: &runtime,
                config_dir: &self.config,
                saves_dir: &saves,
                instance_id: "names",
                instance_running: true,
                settings: &settings,
            },
            &self.config.join("instance.json"),
            InstanceConfigInput {
                instance_id: "names",
                instance_name: "Fallback room",
                module_id,
                bind_ip: "0.0.0.0",
                autostart: false,
                settings: settings.clone(),
                ports: &[],
            },
            None,
        )
        .unwrap()
    }
}

impl Drop for ConfigFixture {
    fn drop(&mut self) {
        assert!(self.root.starts_with(std::env::temp_dir()));
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn ark_legacy_read_keeps_bytes_and_normal_managed_write_persists_current_names_once() {
    for edition in ["arksurvivalascended", "arksurvivalevolved"] {
        let fixture = ConfigFixture::new();
        let config = fixture.config.join("instance.json");
        let original = serde_json::to_vec_pretty(&legacy_document(edition)).unwrap();
        fs::write(&config, &original).unwrap();
        let before_modified = fs::metadata(&config).unwrap().modified().unwrap();
        let decoded: Value =
            serde_json::from_str(&crate::instances::read_instance_settings_json(&config).unwrap())
                .unwrap();
        assert_eq!(decoded["server_name"], "Original room | OriginalMain");
        assert_eq!(fs::read(&config).unwrap(), original);
        assert_eq!(
            fs::metadata(&config).unwrap().modified().unwrap(),
            before_modified
        );
        fixture.write(edition, &decoded).commit();
        let persisted: Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        assert_eq!(persisted["ark_server_names_version"], 1);
        assert_eq!(persisted["settings"]["server_name"], decoded["server_name"]);
        assert_eq!(
            persisted["settings"]["additional_maps"],
            decoded["additional_maps"]
        );
        let readback: Value =
            serde_json::from_str(&crate::instances::read_instance_settings_json(&config).unwrap())
                .unwrap();
        assert_eq!(readback, persisted["settings"]);
        fixture.write(edition, &readback).commit();
        let reread: Value =
            serde_json::from_str(&crate::instances::read_instance_settings_json(&config).unwrap())
                .unwrap();
        assert_eq!(
            reread, readback,
            "a second write/read must not compose names again"
        );
    }
}

#[test]
fn ark_new_managed_document_has_current_version_and_rollback_keeps_legacy_source() {
    let fixture = ConfigFixture::new();
    let config = fixture.config.join("instance.json");
    let current = json!({"server_name": "Main direct name", "additional_maps": []});
    fixture.write("arksurvivalascended", &current).commit();
    let document: Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    assert_eq!(document["ark_server_names_version"], 1);
    assert_eq!(document["settings"]["server_name"], "Main direct name");
    let original = serde_json::to_vec_pretty(&legacy_document("arksurvivalascended")).unwrap();
    fs::write(&config, &original).unwrap();
    let settings: Value =
        serde_json::from_str(&crate::instances::read_instance_settings_json(&config).unwrap())
            .unwrap();
    fixture
        .write("arksurvivalascended", &settings)
        .rollback()
        .unwrap();
    assert_eq!(fs::read(&config).unwrap(), original);
}

#[test]
fn ark_reader_rejects_invalid_json_without_touching_the_file() {
    let fixture = ConfigFixture::new();
    let config = fixture.config.join("instance.json");
    let invalid = b"{ malformed";
    fs::write(&config, invalid).unwrap();
    assert!(matches!(
        crate::instances::read_instance_settings_json(&config),
        Err(StorageError::InvalidConfigJson { .. })
    ));
    assert_eq!(fs::read(&config).unwrap(), invalid);
}
