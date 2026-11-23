use super::*;
use crate::instance_settings_lock::acquire_instance_settings_mutation_lock;

#[tokio::test]
async fn barotrauma_runtime_is_prepared_only_for_stopped_prestart() {
    let root = std::env::temp_dir().join(format!("barotrauma-prestart-{}", uuid::Uuid::new_v4()));
    let paths = StoragePaths {
        app_data_root: root.join("app-data"),
        settings_path: root.join("app-data/settings.json"),
        database_path: root.join("app-data/db/lgs.db"),
        logs_root: root.join("logs"),
        modules_root: root.join("modules"),
        migrations_root: root.join("migrations"),
        steamcmd_root: root.join("steamcmd"),
        games_root: root.join("games"),
        instances_root: root.join("instances"),
        archives_root: root.join("instances").join(".trash"),
    };
    let install = paths.games_root.join("barotrauma");
    let config = paths.instances_root.join("one/config");
    let saves = config.join("Multiplayer");
    fs::create_dir_all(install.join("Data")).unwrap();
    fs::create_dir_all(install.join("Content")).unwrap();
    fs::create_dir_all(&saves).unwrap();
    fs::write(install.join("DedicatedServer.exe"), "program").unwrap();
    fs::write(install.join("Content/Vanilla.xml"), "content").unwrap();
    fs::write(saves.join("world.save"), "retained").unwrap();
    let settings = Map::new();
    let context = ModuleSupportMaterializationContext {
        storage_paths: &paths,
        module_id: "barotrauma",
        install_root: &install,
        shared_install_root: &install,
        config_dir: &config,
        saves_dir: &saves,
        instance_id: "one",
        instance_running: false,
        settings: &settings,
    };
    let lock = acquire_instance_settings_mutation_lock(&paths, "one").unwrap();
    super::super::templates_materialize::materialize_module_support_files(&context).unwrap();
    assert!(!config.join("DedicatedServer.exe").exists());
    assert!(
        apply_module_prestart_support(
            &ModuleSupportMaterializationContext {
                instance_running: true,
                ..context
            },
            &lock,
        )
        .await
        .is_err()
    );
    assert!(!config.join("DedicatedServer.exe").exists());
    apply_module_prestart_support(&context, &lock)
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(config.join("DedicatedServer.exe")).unwrap(),
        "program"
    );
    assert_eq!(
        fs::read_to_string(config.join("Content/Vanilla.xml")).unwrap(),
        "content"
    );
    assert_eq!(
        fs::read_to_string(saves.join("world.save")).unwrap(),
        "retained"
    );
    drop(lock);
    fs::remove_dir_all(root).unwrap();
}
