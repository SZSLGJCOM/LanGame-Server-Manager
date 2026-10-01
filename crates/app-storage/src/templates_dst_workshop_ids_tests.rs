use super::*;
use crate::settings_validation::{SettingsValidationPhase, collect_settings_schema_diagnostics};

#[test]
fn dst_workshop_ids_csv_reaches_shared_subscriptions_and_both_shards() {
    let schema: Value =
        serde_json::from_str(include_str!("../../../modules/dontstarve/schema.json"))
            .expect("DST schema");
    let mut settings = dst_world_schema_defaults();
    settings.insert("cluster_name".to_owned(), json!("CSV Workshop fixture"));
    for key in [
        "shared_workshop_mod_ids",
        "master_enabled_workshop_mod_ids",
        "caves_enabled_workshop_mod_ids",
    ] {
        settings.insert(key.to_owned(), json!("2039181790,1392778117"));
    }
    settings.insert(
        "shared_workshop_collection_ids".to_owned(),
        json!("1111111111,2222222222"),
    );
    let diagnostics =
        collect_settings_schema_diagnostics(&schema, &settings, SettingsValidationPhase::Creation);
    assert!(diagnostics.is_empty(), "CSV is accepted: {diagnostics:?}");

    for shard in ["master", "caves"] {
        let rendered = render_dst_modoverrides(&settings, shard);
        for id in ["2039181790", "1392778117"] {
            let entry = format!("[\"workshop-{id}\"] = {{ enabled = true }}");
            assert_eq!(rendered.matches(&entry).count(), 1, "{shard}: {rendered}");
        }
    }

    let root = unique_dst_test_root();
    let paths = dst_test_storage_paths(&root);
    let install_root = paths.instances_root.join("csv-mods/runtime");
    write_dst_instance_config(&paths, "csv-mods", &settings).commit();
    let setup = fs::read_to_string(install_root.join("mods/dedicated_server_mods_setup.lua"))
        .expect("instance subscriptions used by both shards");
    for (function, ids) in [
        ("ServerModSetup", ["2039181790", "1392778117"]),
        ("ServerModCollectionSetup", ["1111111111", "2222222222"]),
    ] {
        for id in ids {
            assert_eq!(
                setup.matches(&format!("{function}(\"{id}\")")).count(),
                1,
                "{setup}"
            );
        }
    }
    fs::remove_dir_all(root).expect("remove isolated test data");
}

#[test]
fn dst_workshop_ids_preserve_urls_newlines_comments_and_deduplication() {
    for raw in [
        "# ignored 9999999999,8888888888\r\n-- ignored 7777777777,6666666666\n\
         https://steamcommunity.com/sharedfiles/filedetails/?id=2039181790&searchtext=\r\n\
         workshop-1392778117\n2039181790\n",
        "# ignored 9999999999,8888888888\r\n-- ignored 7777777777,6666666666\n\
         https://steamcommunity.com/sharedfiles/filedetails/?id=2039181790&searchtext=, \
         workshop-1392778117,,2039181790\n",
    ] {
        let settings = Map::from_iter([("ids".to_owned(), json!(raw))]);
        assert_eq!(
            parse_workshop_id_list(&settings, "ids"),
            ["2039181790", "1392778117"],
            "{raw}"
        );
    }
}
