use super::*;
use crate::{
    create_instance, initialize_database, materialize_instance_configuration_for_start,
    read_instance_details, sync_modules, update_instance,
};
use app_core::{CreateInstanceInput, InstanceDetails, InstanceProvisioning, UpdateInstanceInput};
use serde_json::{Value, json};
use std::fs;

#[path = "instance_isolation_archive_tests.rs"]
mod archive_roots;

struct Fixture {
    root: PathBuf,
    storage: StoragePaths,
}
impl Fixture {
    async fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lgsm-isolation-{}", uuid::Uuid::new_v4()));
        let storage = StoragePaths {
            app_data_root: root.join("appdata"),
            settings_path: root.join("appdata/settings.json"),
            database_path: root.join("appdata/db/lgs.db"),
            logs_root: root.join("appdata/logs"),
            modules_root: root.join("modules"),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances").join(".trash"),
        };
        for directory in [
            &storage.app_data_root,
            storage.database_path.parent().unwrap(),
            &storage.logs_root,
            &storage.modules_root,
            &storage.games_root,
            &storage.instances_root,
        ] {
            fs::create_dir_all(directory).unwrap();
        }
        initialize_database(&storage).await.unwrap();
        Self { root, storage }
    }
    async fn module(&self, id: &str, saves: &str) -> ModuleDescriptor {
        let module = self.storage.modules_root.join(id);
        fs::create_dir_all(module.join("templates")).unwrap();
        fs::write(module.join("module.toml"), format!(
            "id = {id:?}\nname = {id:?}\nversion = \"1.0.0\"\nsupported_platforms = [\"windows\"]\n[storage]\nsaves_path_template = {saves:?}\n"
        )).unwrap();
        fs::write(module.join("schema.json"), json!({"type":"object","properties":{"label":{"type":"string","default":"original"},"world":{"type":"string","default":"world"},"cluster_id":{"type":"string","default":""},"cluster_directory":{"type":"string","default":""}}}).to_string()).unwrap();
        if id == "arksurvivalevolved" {
            for (name, content) in [
                ("Game.ini.hbs", "[/Script/ShooterGame.ShooterGameMode]\n"),
                ("GameUserSettings.ini.hbs", "[ServerSettings]\n"),
                ("AllowedCheaterSteamIDs.txt.hbs", ""),
                ("PlayersExclusiveJoinList.txt.hbs", ""),
                ("PlayersJoinNoCheckList.txt.hbs", ""),
            ] {
                fs::write(module.join("templates").join(name), content).unwrap();
            }
        }
        let shared = self.storage.games_root.join(id);
        fs::create_dir_all(&shared).unwrap();
        fs::write(shared.join("package.fixture"), b"owned test package").unwrap();
        let descriptor = discover_modules(&self.storage.modules_root)
            .unwrap()
            .into_iter()
            .find(|module| module.summary.id == id)
            .unwrap();
        sync_modules(&self.storage, std::slice::from_ref(&descriptor))
            .await
            .unwrap();
        descriptor
    }
    async fn create(&self, descriptor: &ModuleDescriptor, name: &str) -> InstanceProvisioning {
        let download = self.storage.games_root.join(&descriptor.summary.id);
        if !download.exists() {
            // Model the caller's clean installation preparation after adoption,
            // without inheriting the first instance's settings, saves or mods.
            fs::create_dir_all(&download).unwrap();
            fs::write(download.join("package.fixture"), b"owned test package").unwrap();
        }
        create_instance(
            &self.storage,
            descriptor,
            CreateInstanceInput {
                name: name.into(),
                module_id: descriptor.summary.id.clone(),
            },
        )
        .await
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn runtime_root(instance: &InstanceProvisioning) -> PathBuf {
    Path::new(&instance.config_file_path)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("runtime")
}

fn update_input(details: &InstanceDetails) -> UpdateInstanceInput {
    let mut settings: Value = serde_json::from_str(&details.settings_json).unwrap();
    settings["label"] = json!("changed");
    UpdateInstanceInput {
        id: details.summary.id.clone(),
        bind_ip: details.summary.bind_ip.clone(),
        auto_backup_on_stop: details.auto_backup_on_stop,
        backup_retention_count: details.backup_retention_count,
        settings_json: settings.to_string(),
        ports: details.ports.clone(),
    }
}

#[tokio::test]
async fn reports_private_and_missing_runtime_without_exposing_settings() {
    let fixture = Fixture::new().await;
    let descriptor = fixture
        .module("fixture", "{{paths.config_dir}}/world")
        .await;
    let first = fixture.create(&descriptor, "One").await;
    let report = read_instance_isolation(&fixture.storage, &first.summary.id)
        .await
        .unwrap();
    assert_eq!(report.mode, "private");
    assert_eq!(Path::new(&report.runtime_path), runtime_root(&first));
    assert_eq!(
        Path::new(&report.data_path),
        runtime_root(&first).parent().unwrap()
    );
    assert!(report.conflicts.is_empty());
    let config = Path::new(&first.config_file_path);
    let mut document: Value = serde_json::from_slice(&fs::read(config).unwrap()).unwrap();
    document["settings"]["token"] = json!("secret-never-return");
    fs::write(config, document.to_string()).unwrap();
    let changed = read_instance_isolation(&fixture.storage, &first.summary.id)
        .await
        .unwrap();
    assert_eq!(changed.mode, "private");
    assert!(
        !serde_json::to_string(&changed)
            .unwrap()
            .contains("secret-never-return")
    );
    let runtime = runtime_root(&first);
    fs::rename(&runtime, runtime.with_file_name("runtime.unavailable")).unwrap();
    let damaged = read_instance_isolation(&fixture.storage, &first.summary.id)
        .await
        .unwrap();
    assert_eq!(damaged.mode, "damaged");
    assert!(!damaged.issues.is_empty());
    assert!(
        !serde_json::to_string(&damaged)
            .unwrap()
            .contains("secret-never-return")
    );
    assert!(matches!(
        read_instance_details(&fixture.storage, &first.summary.id)
            .await
            .unwrap_err(),
        StorageError::PrivateRuntimeRefresh { .. }
    ));
    assert!(!runtime.exists());
}

#[tokio::test]
async fn independent_runtimes_reject_overlapping_worlds_before_save_or_start_writes() {
    let fixture = Fixture::new().await;
    let descriptor = fixture
        .module("fixture", "{{paths.config_dir}}/world")
        .await;
    let first = fixture.create(&descriptor, "One").await;
    let second = fixture.create(&descriptor, "Two").await;
    let report = read_instance_isolation(&fixture.storage, &first.summary.id)
        .await
        .unwrap();
    assert_eq!(report.mode, "private");
    assert!(report.conflicts.is_empty());
    let details = read_instance_details(&fixture.storage, &first.summary.id)
        .await
        .unwrap();
    let second_before = fs::read(&second.config_file_path).unwrap();
    update_instance(&fixture.storage, update_input(&details))
        .await
        .unwrap();
    materialize_instance_configuration_for_start(&fixture.storage, &first.summary.id)
        .await
        .unwrap();
    assert_eq!(fs::read(&second.config_file_path).unwrap(), second_before);
    let world = fixture.root.join("overlapping-world");
    fs::create_dir_all(&world).unwrap();
    fs::write(world.join("sentinel"), b"existing world").unwrap();
    fixture
        .module("fixture", &world.to_string_lossy().replace('\\', "/"))
        .await;
    let before = fs::read(&first.config_file_path).unwrap();
    let report = read_instance_isolation(&fixture.storage, &first.summary.id)
        .await
        .unwrap();
    assert_eq!(report.mode, "private");
    assert!(
        report
            .conflicts
            .iter()
            .any(|conflict| conflict.kind == "saves" && conflict.instance_id == second.summary.id)
    );
    assert!(
        !report
            .conflicts
            .iter()
            .any(|conflict| conflict.kind == "runtime")
    );
    for error in [
        update_instance(&fixture.storage, update_input(&details))
            .await
            .unwrap_err(),
        materialize_instance_configuration_for_start(&fixture.storage, &first.summary.id)
            .await
            .unwrap_err(),
    ] {
        assert!(
            matches!(error, StorageError::InstancePathConflict { ref kind, .. } if kind == "saves"),
            "{error}"
        );
    }
    assert_eq!(fs::read(&first.config_file_path).unwrap(), before);
    assert_eq!(fs::read(&second.config_file_path).unwrap(), second_before);
    assert_eq!(fs::read(world.join("sentinel")).unwrap(), b"existing world");
    assert!(runtime_root(&first).is_dir());
    assert!(runtime_root(&second).is_dir());
}

#[tokio::test]
async fn create_checks_cross_module_save_ownership_before_creating_world_files() {
    let fixture = Fixture::new().await;
    let world = fixture.root.join("world");
    let first_module = fixture
        .module("one", &world.to_string_lossy().replace('\\', "/"))
        .await;
    let second_module = fixture
        .module(
            "two",
            &world.join("nested").to_string_lossy().replace('\\', "/"),
        )
        .await;
    let first = fixture.create(&first_module, "One").await;
    fs::write(world.join("sentinel"), b"existing world").unwrap();
    let error = create_instance(
        &fixture.storage,
        &second_module,
        CreateInstanceInput {
            name: "Two".into(),
            module_id: "two".into(),
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, StorageError::InstancePathConflict { ref other_instance_id, .. } if other_instance_id == &first.summary.id),
        "{error}"
    );
    assert_eq!(fs::read(world.join("sentinel")).unwrap(), b"existing world");
    assert!(!world.join("nested").exists());
}

#[tokio::test]
async fn incoming_traversal_is_rejected_without_overwriting_saved_settings() {
    let fixture = Fixture::new().await;
    let descriptor = fixture
        .module("fixture", "{{paths.instance_root}}/{{settings.world}}")
        .await;
    let first = fixture.create(&descriptor, "One").await;
    let details = read_instance_details(&fixture.storage, &first.summary.id)
        .await
        .unwrap();
    let before = fs::read(&first.config_file_path).unwrap();
    let mut input = update_input(&details);
    let mut settings: Value = serde_json::from_str(&input.settings_json).unwrap();
    settings["world"] = json!("../shared-world");
    input.settings_json = settings.to_string();
    assert!(matches!(
        update_instance(&fixture.storage, input).await.unwrap_err(),
        StorageError::InvalidInstancePath { .. }
    ));
    assert_eq!(fs::read(&first.config_file_path).unwrap(), before);
}

#[tokio::test]
async fn intentional_ark_cluster_sharing_does_not_share_world_ownership() {
    let fixture = Fixture::new().await;
    let descriptor = fixture
        .module(
            "arksurvivalevolved",
            "{{paths.install_root}}/ShooterGame/Saved/{{instance.id}}",
        )
        .await;
    let first = fixture.create(&descriptor, "One").await;
    let second = fixture.create(&descriptor, "Two").await;
    let cluster = fixture.root.join("shared-cluster");
    fs::create_dir_all(&cluster).unwrap();
    fs::write(
        cluster.join("uploaded-character"),
        b"preserve cluster transfer",
    )
    .unwrap();
    for instance in [&first, &second] {
        let details = read_instance_details(&fixture.storage, &instance.summary.id)
            .await
            .unwrap();
        let mut input = update_input(&details);
        let mut settings: Value = serde_json::from_str(&input.settings_json).unwrap();
        settings["cluster_id"] = json!("intentional-sharing");
        settings["cluster_directory"] = json!(cluster);
        input.settings_json = settings.to_string();
        update_instance(&fixture.storage, input).await.unwrap();
        materialize_instance_configuration_for_start(&fixture.storage, &instance.summary.id)
            .await
            .unwrap();
        let report = read_instance_isolation(&fixture.storage, &instance.summary.id)
            .await
            .unwrap();
        assert_eq!(report.mode, "private");
        assert!(report.conflicts.is_empty(), "{report:?}");
        assert!(Path::new(&report.saves_path).starts_with(runtime_root(instance)));
        let saved = read_instance_details(&fixture.storage, &instance.summary.id)
            .await
            .unwrap();
        let settings: Value = serde_json::from_str(&saved.settings_json).unwrap();
        assert_eq!(settings["cluster_id"], "intentional-sharing");
        assert_eq!(settings["cluster_directory"], json!(cluster));
    }
    assert_ne!(runtime_root(&first), runtime_root(&second));
    assert_eq!(
        fs::read(cluster.join("uploaded-character")).unwrap(),
        b"preserve cluster transfer"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn windows_aliases_and_junctions_cannot_hide_conflicts() {
    use std::os::windows::process::CommandExt;
    let fixture = Fixture::new().await;
    let original = fixture.root.join("World");
    fs::create_dir_all(&original).unwrap();
    assert!(overlaps(
        &normalize_path(&original).unwrap(),
        &normalize_path(&fixture.root.join("WORLD/child")).unwrap()
    ));
    assert!(!overlaps(
        &normalize_path(&original).unwrap(),
        &normalize_path(&fixture.root.join("World-other")).unwrap()
    ));
    let junction = fixture.root.join("world-link");
    let result = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&original)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    let checked = normalize_path(&junction.join("uncreated/world"));
    fs::remove_dir(&junction).unwrap();
    assert!(matches!(
        checked,
        Err(StorageError::InvalidInstancePath { .. })
    ));
    assert!(original.is_dir());
}

#[tokio::test]
async fn peer_save_cannot_claim_native_configuration_of_an_independent_runtime() {
    let fixture = Fixture::new().await;
    let descriptor = fixture
        .module("palworld", "{{paths.config_dir}}/world")
        .await;
    let peer_module = fixture.module("peer", "{{paths.config_dir}}/world").await;
    let first = fixture.create(&descriptor, "One").await;
    let second = fixture.create(&peer_module, "Two").await;
    let native = runtime_root(&first).join("Pal/Saved/Config/WindowsServer/PalWorldSettings.ini");
    fs::create_dir_all(native.parent().unwrap()).unwrap();
    fs::write(&native, b"original native configuration").unwrap();
    fixture
        .module(
            "peer",
            &native
                .parent()
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/"),
        )
        .await;
    let report = read_instance_isolation(&fixture.storage, &first.summary.id)
        .await
        .unwrap();
    assert_eq!(report.mode, "private");
    assert!(
        report
            .conflicts
            .iter()
            .any(|conflict| conflict.kind == "configuration"
                && conflict.instance_id == second.summary.id
                && conflict.path.contains("WindowsServer"))
    );
    assert!(
        !report
            .conflicts
            .iter()
            .any(|conflict| conflict.kind == "saves" || conflict.kind == "runtime")
    );
    let details = read_instance_details(&fixture.storage, &first.summary.id)
        .await
        .unwrap();
    let before = fs::read(&first.config_file_path).unwrap();
    let peer_before = fs::read(&second.config_file_path).unwrap();
    for error in [
        update_instance(&fixture.storage, update_input(&details))
            .await
            .unwrap_err(),
        materialize_instance_configuration_for_start(&fixture.storage, &first.summary.id)
            .await
            .unwrap_err(),
    ] {
        assert!(
            matches!(error, StorageError::InstancePathConflict { ref kind, .. } if kind == "configuration"),
            "{error}"
        );
    }
    assert_eq!(fs::read(&native).unwrap(), b"original native configuration");
    assert_eq!(fs::read(&first.config_file_path).unwrap(), before);
    assert_eq!(fs::read(&second.config_file_path).unwrap(), peer_before);
    assert!(runtime_root(&first).is_dir());
    assert!(runtime_root(&second).is_dir());
}

#[tokio::test]
async fn simultaneous_cross_module_creation_cannot_claim_one_world_twice() {
    let fixture = Fixture::new().await;
    let world = fixture
        .root
        .join("world")
        .to_string_lossy()
        .replace('\\', "/");
    let one = fixture.module("one", &world).await;
    let two = fixture.module("two", &world).await;
    let (first, second) = tokio::join!(
        create_instance(
            &fixture.storage,
            &one,
            CreateInstanceInput {
                name: "One".into(),
                module_id: "one".into()
            }
        ),
        create_instance(
            &fixture.storage,
            &two,
            CreateInstanceInput {
                name: "Two".into(),
                module_id: "two".into()
            }
        ),
    );
    assert_eq!(
        usize::from(first.is_ok()) + usize::from(second.is_ok()),
        1,
        "{first:?}; {second:?}"
    );
    let error = first.err().or_else(|| second.err()).unwrap();
    assert!(
        matches!(error, StorageError::InstancePathConflict { .. }),
        "{error}"
    );
}

#[tokio::test]
async fn invalid_or_missing_runtime_marker_is_damaged_without_touching_files() {
    let fixture = Fixture::new().await;
    let descriptor = fixture
        .module("fixture", "{{paths.config_dir}}/world")
        .await;
    let instance = fixture.create(&descriptor, "One").await;
    let runtime = runtime_root(&instance);
    let marker = runtime.join(crate::private_runtime::PRIVATE_RUNTIME_MARKER);
    let before = fs::read(&instance.config_file_path).unwrap();
    fs::write(runtime.join("local-mod"), b"instance-owned mod").unwrap();
    for contents in [Some(b"unmanaged\n".as_slice()), None] {
        if let Some(contents) = contents {
            fs::write(&marker, contents).unwrap();
        } else {
            fs::remove_file(&marker).unwrap();
        }
        let report = read_instance_isolation(&fixture.storage, &instance.summary.id)
            .await
            .unwrap();
        assert_eq!(report.mode, "damaged");
        assert!(!report.issues.is_empty());
        assert!(matches!(
            read_instance_details(&fixture.storage, &instance.summary.id)
                .await
                .unwrap_err(),
            StorageError::PrivateRuntimeRefresh { .. }
        ));
        assert_eq!(fs::read(&instance.config_file_path).unwrap(), before);
        assert_eq!(
            fs::read(runtime.join("local-mod")).unwrap(),
            b"instance-owned mod"
        );
    }
}

#[tokio::test]
async fn peer_save_cannot_claim_another_instances_configuration_directory() {
    let fixture = Fixture::new().await;
    let first_module = fixture.module("one", "{{paths.config_dir}}/world").await;
    let second_module = fixture.module("two", "{{paths.config_dir}}/world").await;
    let first = fixture.create(&first_module, "One").await;
    let second = fixture.create(&second_module, "Two").await;
    let config = Path::new(&first.config_file_path).parent().unwrap();
    fixture
        .module("two", &config.to_string_lossy().replace('\\', "/"))
        .await;
    let report = read_instance_isolation(&fixture.storage, &first.summary.id)
        .await
        .unwrap();
    assert!(
        report
            .conflicts
            .iter()
            .any(|conflict| conflict.kind == "configuration"
                && conflict.instance_id == second.summary.id)
    );
    let details = read_instance_details(&fixture.storage, &first.summary.id)
        .await
        .unwrap();
    let before = fs::read(&first.config_file_path).unwrap();
    let peer_before = fs::read(&second.config_file_path).unwrap();
    for error in [
        update_instance(&fixture.storage, update_input(&details))
            .await
            .unwrap_err(),
        materialize_instance_configuration_for_start(&fixture.storage, &first.summary.id)
            .await
            .unwrap_err(),
    ] {
        assert!(
            matches!(error, StorageError::InstancePathConflict { ref kind, .. } if kind == "configuration"),
            "{error}"
        );
    }
    assert_eq!(fs::read(&first.config_file_path).unwrap(), before);
    assert_eq!(fs::read(&second.config_file_path).unwrap(), peer_before);
}
