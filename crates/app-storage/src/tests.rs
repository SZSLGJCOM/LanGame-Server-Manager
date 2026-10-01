use super::*;
use crate::settings_validation::{SettingsValidationPhase, validate_settings_against_schema};
use crate::storage_db::connect_pool;
use crate::templates::{SchemaDefaultContext, collect_schema_defaults_from_schema_json};
use app_core::{
    CreateInstanceInput, InstallState, InstanceStatus, ModuleSummary, PortBinding,
    UpdateInstanceInput,
};
use std::collections::BTreeSet;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[path = "tests_curseforge_membership.rs"]
mod curseforge_membership;
#[path = "settings_validation_tests.rs"]
mod settings_validation_tests;
#[path = "tests_workshop_collection_removal.rs"]
mod workshop_collection_removal;
#[path = "tests_workshop_collections.rs"]
mod workshop_collections;

#[path = "tests_retired_config_preservation.rs"]
mod retired_config_preservation_tests;
#[path = "tests_satisfactory_save.rs"]
mod satisfactory_save_tests;
#[path = "tests_save_policy.rs"]
mod save_policy_tests;

#[path = "tests_dst_instance_isolation.rs"]
mod dst_instance_isolation_tests;
#[path = "tests_private_runtime_required.rs"]
mod private_runtime_required_tests;

#[path = "tests_dst_delete_transaction.rs"]
mod dst_delete_transaction_tests;
#[path = "tests_instance_autostart.rs"]
mod instance_autostart_tests;
#[path = "tests_instance_connections.rs"]
mod instance_connections;
#[path = "tests_instance_creation_clean.rs"]
mod instance_creation_clean_tests;
#[path = "tests_instance_creation_isolation.rs"]
mod instance_creation_isolation_tests;
#[path = "tests_instance_deletion.rs"]
mod instance_deletion_tests;
#[cfg(windows)]
#[path = "tests_instance_details.rs"]
mod instance_details_tests;
#[path = "tests_instance_mutation_cancellation.rs"]
mod instance_mutation_cancellation_tests;
#[path = "tests_shared_save.rs"]
mod shared_save_tests;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

async fn record_started_test_instance(
    paths: &StoragePaths,
    instance_id: &str,
    pid: u32,
    log_path: &str,
) -> Result<ActiveInstanceRun, StorageError> {
    let process = mark_instance_process_started_with_identity(
        paths,
        &StartedInstanceProcess {
            instance_id,
            session_id: None,
            process_key: "main",
            display_name: "Server",
            pid,
            log_path,
            is_primary: true,
        },
        None,
    )
    .await?;

    Ok(ActiveInstanceRun {
        run_id: process.run_id,
        session_id: process.session_id.clone(),
        pid: process.pid,
        log_path: process.log_path.clone(),
        process_count: 1,
        processes: vec![process],
    })
}

#[tokio::test]
async fn create_instance_owns_wildcard_bind_and_disabled_autostart_defaults() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    let input: CreateInstanceInput = serde_json::from_value(serde_json::json!({
        "name": "Backend Defaults",
        "module_id": descriptor.summary.id
    }))
    .expect("identity-only create input should deserialize");
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let provisioning = create_instance(&paths, &descriptor, input).await.unwrap();
    let details = read_instance_details(&paths, &provisioning.summary.id)
        .await
        .unwrap();
    let config: Value =
        serde_json::from_str(&fs::read_to_string(&provisioning.config_file_path).unwrap()).unwrap();

    assert_eq!(provisioning.summary.bind_ip, "0.0.0.0");
    assert!(!provisioning.summary.autostart);
    assert_eq!(details.summary.bind_ip, "0.0.0.0");
    assert!(!details.summary.autostart);
    assert_eq!(config["settings"]["bind_ip"], "0.0.0.0");
    assert_eq!(config["autostart"], false);

    cleanup_root(&root);
}

#[tokio::test]
async fn repeated_instance_creation_inserts_distinct_records_and_directories() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let input = CreateInstanceInput {
        name: String::from("Shared Name"),
        module_id: descriptor.summary.id.clone(),
    };

    let first = create_instance(&paths, &descriptor, input.clone())
        .await
        .unwrap();
    let first_config = fs::read(&first.config_file_path).unwrap();
    replenish_test_library(&paths, &descriptor).await;
    let second = create_instance(&paths, &descriptor, input).await.unwrap();

    assert_ne!(first.summary.id, second.summary.id);
    assert_eq!(fs::read(&first.config_file_path).unwrap(), first_config);
    assert!(
        Path::new(&first.config_file_path).parent().unwrap()
            != Path::new(&second.config_file_path).parent().unwrap()
    );
    for instance in [&first, &second] {
        let instance_root = Path::new(&instance.config_file_path)
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        assert!(
            instance_root
                .join("runtime")
                .join(".langame-private-runtime")
                .is_file()
        );
        assert!(instance_root.join("runtime").join("mods").is_dir());
    }
    let pool = connect_pool(&paths).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM instances WHERE module_id = ?1")
        .bind(&descriptor.summary.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert_eq!(count, 2);
    cleanup_root(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_same_module_creation_assigns_distinct_private_runtimes() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let create = |name: &str| {
        let paths = paths.clone();
        let descriptor = descriptor.clone();
        let barrier = Arc::clone(&barrier);
        let name = name.to_owned();
        tokio::spawn(async move {
            barrier.wait().await;
            create_instance_with_options(
                &paths,
                &descriptor,
                CreateInstanceInput {
                    name,
                    module_id: descriptor.summary.id.clone(),
                },
                InstanceCreationOptions {
                    private_runtime: Some(PrivateRuntimeProjection {
                        private_directories: vec![PathBuf::from("mods")],
                    }),
                    ..Default::default()
                },
            )
            .await
        })
    };
    let first = create("Concurrent Alpha");
    let second = create("Concurrent Beta");
    barrier.wait().await;

    let first = first.await.unwrap().unwrap();
    let second = second.await.unwrap().unwrap();
    let shared_root = paths.games_root.join(&descriptor.summary.id);
    let results = [&first, &second];

    assert_ne!(first.effective_install_root, second.effective_install_root);
    for result in results {
        assert_ne!(result.effective_install_root, shared_root);
        assert!(
            result
                .effective_install_root
                .join(".langame-private-runtime")
                .is_file()
        );
    }
    assert_eq!(list_instances(&paths).await.unwrap().len(), 2);

    cleanup_root(&root);
}

#[tokio::test]
async fn schema_default_sources_resolve_instance_identity_fields() {
    let defaults = collect_schema_defaults_from_schema_json(
        Some(
            r#"{
              "type": "object",
              "properties": {
                "server_name": {
                  "type": "string",
                  "default": "Fallback Server",
                  "x-lsgm-default-source": "instance_name"
                },
                "world_name": {
                  "type": "string",
                  "default": "Fallback World",
                  "x-lsgm-default-source": "instance_name"
                },
                "world_save_name": {
                  "type": "string",
                  "default": "Cascade",
                  "x-lsgm-default-source": "instance_id"
                },
                "generated_password": {
                  "type": "string",
                  "minLength": 24,
                  "x-lsgm-default-source": "generated_secret"
                }
              }
            }"#,
        ),
        SchemaDefaultContext {
            instance_id: Some("facility-alpha"),
            instance_name: Some("Facility Alpha"),
        },
    )
    .unwrap();

    assert_eq!(
        defaults.get("server_name").and_then(Value::as_str),
        Some("Facility Alpha")
    );
    assert_eq!(
        defaults.get("world_name").and_then(Value::as_str),
        Some("Facility Alpha")
    );
    assert_eq!(
        defaults.get("world_save_name").and_then(Value::as_str),
        Some("facility-alpha")
    );
    let generated_password = defaults
        .get("generated_password")
        .and_then(Value::as_str)
        .expect("generated secret default");
    assert_eq!(generated_password.len(), 32);
    assert!(
        generated_password
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    );
}

#[test]
fn repository_module_defaults_pass_creation_phase_schema_validation() {
    let descriptors = app_modules::discover_modules(repo_root().join("modules")).unwrap();

    for descriptor in descriptors {
        let mut settings = collect_schema_defaults_from_schema_json(
            descriptor.schema_json.as_deref(),
            SchemaDefaultContext {
                instance_id: Some("schema-audit-instance"),
                instance_name: Some(&descriptor.summary.name),
            },
        )
        .unwrap();
        settings.insert(
            String::from("bind_ip"),
            Value::String(String::from("0.0.0.0")),
        );

        validate_settings_against_schema(
            Some(&descriptor),
            &settings,
            SettingsValidationPhase::Creation,
        )
        .unwrap_or_else(|error| {
            panic!(
                "module {} produced invalid creation defaults: {error}",
                descriptor.summary.id
            )
        });
    }
}

#[test]
fn complete_schema_validation_covers_types_enums_ranges_and_nested_items() {
    let root = unique_test_root();
    let mut descriptor = declared_save_path_test_descriptor(&root);
    descriptor.schema_json = Some(String::from(
        r#"{
          "type": "object",
          "properties": {
            "enabled": { "type": "boolean" },
            "mode": { "type": "string", "enum": ["campaign", "sandbox"] },
            "factor": { "type": "number", "enum": [0.5, 1, 1.5] },
            "players": { "type": "integer", "minimum": 1, "maximum": 8 },
            "tick_rate": { "type": "number", "multipleOf": 0.25 },
            "admins": {
              "type": "array",
              "items": {
                "type": "object",
                "properties": {
                  "steam_id": { "type": "string", "pattern": "^[0-9]{17}$" }
                },
                "required": ["steam_id"]
              }
            }
          },
          "required": ["enabled", "mode", "factor", "players", "tick_rate"]
        }"#,
    ));
    let valid = json!({
        "enabled": true,
        "mode": "campaign",
        "factor": 1.0,
        "players": 4,
        "tick_rate": 1.25,
        "admins": [{ "steam_id": "76561198000000000" }]
    })
    .as_object()
    .unwrap()
    .clone();

    validate_settings_against_schema(Some(&descriptor), &valid, SettingsValidationPhase::Complete)
        .unwrap();

    let invalid_cases = [
        ("enabled", json!(1), "must be boolean"),
        ("mode", json!("survival"), "must be one of"),
        ("players", json!(9), "must be at most 8"),
        ("tick_rate", json!(1.3), "must be a multiple of 0.25"),
        (
            "admins",
            json!([{ "steam_id": "invalid" }]),
            "admins[0].steam_id",
        ),
    ];

    for (field, value, expected_message) in invalid_cases {
        let mut settings = valid.clone();
        settings.insert(String::from(field), value);
        let error = validate_settings_against_schema(
            Some(&descriptor),
            &settings,
            SettingsValidationPhase::Complete,
        )
        .expect_err("invalid schema value should be rejected");
        assert!(
            error.to_string().contains(expected_message),
            "unexpected validation error for {field}: {error}"
        );
    }
}

#[tokio::test]
async fn broadcast_policy_and_events_roundtrip() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    write_module_descriptor_files(&descriptor);
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    prepare_shared_install(&paths, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Broadcast Test"),
            module_id: descriptor.summary.id.clone(),
        },
    )
    .await
    .unwrap();

    let default_policy = read_instance_broadcast_policy(&paths, &created.summary.id)
        .await
        .unwrap();
    assert!(!default_policy.enabled);
    assert_eq!(default_policy.rules.tone, "short");
    assert_eq!(default_policy.rules.periodic.interval_minutes, 30);
    assert!(default_policy.rules.startup.prompt.is_none());

    let mut rules = InstanceBroadcastRules::default();
    rules.startup.enabled = true;
    rules.startup.prompt = Some(String::from("Announce the server is online."));
    rules.periodic.enabled = true;
    rules.periodic.interval_minutes = 45;
    rules.periodic.prompt = Some(String::from("Share a short regular server reminder."));
    rules.tone = String::from("formal");
    rules.cooldown_minutes = 15;

    let saved_policy = upsert_instance_broadcast_policy(
        &paths,
        UpdateInstanceBroadcastPolicyInput {
            instance_id: created.summary.id.clone(),
            enabled: true,
            rules: rules.clone(),
        },
    )
    .await
    .unwrap();
    assert!(saved_policy.enabled);
    assert!(saved_policy.rules.startup.enabled);
    assert_eq!(saved_policy.rules.periodic.interval_minutes, 45);

    let reloaded_policy = read_instance_broadcast_policy(&paths, &created.summary.id)
        .await
        .unwrap();
    assert!(reloaded_policy.enabled);
    assert_eq!(reloaded_policy.rules.tone, "formal");
    assert_eq!(reloaded_policy.rules.cooldown_minutes, 15);
    assert_eq!(
        reloaded_policy.rules.startup.prompt.as_deref(),
        Some("Announce the server is online.")
    );
    assert_eq!(
        reloaded_policy.rules.periodic.prompt.as_deref(),
        Some("Share a short regular server reminder.")
    );

    let event = insert_instance_broadcast_event(
        &paths,
        InsertInstanceBroadcastEventInput {
            instance_id: created.summary.id.clone(),
            module_id: descriptor.summary.id.clone(),
            source: String::from("manual"),
            rule_id: None,
            message: String::from("Server restart in five minutes."),
            ai_provider: Some(String::from("openai")),
            ai_model: Some(String::from("gpt-4.1-mini")),
            action_id: Some(String::from("broadcast")),
            transport: Some(String::from("source_rcon")),
            command_preview: Some(String::from("say Server restart in five minutes.")),
            status: String::from("sent"),
            response_text: Some(String::from("ok")),
            error_message: None,
            initiator: Some(String::from("manual")),
            policy_snapshot_json: Some(String::from(r#"{"enabled":true}"#)),
        },
    )
    .await
    .unwrap();
    assert_eq!(event.status, "sent");

    let events = list_instance_broadcast_events(&paths, &created.summary.id, 10)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].message, "Server restart in five minutes.");
    assert_eq!(events[0].response_text.as_deref(), Some("ok"));
    assert_eq!(events[0].initiator.as_deref(), Some("manual"));
    assert_eq!(
        events[0].policy_snapshot_json.as_deref(),
        Some(r#"{"enabled":true}"#)
    );

    cleanup_root(&root);
}

fn unique_test_root() -> PathBuf {
    let counter = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("test clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "langame-storage-test-{}-{stamp}-{counter}",
        std::process::id()
    ))
}

fn cleanup_root(root: &Path) {
    for _ in 0..5 {
        if !root.exists() {
            return;
        }

        if fs::remove_dir_all(root).is_ok() {
            return;
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}

async fn configure_test_instance_runtime(
    paths: &StoragePaths,
    instance_id: &str,
    bind_ip: &str,
    autostart: bool,
) -> InstanceDetails {
    let details = update_instance_autostart(paths, instance_id, autostart)
        .await
        .unwrap();
    update_instance(
        paths,
        UpdateInstanceInput {
            id: details.summary.id,
            bind_ip: String::from(bind_ip),
            auto_backup_on_stop: details.auto_backup_on_stop,
            backup_retention_count: details.backup_retention_count,
            settings_json: details.settings_json,
            ports: details.ports,
        },
    )
    .await
    .unwrap()
}

#[test]
fn default_storage_paths_use_langame_runtime_layout() {
    let paths = StoragePaths::default();
    let expected_runtime_root =
        resolve_runtime_root(Path::new(DEFAULT_LANGAME_DATA_ROOT), &paths.app_data_root);

    assert_eq!(
        paths.instances_root,
        expected_runtime_root.join("instances")
    );
    assert_eq!(paths.games_root, expected_runtime_root.join("server-files"));
    assert_eq!(
        paths.steamcmd_root,
        expected_runtime_root.join("cmd").join("steamcmd")
    );
    assert!(paths.modules_root.is_absolute());
    assert!(
        !paths
            .modules_root
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    );
}

#[test]
fn runtime_layout_falls_back_to_user_data_when_the_managed_drive_is_unavailable() {
    let root = unique_test_root();
    let missing_managed_root = root.join("missing-managed-root");
    let app_data_root = root.join("appdata");

    assert_eq!(
        resolve_runtime_root(&missing_managed_root, &app_data_root),
        app_data_root.join("runtime")
    );
}

async fn replace_instance_ports_for_test(
    paths: &StoragePaths,
    instance_id: &str,
    ports: &[PortBinding],
) {
    let pool = connect_pool(paths).await.unwrap();
    let mut tx = pool.begin().await.unwrap();

    sqlx::query(
        r#"
        DELETE FROM instance_ports
        WHERE instance_id = ?1
        "#,
    )
    .bind(instance_id)
    .execute(&mut *tx)
    .await
    .unwrap();

    for port in ports {
        sqlx::query(
            r#"
            INSERT INTO instance_ports (instance_id, name, protocol, port)
            VALUES (?1, ?2, ?3, ?4)
            "#,
        )
        .bind(instance_id)
        .bind(&port.name)
        .bind(&port.protocol)
        .bind(i64::from(port.port))
        .execute(&mut *tx)
        .await
        .unwrap();
    }

    tx.commit().await.unwrap();
    pool.close().await;
}

fn assert_file_has_no_utf8_bom(path: &Path) {
    let bytes = fs::read(path).unwrap();
    let has_bom = bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF;
    assert!(
        !has_bom,
        "expected {} to be written without a UTF-8 BOM",
        path.display()
    );
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

struct RepoSurvivalInstanceSmokeCase {
    module_id: &'static str,
    executable: &'static str,
    rendered_config_paths: &'static [&'static str],
    materialized_install_paths: &'static [&'static str],
}

#[tokio::test]
async fn minecraft_instances_generate_distinct_persisted_rcon_secrets() {
    let root = unique_test_root();
    let mut paths = test_paths(&root);
    paths.modules_root = repo_root().join("modules");
    let descriptor = app_modules::discover_modules(&paths.modules_root)
        .expect("discover repo modules")
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "minecraft")
        .expect("missing Minecraft module");

    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    fs::create_dir_all(paths.games_root.join("minecraft")).unwrap();
    replenish_test_library(&paths, &descriptor).await;

    let create = |name: &str| CreateInstanceInput {
        name: String::from(name),
        module_id: String::from("minecraft"),
    };
    let first = create_instance(&paths, &descriptor, create("Minecraft One"))
        .await
        .unwrap();
    let second = create_instance(&paths, &descriptor, create("Minecraft Two"))
        .await
        .unwrap();

    let read_secret = |config_file_path: &str| {
        let document: Value =
            serde_json::from_str(&fs::read_to_string(config_file_path).unwrap()).unwrap();
        document["settings"]["rcon_password"]
            .as_str()
            .expect("persisted Minecraft RCON password")
            .to_string()
    };
    let first_secret = read_secret(&first.config_file_path);
    let second_secret = read_secret(&second.config_file_path);
    assert_eq!(first_secret.len(), 32);
    assert_eq!(second_secret.len(), 32);
    assert_ne!(first_secret, second_secret);
    assert!(
        first_secret
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    );
    assert!(
        second_secret
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    );

    let first_properties = fs::read_to_string(
        Path::new(&first.config_file_path)
            .parent()
            .unwrap()
            .join("server.properties"),
    )
    .unwrap();
    assert!(first_properties.contains(&format!("rcon.password={first_secret}")));

    cleanup_root(&root);
}

#[tokio::test]
async fn requested_survival_repo_modules_create_instances_and_materialize_configs() {
    let root = unique_test_root();
    let mut paths = test_paths(&root);
    paths.modules_root = repo_root().join("modules");

    let cases = [
        RepoSurvivalInstanceSmokeCase {
            module_id: "returntomoria",
            executable: "MoriaServer.exe",
            rendered_config_paths: &[
                "MoriaServerConfig.ini",
                "MoriaServerRules.txt",
                "MoriaServerPermissions.txt",
            ],
            materialized_install_paths: &[
                "MoriaServerConfig.ini",
                "MoriaServerRules.txt",
                "MoriaServerPermissions.txt",
            ],
        },
        RepoSurvivalInstanceSmokeCase {
            module_id: "astroneer",
            executable: "AstroServer.exe",
            rendered_config_paths: &[
                "Astro/Saved/Config/WindowsServer/AstroServerSettings.ini",
                "Astro/Saved/Config/WindowsServer/Engine.ini",
                "Astro/Saved/Config/WindowsServer/Game.ini",
            ],
            materialized_install_paths: &[
                "Astro/Saved/Config/WindowsServer/AstroServerSettings.ini",
                "Astro/Saved/Config/WindowsServer/Engine.ini",
                "Astro/Saved/Config/WindowsServer/Game.ini",
            ],
        },
        RepoSurvivalInstanceSmokeCase {
            module_id: "nightingale",
            executable: "NWXServer.exe",
            rendered_config_paths: &["NWX/Config/ServerSettings.ini"],
            materialized_install_paths: &["NWX/Config/ServerSettings.ini"],
        },
        RepoSurvivalInstanceSmokeCase {
            module_id: "theforest",
            executable: "TheForestDedicatedServer.exe",
            rendered_config_paths: &["server.cfg"],
            materialized_install_paths: &[],
        },
    ];

    let repo_descriptors =
        app_modules::discover_modules(&paths.modules_root).expect("discover repo modules");
    let descriptors = cases
        .iter()
        .map(|case| {
            repo_descriptors
                .iter()
                .find(|descriptor| descriptor.summary.id == case.module_id)
                .unwrap_or_else(|| panic!("missing repo module {}", case.module_id))
                .clone()
        })
        .collect::<Vec<_>>();

    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, &descriptors).await.unwrap();

    for case in cases {
        let descriptor = descriptors
            .iter()
            .find(|descriptor| descriptor.summary.id == case.module_id)
            .unwrap_or_else(|| panic!("missing descriptor {}", case.module_id));
        let process = descriptor
            .process
            .as_ref()
            .unwrap_or_else(|| panic!("{} missing process spec", case.module_id));
        assert_eq!(process.executable, case.executable);

        let install_root = root.join("verified-installs").join(case.module_id);
        let executable_path = install_root.join(case.executable);
        fs::create_dir_all(executable_path.parent().unwrap()).unwrap();
        fs::write(&executable_path, "").unwrap();

        sync_game_installs(
            &paths,
            &[GameInstallSyncRecord {
                module_id: String::from(case.module_id),
                install_root: install_root.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: Some(String::from("repo-survival-smoke")),
                mark_verified: true,
            }],
        )
        .await
        .unwrap();

        let created = create_instance(
            &paths,
            descriptor,
            CreateInstanceInput {
                name: format!("{} Install Chain", case.module_id),
                module_id: String::from(case.module_id),
            },
        )
        .await
        .unwrap();

        assert_eq!(created.summary.module_id, case.module_id);
        assert_eq!(created.ports.len(), descriptor.default_ports.len());
        let runtime_root = instance_private_runtime_root(&created);
        assert!(runtime_root.join(case.executable).is_file());

        let config_file_path = PathBuf::from(&created.config_file_path);
        assert!(config_file_path.exists());
        let config_dir = config_file_path.parent().unwrap();

        for relative_path in case.rendered_config_paths {
            assert!(
                config_dir.join(relative_path).exists(),
                "{} should render {}",
                case.module_id,
                relative_path
            );
        }
        for relative_path in case.materialized_install_paths {
            assert!(
                runtime_root.join(relative_path).exists(),
                "{} should materialize {} into the instance runtime",
                case.module_id,
                relative_path
            );
            assert!(
                !install_root.join(relative_path).exists(),
                "{} should leave shared install {} untouched",
                case.module_id,
                relative_path
            );
        }

        let details = read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap();
        assert_eq!(details.summary.module_id, case.module_id);
        assert_eq!(details.ports.len(), descriptor.default_ports.len());
        assert!(details.settings_json.contains("\"bind_ip\""));
    }

    cleanup_root(&root);
}

const EXPECTED_PALWORLD_OPTION_KEYS: &[&str] = &[
    "AdditionalDropItemNumWhenPlayerKillingInPvPMode",
    "AdditionalDropItemWhenPlayerKillingInPvPMode",
    "AdminPassword",
    "AutoResetGuildTimeNoOnlinePlayers",
    "AutoSaveSpan",
    "AutoTransferMasterCheckIntervalSeconds",
    "AutoTransferMasterThresholdDays",
    "bActiveUNKO",
    "bAdditionalDropItemWhenPlayerKillingInPvPMode",
    "bAllowClientMod",
    "bAllowEnhanceStat_Attack",
    "bAllowEnhanceStat_Health",
    "bAllowEnhanceStat_Stamina",
    "bAllowEnhanceStat_Weight",
    "bAllowEnhanceStat_WorkSpeed",
    "bAllowGlobalPalboxExport",
    "bAllowGlobalPalboxImport",
    "BanListURL",
    "BaseCampMaxNum",
    "BaseCampMaxNumInGuild",
    "BaseCampWorkerMaxNum",
    "bAutoResetGuildNoOnlinePlayers",
    "bBuildAreaLimit",
    "bCanPickupOtherGuildDeathPenaltyDrop",
    "bCharacterRecreateInHardcore",
    "bDisplayPvPItemNumOnWorldMap_BaseCamp",
    "bDisplayPvPItemNumOnWorldMap_Player",
    "bEnableAimAssistKeyboard",
    "bEnableAimAssistPad",
    "bEnableBuildingPlayerUIdDisplay",
    "bEnableFastTravel",
    "bEnableFastTravelOnlyBaseCamp",
    "bEnableFriendlyFire",
    "bEnableInvaderEnemy",
    "bEnablePlayerToPlayerDamage",
    "bEnableVoiceChat",
    "bExistPlayerAfterLogout",
    "bHardcore",
    "bInvisibleOtherGuildBaseCampAreaFX",
    "bIsPvP",
    "bIsRandomizerPalLevelRandom",
    "bIsShowJoinLeftMessage",
    "bIsStartLocationSelectByMap",
    "bIsUseBackupSaveData",
    "BlockRespawnTime",
    "bPalLost",
    "bShowPlayerList",
    "BuildObjectDamageRate",
    "BuildObjectDeteriorationDamageRate",
    "BuildObjectHpRate",
    "BuildingNameDisplayCacheTTLSeconds",
    "bUseAuth",
    "ChatPostLimitPerMinute",
    "CollectionDropRate",
    "CollectionObjectHpRate",
    "CollectionObjectRespawnSpeedRate",
    "CrossplayPlatforms",
    "DayTimeSpeedRate",
    "DeathPenalty",
    "DenyTechnologyList",
    "DropItemAliveMaxHours",
    "DropItemMaxNum",
    "DropItemMaxNum_UNKO",
    "EnablePredatorBossPal",
    "EnemyDropItemRate",
    "EquipmentDurabilityDamageRate",
    "ExpRate",
    "GuildPlayerMaxNum",
    "GuildRejoinCooldownMinutes",
    "ItemContainerForceMarkDirtyInterval",
    "ItemCorruptionMultiplier",
    "ItemWeightRate",
    "LogFormatType",
    "MaxBuildingLimitNum",
    "MaxBuildingLimitNumPerPlayer",
    "MaxGuildsPerFrame",
    "MonsterFarmActionSpeedRate",
    "NightTimeSpeedRate",
    "PalAutoHPRegeneRate",
    "PalAutoHpRegeneRateInSleep",
    "PalCaptureRate",
    "PalDamageRateAttack",
    "PalDamageRateDefense",
    "PalEggDefaultHatchingTime",
    "PalSpawnNumRate",
    "PalStaminaDecreaceRate",
    "PalStomachDecreaceRate",
    "PhysicsActiveDropItemMaxNum",
    "PlayerAutoHPRegeneRate",
    "PlayerAutoHpRegeneRateInSleep",
    "PlayerDamageRateAttack",
    "PlayerDamageRateDefense",
    "PlayerStaminaDecreaceRate",
    "PlayerStomachDecreaceRate",
    "PlayerDataPalStorageUpdateCheckTickInterval",
    "PublicIP",
    "PublicPort",
    "RandomizerSeed",
    "RandomizerType",
    "RCONEnabled",
    "RCONPort",
    "Region",
    "RespawnPenaltyDurationThreshold",
    "RespawnPenaltyTimeScale",
    "RESTAPIEnabled",
    "RESTAPIPort",
    "ServerDescription",
    "ServerName",
    "ServerPassword",
    "ServerPlayerMaxNum",
    "ServerReplicatePawnCullDistance",
    "SupplyDropSpan",
    "VoiceChatMaxVolumeDistance",
    "VoiceChatZeroVolumeDistance",
    "WorkSpeedRate",
];

fn parse_palworld_option_keys(document: &str) -> BTreeSet<String> {
    let Some(option_settings_start) = document.find("OptionSettings=(") else {
        panic!("expected Palworld settings document to contain OptionSettings=(");
    };

    let mut depth = 1i32;
    let mut in_string = false;
    let mut escaping = false;
    let mut entry = String::new();
    let mut keys = BTreeSet::new();

    for character in document[option_settings_start + "OptionSettings=(".len()..].chars() {
        if in_string {
            entry.push(character);
            if escaping {
                escaping = false;
                continue;
            }
            match character {
                '\\' => escaping = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match character {
            '"' => {
                in_string = true;
                entry.push(character);
            }
            '(' => {
                depth += 1;
                entry.push(character);
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    if let Some((key, _)) = entry.trim().split_once('=') {
                        keys.insert(String::from(key.trim()));
                    }
                    break;
                }
                entry.push(character);
            }
            ',' if depth == 1 => {
                if let Some((key, _)) = entry.trim().split_once('=') {
                    keys.insert(String::from(key.trim()));
                }
                entry.clear();
            }
            _ => entry.push(character),
        }
    }

    keys
}

fn instance_private_runtime_root(created: &InstanceProvisioning) -> PathBuf {
    let config_root = Path::new(&created.config_file_path)
        .parent()
        .expect("instance config directory");
    let runtime_root = config_root.parent().expect("instance root").join("runtime");
    assert!(runtime_root.join(".langame-private-runtime").is_file());
    runtime_root
}

fn test_paths(root: &Path) -> StoragePaths {
    StoragePaths {
        app_data_root: root.join("appdata"),
        settings_path: root.join("appdata").join("settings.json"),
        database_path: root.join("appdata").join("db").join("lgs.db"),
        logs_root: root.join("appdata").join("logs"),
        modules_root: root.join("modules"),
        migrations_root: root.join("migrations"),
        steamcmd_root: root.join("programdata").join("steamcmd"),
        games_root: root.join("games"),
        instances_root: root.join("instances"),
        archives_root: root.join("instances").join(".trash"),
    }
}

fn managed_instance_directories(paths: &StoragePaths) -> Vec<PathBuf> {
    let mut directories = fs::read_dir(&paths.instances_root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    directories.sort();
    directories
}

fn write_module_descriptor_files(descriptor: &ModuleDescriptor) {
    fs::create_dir_all(&descriptor.root).unwrap();
    fs::write(
        descriptor.root.join("module.toml"),
        &descriptor.manifest_toml,
    )
    .unwrap();
    if let Some(schema_json) = &descriptor.schema_json {
        fs::write(descriptor.root.join("schema.json"), schema_json).unwrap();
    }
}

fn test_descriptor(root: &Path) -> ModuleDescriptor {
    ModuleDescriptor {
            root: root.join("modules").join("dontstarve"),
            manifest_toml: String::from(
                "id = \"dontstarve\"\nname = \"Don't Starve Together\"\nversion = \"0.1.0\"\nsupported_platforms = [\"windows\"]\n\n[storage]\nsaves_path_template = \"{{paths.config_dir}}/clusters/main\"\n",
            ),
            schema_json: Some(
                r#"{
                  "type": "object",
                  "properties": {
                    "cluster_name": { "type": "string", "default": "DST Server", "x-lsgm-default-source": "instance_name" },
                    "cluster_description": { "type": "string", "default": "Test cluster" },
                    "max_players": { "type": "integer", "default": 6 },
                    "bind_ip": { "type": "string", "default": "0.0.0.0" },
                    "cluster_password": { "type": "string", "default": "" },
                    "cluster_intention": { "type": "string", "default": "default" },
                    "pause_when_empty": { "type": "boolean", "default": false },
                    "pvp": { "type": "boolean", "default": false },
                    "vote_enabled": { "type": "boolean", "default": true },
                    "offline_cluster": { "type": "boolean", "default": true },
                    "lan_only_cluster": { "type": "boolean", "default": true },
                    "tick_rate": { "type": "integer", "default": 15 },
                    "autosaver_enabled": { "type": "boolean", "default": true },
                    "enable_caves": { "type": "boolean", "default": false },
                    "world_specialevent": { "type": "string", "default": "default" },
                    "world_autumn": { "type": "string", "default": "default" },
                    "world_winter": { "type": "string", "default": "default" },
                    "world_spring": { "type": "string", "default": "default" },
                    "world_summer": { "type": "string", "default": "default" },
                    "world_extrastartingitems": { "type": "string", "default": "default" },
                    "world_seasonalstartingitems": { "type": "string", "default": "default" },
                    "world_spawnprotection": { "type": "string", "default": "default" },
                    "world_dropeverythingondespawn": { "type": "string", "default": "default" },
                    "world_darkness": { "type": "string", "default": "default" },
                    "world_temperaturedamage": { "type": "string", "default": "default" },
                    "world_hunger": { "type": "string", "default": "default" },
                    "world_healthpenalty": { "type": "string", "default": "always" },
                    "world_shadowcreatures": { "type": "string", "default": "default" },
                    "world_brightmarecreatures": { "type": "string", "default": "default" },
                    "master_world_size": { "type": "string", "default": "default" },
                    "master_season_start": { "type": "string", "default": "default" },
                    "master_task_set": { "type": "string", "default": "default" },
                    "master_start_location": { "type": "string", "default": "default" },
                    "master_day": { "type": "string", "default": "default" },
                    "master_branching": { "type": "string", "default": "default" },
                    "master_loop": { "type": "string", "default": "default" },
                    "master_touchstone": { "type": "string", "default": "default" },
                    "master_roads": { "type": "string", "default": "default" },
                    "master_boons": { "type": "string", "default": "default" },
                    "master_prefabswaps_start": { "type": "string", "default": "default" },
                    "master_petrification": { "type": "string", "default": "default" },
                    "master_meteorshowers": { "type": "string", "default": "default" },
                    "master_regrowth": { "type": "string", "default": "default" },
                    "master_weather": { "type": "string", "default": "default" },
                    "master_frogs": { "type": "string", "default": "default" },
                    "master_hounds": { "type": "string", "default": "default" },
                    "master_lightning": { "type": "string", "default": "default" },
                    "master_wildfires": { "type": "string", "default": "default" },
                    "master_berrybush": { "type": "string", "default": "default" },
                    "master_carrot": { "type": "string", "default": "default" },
                    "master_flint": { "type": "string", "default": "default" },
                    "master_grass": { "type": "string", "default": "default" },
                    "master_marshbush": { "type": "string", "default": "default" },
                    "master_reeds": { "type": "string", "default": "default" },
                    "master_rock": { "type": "string", "default": "default" },
                    "master_sapling": { "type": "string", "default": "default" },
                    "master_trees": { "type": "string", "default": "default" },
                    "master_flowers": { "type": "string", "default": "default" },
                    "master_ponds": { "type": "string", "default": "default" },
                    "master_tumbleweed": { "type": "string", "default": "default" },
                    "master_bees": { "type": "string", "default": "default" },
                    "master_beefalo": { "type": "string", "default": "default" },
                    "master_butterfly": { "type": "string", "default": "default" },
                    "master_buzzard": { "type": "string", "default": "default" },
                    "master_catcoon": { "type": "string", "default": "default" },
                    "master_moles": { "type": "string", "default": "default" },
                    "master_pigs": { "type": "string", "default": "default" },
                    "master_rabbits": { "type": "string", "default": "default" },
                    "master_lightninggoat": { "type": "string", "default": "default" },
                    "master_spiders": { "type": "string", "default": "default" },
                    "master_tallbirds": { "type": "string", "default": "default" },
                    "master_tentacles": { "type": "string", "default": "default" },
                    "master_penguins": { "type": "string", "default": "default" },
                    "master_perd": { "type": "string", "default": "default" },
                    "master_angrybees": { "type": "string", "default": "default" },
                    "master_chess": { "type": "string", "default": "default" },
                    "master_krampus": { "type": "string", "default": "default" },
                    "master_walrus": { "type": "string", "default": "default" },
                    "master_merm": { "type": "string", "default": "default" },
                    "master_houndmound": { "type": "string", "default": "default" },
                    "master_lureplants": { "type": "string", "default": "default" },
                    "master_bearger": { "type": "string", "default": "default" },
                    "master_beequeen": { "type": "string", "default": "default" },
                    "master_deerclops": { "type": "string", "default": "default" },
                    "master_dragonfly": { "type": "string", "default": "default" },
                    "master_klaus": { "type": "string", "default": "default" },
                    "master_goosemoose": { "type": "string", "default": "default" },
                    "master_spiderqueen": { "type": "string", "default": "default" },
                    "master_liefs": { "type": "string", "default": "default" },
                    "master_toadstool": { "type": "string", "default": "default" },
                    "master_world_overrides_extra": { "type": "string", "default": "" },
                    "master_worldgenoverride_lua": { "type": "string", "default": "return {\n  override_enabled = true,\n  settings_preset = \"SURVIVAL_TOGETHER\",\n  worldgen_preset = \"SURVIVAL_TOGETHER\",\n  overrides = {\n    world_size = \"default\",\n  }\n}\n" },
                    "caves_world_size": { "type": "string", "default": "default" },
                    "caves_branching": { "type": "string", "default": "default" },
                    "caves_loop": { "type": "string", "default": "default" },
                    "caves_atriumgate": { "type": "string", "default": "default" },
                    "caves_wormattacks": { "type": "string", "default": "default" },
                    "caves_earthquakes": { "type": "string", "default": "default" },
                    "caves_regrowth": { "type": "string", "default": "default" },
                    "caves_banana": { "type": "string", "default": "default" },
                    "caves_cave_ponds": { "type": "string", "default": "default" },
                    "caves_fern": { "type": "string", "default": "default" },
                    "caves_flint": { "type": "string", "default": "default" },
                    "caves_lichen": { "type": "string", "default": "default" },
                    "caves_marshbush": { "type": "string", "default": "default" },
                    "caves_mushroom": { "type": "string", "default": "default" },
                    "caves_wormlights": { "type": "string", "default": "default" },
                    "caves_flower_cave": { "type": "string", "default": "default" },
                    "caves_mushtree": { "type": "string", "default": "default" },
                    "caves_rock": { "type": "string", "default": "default" },
                    "caves_sapling": { "type": "string", "default": "default" },
                    "caves_bunnymen": { "type": "string", "default": "default" },
                    "caves_rocky": { "type": "string", "default": "default" },
                    "caves_slurper": { "type": "string", "default": "default" },
                    "caves_slurtles": { "type": "string", "default": "default" },
                    "caves_snurtles": { "type": "string", "default": "default" },
                    "caves_monkey": { "type": "string", "default": "default" },
                    "caves_bats": { "type": "string", "default": "default" },
                    "caves_worms": { "type": "string", "default": "default" },
                    "caves_spiders": { "type": "string", "default": "default" },
                    "caves_cave_spiders": { "type": "string", "default": "default" },
                    "caves_tentacles": { "type": "string", "default": "default" },
                    "caves_molebats": { "type": "string", "default": "default" },
                    "caves_nightmarecreatures": { "type": "string", "default": "default" },
                    "caves_spider_dropper": { "type": "string", "default": "default" },
                    "caves_spider_spitter": { "type": "string", "default": "default" },
                    "caves_fruitfly": { "type": "string", "default": "default" },
                    "caves_fissure": { "type": "string", "default": "default" },
                    "caves_world_overrides_extra": { "type": "string", "default": "" },
                    "caves_worldgenoverride_lua": { "type": "string", "default": "return {\n  override_enabled = true,\n  settings_preset = \"DST_CAVE\",\n  worldgen_preset = \"DST_CAVE\",\n  overrides = {\n    world_size = \"default\",\n  }\n}\n" },
                    "cluster_token": { "type": "string", "default": "" },
                    "admin_list": { "type": "string", "default": "" },
                    "whitelist": { "type": "string", "default": "" },
                    "blocklist": { "type": "string", "default": "" },
                    "whitelist_slots": { "type": "integer", "default": 0 },
                    "steam_group_only": { "type": "boolean", "default": false },
                    "steam_group_id": { "type": "integer", "default": 0 },
                    "steam_group_admins": { "type": "boolean", "default": false },
                    "shared_workshop_mod_ids": { "type": "string", "default": "" },
                    "shared_workshop_collection_ids": { "type": "string", "default": "" },
                    "master_enabled_workshop_mod_ids": { "type": "string", "default": "" },
                    "caves_enabled_workshop_mod_ids": { "type": "string", "default": "" },
                    "master_mod_configuration_options": { "type": "object", "default": {} },
                    "caves_mod_configuration_options": { "type": "object", "default": {} },
                    "master_modoverrides_lua": { "type": "string", "default": "return {\n}\n" },
                    "caves_modoverrides_lua": { "type": "string", "default": "return {\n}\n" }
                  }
                }"#
                .to_string(),
            ),
            default_ports: vec![
                PortBinding {
                    name: String::from("master"),
                    protocol: String::from("udp"),
                    port: 10999,
                },
                PortBinding {
                    name: String::from("backup"),
                    protocol: String::from("udp"),
                    port: 11000,
                },
            ],
            install: Some(app_core::InstallSpec {
                shared_game_dir: String::from("dontstarve"),
                download_url_windows: None,
                download_integrity_windows: None,
                source: None,
                verification_path: None,
                minecraft: None,
            }),
            process: Some(app_core::ProcessSpec {
                environment_template: Default::default(),
                executable: String::from("bin/server.exe"),
                args_template: vec![String::from("-cluster"), String::from("{{instance.name}}")],
                working_directory_template: None,
                window_policy: app_core::ProcessWindowPolicy::Background,
                host_surface: app_core::ProcessHostSurface::ManagedTerminal,
                host_notes: None,
            }),
            workshop: None,
            runtime: app_core::ModuleRuntimeSpec::default(),
            storage: app_modules::ModuleStorageSpec { saves_path_template: Some(String::from("{{paths.config_dir}}/clusters/main")), ..Default::default() },
            summary: ModuleSummary {
                id: String::from("dontstarve"),
                name: String::from("Don't Starve Together"),
                version: String::from("0.1.0"),
                description: Some(String::from("Test module")),
                steam_app_id: Some(343050),
                install_state: InstallState::NotInstalled,
                instance_program_count: 0,
                archived_program_count: 0,
                supported_platforms: vec![String::from("windows")],
            },
        }
}

fn runescape_dragonwilds_test_descriptor(root: &Path) -> ModuleDescriptor {
    let repo_module_root = repo_root().join("modules").join("runescapedragonwilds");
    ModuleDescriptor {
        root: root.join("modules").join("runescapedragonwilds"),
        manifest_toml: fs::read_to_string(repo_module_root.join("module.toml")).unwrap(),
        schema_json: Some(fs::read_to_string(repo_module_root.join("schema.json")).unwrap()),
        default_ports: vec![PortBinding {
            name: String::from("game"),
            protocol: String::from("udp"),
            port: 7777,
        }],
        install: None,
        process: None,
        workshop: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
        storage: app_modules::ModuleStorageSpec {
            saves_path_template: Some(String::from(
                "{{paths.install_root}}/RSDragonwilds/Saved/SaveGames",
            )),
            ..Default::default()
        },
        summary: ModuleSummary {
            id: String::from("runescapedragonwilds"),
            name: String::from("RuneScape: Dragonwilds Dedicated Server"),
            version: String::from("0.1.0"),
            description: Some(String::from("Test module")),
            steam_app_id: Some(4019830),
            install_state: InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
    }
}

fn prepare_shared_install(paths: &StoragePaths, descriptor: &ModuleDescriptor) {
    let shared_game_dir = descriptor
        .install
        .as_ref()
        .map(|install| install.shared_game_dir.as_str())
        .unwrap_or(&descriptor.summary.id);
    fs::create_dir_all(paths.games_root.join(shared_game_dir)).unwrap();
}

fn prepare_runescape_dragonwilds_environment(root: &Path, descriptor: &ModuleDescriptor) {
    prepare_environment(root, descriptor);
    let repo_module_root = repo_root().join("modules").join("runescapedragonwilds");
    let template_target = descriptor
        .root
        .join("templates")
        .join("RSDragonwilds")
        .join("Saved")
        .join("Config")
        .join("WindowsServer");
    fs::create_dir_all(&template_target).unwrap();
    fs::copy(
        repo_module_root
            .join("templates")
            .join("RSDragonwilds")
            .join("Saved")
            .join("Config")
            .join("WindowsServer")
            .join("DedicatedServer.ini.hbs"),
        template_target.join("DedicatedServer.ini.hbs"),
    )
    .unwrap();
}

fn prepare_environment(root: &Path, descriptor: &ModuleDescriptor) {
    let paths = test_paths(root);
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    fs::create_dir_all(&paths.logs_root).unwrap();
    fs::create_dir_all(&paths.steamcmd_root).unwrap();
    fs::create_dir_all(&paths.games_root).unwrap();
    fs::create_dir_all(paths.games_root.join("dontstarve").join("mods")).unwrap();
    prepare_shared_install(&paths, descriptor);
    fs::create_dir_all(&paths.instances_root).unwrap();
    write_module_descriptor_files(descriptor);
    let templates_root = descriptor.root.join("templates");
    fs::create_dir_all(templates_root.join("Master")).unwrap();
    fs::create_dir_all(templates_root.join("Caves")).unwrap();
    fs::write(
            templates_root.join("cluster.ini.hbs"),
            "[GAMEPLAY]\ncluster_name = {{cluster_name}}\nbind_ip = {{bind_ip}}\nmax_players = {{max_players}}\npvp = {{pvp}}\npause_when_empty = {{pause_when_empty}}\nvote_enabled = {{vote_enabled}}\n\n[NETWORK]\noffline_cluster = {{offline_cluster}}\nlan_only_cluster = {{lan_only_cluster}}\ntick_rate = {{tick_rate}}\nwhitelist_slots = {{whitelist_slots}}\n{{dst.cluster_intention_line}}\nautosaver_enabled = {{autosaver_enabled}}\n\n[SHARD]\nmaster_port = {{ports.master.port}}\n\n[STEAM]\nsteam_group_only = {{steam_group_only}}\nsteam_group_id = {{steam_group_id}}\nsteam_group_admins = {{steam_group_admins}}\n",
        )
        .unwrap();
    fs::write(
        templates_root.join("cluster_token.txt.hbs"),
        "{{cluster_token}}\n",
    )
    .unwrap();
    fs::write(templates_root.join("adminlist.txt.hbs"), "{{admin_list}}\n").unwrap();
    fs::write(templates_root.join("whitelist.txt.hbs"), "{{whitelist}}\n").unwrap();
    fs::write(templates_root.join("blocklist.txt.hbs"), "{{blocklist}}\n").unwrap();
    fs::write(
        templates_root
            .join("Master")
            .join("worldgenoverride.lua.hbs"),
        "\u{feff}{{dst.master_worldgenoverride}}",
    )
    .unwrap();
    fs::write(
        templates_root
            .join("Caves")
            .join("worldgenoverride.lua.hbs"),
        "\u{feff}{{dst.caves_worldgenoverride}}",
    )
    .unwrap();
    fs::write(
        templates_root.join("Master").join("modoverrides.lua.hbs"),
        "\u{feff}{{dst.master_modoverrides}}",
    )
    .unwrap();
    fs::write(
        templates_root.join("Caves").join("modoverrides.lua.hbs"),
        "\u{feff}{{dst.caves_modoverrides}}",
    )
    .unwrap();
    fs::write(
            templates_root.join("runtime-paths.txt.hbs"),
            "install_root={{paths.install_root}}\nconfig_dir={{paths.config_dir}}\ndata_dir={{paths.data_dir}}\nlogs_dir={{paths.logs_dir}}\nsaves_dir={{paths.saves_dir}}\n",
        )
        .unwrap();
}

fn declared_save_path_test_descriptor(root: &Path) -> ModuleDescriptor {
    ModuleDescriptor {
        root: root.join("modules").join("savepathtest"),
        manifest_toml: String::from(
            "id = \"savepathtest\"\nname = \"Save Path Test\"\nversion = \"0.1.0\"\nsupported_platforms = [\"windows\"]\n\n[storage]\nsaves_path_template = \"{{paths.config_dir}}/savegame\"\n",
        ),
        schema_json: None,
        default_ports: vec![],
        install: None,
        process: None,
        workshop: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
        storage: app_modules::ModuleStorageSpec {
            saves_path_template: Some(String::from("{{paths.config_dir}}/savegame")),
            ..Default::default()
        },
        summary: ModuleSummary {
            id: String::from("savepathtest"),
            name: String::from("Save Path Test"),
            version: String::from("0.1.0"),
            description: Some(String::from("Test module for declared save roots")),
            steam_app_id: None,
            install_state: InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
    }
}

fn prepare_declared_save_path_environment(root: &Path, descriptor: &ModuleDescriptor) {
    let paths = test_paths(root);
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    fs::create_dir_all(&paths.logs_root).unwrap();
    fs::create_dir_all(&paths.steamcmd_root).unwrap();
    fs::create_dir_all(&paths.games_root).unwrap();
    prepare_shared_install(&paths, descriptor);
    fs::create_dir_all(&paths.instances_root).unwrap();
    write_module_descriptor_files(descriptor);
}

include!("tests/instance_deletion.rs");

#[tokio::test]
async fn create_instance_allocates_ports_and_writes_config() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let first = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("DST Alpha"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap();

    replenish_test_library(&paths, &descriptor).await;
    let second = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("DST Beta"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap();

    assert_eq!(first.summary.port_count, 2);
    assert_eq!(first.ports[0].port, 10999);
    assert_eq!(first.ports[1].port, 11000);
    assert_eq!(second.summary.port_count, 2);
    assert_eq!(second.ports[0].port, 11001);
    assert_eq!(second.ports[1].port, 11002);

    let config_text = fs::read_to_string(&first.config_file_path).unwrap();
    assert!(config_text.contains("\"cluster_name\": \"DST Alpha\""));
    assert!(config_text.contains("\"ports\""));

    let config_root = root
        .join("instances")
        .join(&first.summary.id)
        .join("config");
    let expected_saves_root = config_root.join("clusters").join("main");
    let details = read_instance_details(&paths, &first.summary.id)
        .await
        .unwrap();
    assert_eq!(PathBuf::from(&details.saves_path), expected_saves_root);
    assert!(details.backup_uses_declared_saves_path);
    let cluster_ini_path = config_root.join("cluster.ini");
    let runtime_paths_path = config_root.join("runtime-paths.txt");
    let master_worldgen_path = config_root.join("Master").join("worldgenoverride.lua");
    let caves_worldgen_path = config_root.join("Caves").join("worldgenoverride.lua");
    let master_modoverrides_path = config_root.join("Master").join("modoverrides.lua");
    let caves_modoverrides_path = config_root.join("Caves").join("modoverrides.lua");
    let whitelist_path = config_root.join("whitelist.txt");
    let blocklist_path = config_root.join("blocklist.txt");
    let shared_mod_setup_path = paths
        .games_root
        .join("dontstarve")
        .join("mods")
        .join("dedicated_server_mods_setup.lua");
    let first_runtime_root = paths.instances_root.join(&first.summary.id).join("runtime");
    let first_mod_setup_path = first_runtime_root.join("mods/dedicated_server_mods_setup.lua");
    let second_mod_setup_path = paths
        .instances_root
        .join(&second.summary.id)
        .join("runtime/mods/dedicated_server_mods_setup.lua");

    let cluster_ini_text = fs::read_to_string(&cluster_ini_path).unwrap();
    assert!(cluster_ini_text.contains("cluster_name = DST Alpha"));
    assert!(cluster_ini_text.contains("bind_ip = 0.0.0.0"));
    assert!(cluster_ini_text.contains("tick_rate = 15"));
    assert!(cluster_ini_text.contains("whitelist_slots = 0"));
    assert!(cluster_ini_text.contains("autosaver_enabled = true"));
    assert!(cluster_ini_text.contains("master_port = 10999"));
    assert!(cluster_ini_text.contains("vote_enabled = true"));
    assert!(cluster_ini_text.contains("steam_group_only = false"));
    assert!(cluster_ini_text.contains("steam_group_id = 0"));
    assert!(cluster_ini_text.contains("steam_group_admins = false"));
    assert!(!cluster_ini_text.contains("cluster_intention ="));

    let runtime_paths_text = fs::read_to_string(&runtime_paths_path).unwrap();
    assert!(runtime_paths_text.contains(&format!(
        "install_root={}",
        first_runtime_root.to_string_lossy()
    )));
    assert!(runtime_paths_text.contains(&format!("config_dir={}", config_root.to_string_lossy())));
    assert!(runtime_paths_text.contains(&format!(
                "data_dir={}",
                root.join("instances")
                    .join(&first.summary.id)
                    .join("data")
                    .to_string_lossy()
            )));
    assert!(runtime_paths_text.contains(&format!(
                "logs_dir={}",
                root.join("instances")
                    .join(&first.summary.id)
                    .join("logs")
                    .to_string_lossy()
            )));
    assert!(runtime_paths_text.contains(&format!(
        "saves_dir={}",
        expected_saves_root.to_string_lossy()
    )));

    let master_worldgen_text = fs::read_to_string(&master_worldgen_path).unwrap();
    assert!(master_worldgen_text.contains("settings_preset = \"SURVIVAL_TOGETHER\""));
    assert!(master_worldgen_text.contains("worldgen_preset = \"SURVIVAL_TOGETHER\""));
    assert!(master_worldgen_text.contains("specialevent = \"default\""));
    assert!(master_worldgen_text.contains("world_size = \"default\""));
    assert!(master_worldgen_text.contains("season_start = \"default\""));
    assert!(master_worldgen_text.contains("task_set = \"default\""));
    assert!(master_worldgen_text.contains("start_location = \"default\""));
    assert!(master_worldgen_text.contains("touchstone = \"default\""));
    assert!(master_worldgen_text.contains("healthpenalty = \"always\""));
    assert!(master_worldgen_text.contains("regrowth = \"default\""));
    assert!(master_worldgen_text.contains("trees = \"default\""));
    assert!(master_worldgen_text.contains("flowers = \"default\""));
    assert!(master_worldgen_text.contains("ponds = \"default\""));
    assert!(master_worldgen_text.contains("beefalo = \"default\""));
    assert!(master_worldgen_text.contains("spiders = \"default\""));
    assert!(master_worldgen_text.contains("penguins = \"default\""));
    assert!(master_worldgen_text.contains("angrybees = \"default\""));
    assert!(master_worldgen_text.contains("krampus = \"default\""));
    assert!(master_worldgen_text.contains("klaus = \"default\""));
    for expected in [
        "beefaloheat = \"default\"",
        "deciduoustree_regrowth = \"default\"",
        "bees_setting = \"default\"",
        "mutated_hounds = \"default\"",
        "moon_tree = \"default\"",
        "ocean_bullkelp = \"default\"",
        "ocean_waterplant = \"ocean_default\"",
        "moon_spiders = \"default\"",
    ] {
        assert!(
            master_worldgen_text.contains(expected),
            "missing {expected}"
        );
    }

    let caves_worldgen_text = fs::read_to_string(&caves_worldgen_path).unwrap();
    assert!(caves_worldgen_text.contains("settings_preset = \"DST_CAVE\""));
    assert!(caves_worldgen_text.contains("worldgen_preset = \"DST_CAVE\""));
    assert!(caves_worldgen_text.contains("world_size = \"default\""));
    assert!(caves_worldgen_text.contains("atriumgate = \"default\""));
    assert!(caves_worldgen_text.contains("earthquakes = \"default\""));
    assert!(caves_worldgen_text.contains("regrowth = \"default\""));
    assert!(caves_worldgen_text.contains("banana = \"default\""));
    assert!(caves_worldgen_text.contains("wormlights = \"default\""));
    assert!(caves_worldgen_text.contains("mushtree = \"default\""));
    assert!(caves_worldgen_text.contains("bunnymen = \"default\""));
    assert!(caves_worldgen_text.contains("snurtles = \"default\""));
    assert!(caves_worldgen_text.contains("cave_spiders = \"default\""));
    assert!(caves_worldgen_text.contains("nightmarecreatures = \"default\""));
    assert!(caves_worldgen_text.contains("fissure = \"default\""));
    for expected in [
        "flower_cave_regrowth = \"default\"",
        "lightfliers = \"default\"",
        "spider_hider = \"default\"",
        "spiderqueen = \"default\"",
        "grass = \"default\"",
        "chess = \"default\"",
    ] {
        assert!(caves_worldgen_text.contains(expected), "missing {expected}");
    }
    for inherited in [
        "specialevent = \"default\"",
        "healthpenalty = \"always\"",
        "day = \"default\"",
        "beefaloheat = \"default\"",
        "krampus = \"default\"",
    ] {
        assert!(
            caves_worldgen_text.contains(inherited),
            "Caves did not inherit Master option {inherited}"
        );
    }
    for master_only in [
        "season_start =",
        "autumn =",
        "winter =",
        "spring =",
        "summer =",
        "deerclops =",
        "ocean_waterplant =",
    ] {
        assert!(
            !caves_worldgen_text.contains(master_only),
            "Caves emitted surface-only key {master_only}"
        );
    }

    assert_file_has_no_utf8_bom(&master_worldgen_path);
    assert_file_has_no_utf8_bom(&caves_worldgen_path);
    assert_file_has_no_utf8_bom(&master_modoverrides_path);
    assert_file_has_no_utf8_bom(&caves_modoverrides_path);

    assert_eq!(
        fs::read_to_string(&master_modoverrides_path).unwrap(),
        "return {\n}\n"
    );
    assert_eq!(
        fs::read_to_string(&caves_modoverrides_path).unwrap(),
        "return {\n}\n"
    );
    assert_eq!(fs::read_to_string(&whitelist_path).unwrap(), "\n");
    assert_eq!(fs::read_to_string(&blocklist_path).unwrap(), "\n");

    assert!(!shared_mod_setup_path.exists());
    for path in [&first_mod_setup_path, &second_mod_setup_path] {
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("Generated by LanGame Server Manager"));
        assert!(text.contains("No Workshop mods configured"));
    }
    assert_ne!(first_mod_setup_path, second_mod_setup_path);

    let instances = list_instances(&paths).await.unwrap();
    assert_eq!(instances.len(), 2);
    assert_eq!(instances[0].port_count, 2);
    assert_eq!(instances[1].port_count, 2);

    cleanup_root(&root);
}

#[tokio::test]
async fn create_rejects_invalid_non_deferred_defaults_before_runtime_copy_or_persistence() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let mut descriptor = declared_save_path_test_descriptor(&root);
    descriptor.schema_json = Some(String::from(
        r#"{
          "type": "object",
          "properties": {
            "enabled": { "type": "boolean", "default": true }
          },
          "required": ["enabled"]
        }"#,
    ));
    prepare_declared_save_path_environment(&root, &descriptor);
    fs::create_dir_all(paths.games_root.join(&descriptor.summary.id)).unwrap();
    fs::write(
        paths
            .games_root
            .join(&descriptor.summary.id)
            .join("shared-server.bin"),
        b"shared runtime",
    )
    .unwrap();

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Valid First Instance"),
            module_id: descriptor.summary.id.clone(),
        },
    )
    .await
    .unwrap();
    let instance_count_before = fs::read_dir(&paths.instances_root).unwrap().count();

    descriptor.schema_json = Some(String::from(
        r#"{
          "type": "object",
          "properties": {
            "enabled": { "type": "boolean", "default": 1 }
          },
          "required": ["enabled"]
        }"#,
    ));
    let error = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Invalid Second Instance"),
            module_id: descriptor.summary.id.clone(),
        },
    )
    .await
    .expect_err("ordinary invalid defaults must fail before provisioning side effects");
    assert!(
        error.to_string().contains("enabled") && error.to_string().contains("boolean"),
        "unexpected creation validation error: {error}"
    );
    assert_eq!(
        fs::read_dir(&paths.instances_root).unwrap().count(),
        instance_count_before,
        "failed validation must not leave a private runtime directory"
    );
    let pool = connect_pool(&paths).await.unwrap();
    let persisted_instances: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM instances")
        .fetch_one(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert_eq!(persisted_instances, 1);

    cleanup_root(&root);
}

#[tokio::test]
async fn failed_private_runtime_copy_removes_the_new_instance_directory_and_staging_runtime() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let library_root = paths.games_root.join(&descriptor.summary.id);
    fs::write(
        library_root.join("program.bin"),
        b"preserved downloaded program",
    )
    .unwrap();
    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: descriptor.summary.id.clone(),
            install_root: library_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some("test-build".into()),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();
    let library_before = crate::read_library_program_install(&paths, &descriptor.summary.id)
        .await
        .unwrap()
        .unwrap();
    let library_files = crate::test_file_snapshot::tree_snapshot(&library_root).unwrap();

    let existing = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Existing Instance"),
            module_id: descriptor.summary.id.clone(),
        },
    )
    .await
    .unwrap();
    let existing_root = Path::new(&existing.config_file_path)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&library_root).unwrap(),
        library_files
    );
    let existing_files = crate::test_file_snapshot::tree_snapshot(&existing_root).unwrap();
    let retained_library = root.join("retained-library");
    fs::rename(&library_root, &retained_library).unwrap();
    assert!(!library_root.exists());

    let error = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Copy Must Fail"),
            module_id: descriptor.summary.id.clone(),
        },
    )
    .await
    .expect_err("a missing library must fail the private copy");

    assert!(
        error
            .to_string()
            .contains("requires a separate existing program directory")
    );
    assert_eq!(
        managed_instance_directories(&paths),
        vec![existing_root.clone()]
    );
    assert!(
        managed_instance_directories(&paths)
            .iter()
            .all(|instance_root| !instance_root.join("runtime.staging").exists())
    );
    assert_eq!(list_instances(&paths).await.unwrap().len(), 1);
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&existing_root).unwrap(),
        existing_files
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&retained_library).unwrap(),
        library_files
    );
    let library_after = crate::read_library_program_install(&paths, &descriptor.summary.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(library_after.id, library_before.id);
    assert_eq!(
        library_after.current_version,
        library_before.current_version
    );
    assert_eq!(library_after.install_root, library_root);
    assert_eq!(library_after.install_state, InstallState::Installed);
    assert_eq!(library_after.scope, crate::ProgramInstallScope::Library);
    assert_eq!(library_after.owner_instance_id, None);

    cleanup_root(&root);
}

#[tokio::test]
async fn template_failure_rolls_back_the_database_and_new_instance_directory() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = declared_save_path_test_descriptor(&root);
    prepare_declared_save_path_environment(&root, &descriptor);
    let templates_root = descriptor.root.join("templates");
    fs::create_dir_all(&templates_root).unwrap();
    fs::write(templates_root.join("invalid.txt.hbs"), [0xff]).unwrap();
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let error = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Invalid Template"),
            module_id: descriptor.summary.id.clone(),
        },
    )
    .await
    .expect_err("invalid UTF-8 template must fail creation");

    assert!(error.to_string().contains("failed to read config file"));
    assert!(managed_instance_directories(&paths).is_empty());
    assert!(list_instances(&paths).await.unwrap().is_empty());

    cleanup_root(&root);
}

#[tokio::test]
async fn database_insert_failure_removes_the_new_instance_directory() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = declared_save_path_test_descriptor(&root);
    prepare_declared_save_path_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let pool = connect_pool(&paths).await.unwrap();
    sqlx::query(
        "CREATE TRIGGER fail_instance_insert \
         BEFORE INSERT ON instances \
         BEGIN SELECT RAISE(ABORT, 'injected instance insert failure'); END",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let error = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Database Failure"),
            module_id: descriptor.summary.id.clone(),
        },
    )
    .await
    .expect_err("the injected insert trigger must fail creation");

    assert!(
        error
            .to_string()
            .contains("injected instance insert failure")
    );
    assert!(managed_instance_directories(&paths).is_empty());
    assert!(list_instances(&paths).await.unwrap().is_empty());

    cleanup_root(&root);
}

#[tokio::test]
async fn materialize_instance_configuration_backfills_schema_defaults_for_sparse_config() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Sparse Legacy Instance"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap();

    let config_file_path = PathBuf::from(&created.config_file_path);
    fs::write(
        &config_file_path,
        serde_json::json!({
            "settings": {
                "cluster_name": "Preserved Legacy Name"
            }
        })
        .to_string(),
    )
    .unwrap();

    let details = materialize_instance_configuration(&paths, &created.summary.id)
        .await
        .unwrap();
    let settings: Value = serde_json::from_str(&details.settings_json).unwrap();

    assert_eq!(
        settings.get("cluster_name").and_then(Value::as_str),
        Some("Preserved Legacy Name")
    );
    assert_eq!(settings.get("max_players").and_then(Value::as_u64), Some(6));
    assert_eq!(
        settings.get("bind_ip").and_then(Value::as_str),
        Some("0.0.0.0")
    );

    let cluster_ini = fs::read_to_string(
        root.join("instances")
            .join(&created.summary.id)
            .join("config")
            .join("cluster.ini"),
    )
    .unwrap();
    assert!(cluster_ini.contains("cluster_name = Preserved Legacy Name"));
    assert!(cluster_ini.contains("max_players = 6"));
    assert!(!cluster_ini.contains("{{"));

    cleanup_root(&root);
}

include!("tests/instance_ports.rs");
include!("tests/game_install_sync.rs");
include!("tests/runtime_overview.rs");

#[tokio::test]
async fn dragonwilds_unassigned_owner_round_trip_preserves_native_state() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = runescape_dragonwilds_test_descriptor(&root);
    prepare_runescape_dragonwilds_environment(&root, &descriptor);
    fs::create_dir_all(paths.games_root.join("runescapedragonwilds")).unwrap();

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Dragonwilds Name Longer Than Fifteen Characters"),
            module_id: String::from("runescapedragonwilds"),
        },
    )
    .await
    .expect("deferred owner_id must not prevent instance creation");
    let created_details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let mut settings = serde_json::from_str::<Value>(&created_details.settings_json)
        .unwrap()
        .as_object()
        .unwrap()
        .clone();
    assert_eq!(settings.get("owner_id").and_then(Value::as_str), Some(""));
    assert_eq!(
        settings.get("server_name").and_then(Value::as_str),
        Some("Dragonwilds"),
        "bounded server_name must use its valid schema default rather than an overlong instance name"
    );

    let native_path = instance_private_runtime_root(&created)
        .join("RSDragonwilds/Saved/Config/WindowsServer/DedicatedServer.ini");
    let unassigned = materialize_instance_configuration(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&unassigned.settings_json).unwrap()["owner_id"],
        ""
    );
    assert!(
        fs::read_to_string(&native_path)
            .unwrap()
            .contains("OwnerId=LGSM_UNASSIGNED_OWNER")
    );
    let mut native = fs::read_to_string(&native_path).unwrap();
    native.push_str("ServerGuid=preserved-guid\nAdminUsers=preserved-admin\n");
    fs::write(&native_path, native).unwrap();

    settings.insert(
        String::from("owner_id"),
        Value::String(String::from("00000000000000000000000000000001")),
    );
    update_instance(
        &paths,
        UpdateInstanceInput {
            id: created.summary.id.clone(),
            bind_ip: created_details.summary.bind_ip.clone(),
            auto_backup_on_stop: created_details.auto_backup_on_stop,
            backup_retention_count: created_details.backup_retention_count,
            settings_json: Value::Object(settings).to_string(),
            ports: created_details.ports,
        },
    )
    .await
    .unwrap();
    let materialized = materialize_instance_configuration(&paths, &created.summary.id)
        .await
        .expect("configured owner_id must satisfy full materialization");
    assert!(
        materialized
            .settings_json
            .contains("00000000000000000000000000000001")
    );

    let native = fs::read_to_string(&native_path).unwrap();
    assert!(native.contains("OwnerId=00000000000000000000000000000001"));
    assert!(!native.contains("LGSM_UNASSIGNED_OWNER"));
    assert!(native.contains("ServerGuid=preserved-guid"));
    assert!(native.contains("AdminUsers=preserved-admin"));

    cleanup_root(&root);
}

#[tokio::test]
async fn update_instance_rejects_schema_string_constraints() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = runescape_dragonwilds_test_descriptor(&root);
    prepare_runescape_dragonwilds_environment(&root, &descriptor);
    prepare_shared_install(&paths, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Dragonwilds Schema"),
            module_id: String::from("runescapedragonwilds"),
        },
    )
    .await
    .unwrap();

    let unassigned_owner = update_instance(
        &paths,
        UpdateInstanceInput {
            id: created.summary.id.clone(),
            bind_ip: String::from("0.0.0.0"),
            auto_backup_on_stop: false,
            backup_retention_count: 1,
            settings_json: json!({
                "owner_id": "",
                "server_name": "Dragonwilds",
                "default_world_name": "Ashenfall"
            })
            .to_string(),
            ports: created.ports.clone(),
        },
    )
    .await
    .expect("empty owner_id represents an explicitly unassigned owner");
    assert_eq!(
        serde_json::from_str::<Value>(&unassigned_owner.settings_json).unwrap()["owner_id"],
        ""
    );

    let invalid_world_name = update_instance(
        &paths,
        UpdateInstanceInput {
            id: created.summary.id.clone(),
            bind_ip: String::from("0.0.0.0"),
            auto_backup_on_stop: false,
            backup_retention_count: 1,
            settings_json: json!({
                "owner_id": "76561198000000000",
                "server_name": "Dragonwilds",
                "default_world_name": "DragonwildsWorldNameTooLong"
            })
            .to_string(),
            ports: created.ports.clone(),
        },
    )
    .await
    .expect_err("overlong Dragonwilds world name should be rejected before config materialization");
    assert!(
        invalid_world_name
            .to_string()
            .contains("default_world_name")
            && invalid_world_name.to_string().contains("at most 15"),
        "unexpected default_world_name validation error: {invalid_world_name}"
    );

    let whitespace_owner = update_instance(
        &paths,
        UpdateInstanceInput {
            id: created.summary.id.clone(),
            bind_ip: String::from("0.0.0.0"),
            auto_backup_on_stop: false,
            backup_retention_count: 1,
            settings_json: json!({
                "owner_id": "   ",
                "server_name": "Dragonwilds",
                "default_world_name": "Ashenfall"
            })
            .to_string(),
            ports: created.ports.clone(),
        },
    )
    .await
    .expect_err(
        "whitespace-only Dragonwilds owner_id should be rejected before config materialization",
    );
    assert!(
        whitespace_owner.to_string().contains("owner_id")
            && whitespace_owner.to_string().contains("pattern"),
        "unexpected whitespace owner_id validation error: {whitespace_owner}"
    );

    cleanup_root(&root);
}

#[tokio::test]
async fn update_instance_if_current_rejects_a_stale_settings_baseline_atomically() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Settings CAS"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap();
    let baseline = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let mut first_settings = serde_json::from_str::<Value>(&baseline.settings_json)
        .unwrap()
        .as_object()
        .unwrap()
        .clone();
    first_settings.insert(
        String::from("cluster_name"),
        Value::String(String::from("First writer")),
    );
    update_instance_if_current(
        &paths,
        UpdateInstanceInput {
            id: created.summary.id.clone(),
            bind_ip: baseline.summary.bind_ip.clone(),
            auto_backup_on_stop: baseline.auto_backup_on_stop,
            backup_retention_count: baseline.backup_retention_count,
            settings_json: Value::Object(first_settings).to_string(),
            ports: baseline.ports.clone(),
        },
        &baseline.settings_json,
    )
    .await
    .expect("first writer should match the settings baseline");

    let mut stale_settings = serde_json::from_str::<Value>(&baseline.settings_json)
        .unwrap()
        .as_object()
        .unwrap()
        .clone();
    stale_settings.insert(
        String::from("cluster_name"),
        Value::String(String::from("Stale writer")),
    );
    let stale = update_instance_if_current(
        &paths,
        UpdateInstanceInput {
            id: created.summary.id.clone(),
            bind_ip: baseline.summary.bind_ip.clone(),
            auto_backup_on_stop: baseline.auto_backup_on_stop,
            backup_retention_count: baseline.backup_retention_count,
            settings_json: Value::Object(stale_settings).to_string(),
            ports: baseline.ports,
        },
        &baseline.settings_json,
    )
    .await
    .expect_err("stale settings baseline must not overwrite the first writer");
    assert!(matches!(
        stale,
        StorageError::InstanceSettingsPreconditionFailed { .. }
    ));

    let after = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let after_settings = serde_json::from_str::<Value>(&after.settings_json).unwrap();
    assert_eq!(after_settings["cluster_name"], json!("First writer"));

    cleanup_root(&root);
}

#[tokio::test]
async fn cross_process_instance_lock_guards_normal_and_player_access_writes() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Cross Process Settings Lock"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap();
    let baseline = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let ready_path = root.join("instance-child-ready");
    let release_path = root.join("instance-child-release");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "instance_settings_lock::tests::hold_instance_mutation_lock_in_child_process",
            "--nocapture",
        ])
        .env("LSGM_INSTANCE_LOCK_TEST_ROOT", &root)
        .env("LSGM_INSTANCE_LOCK_TEST_ID", &created.summary.id)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    for _ in 0..500 {
        if ready_path.exists() {
            break;
        }
        assert!(
            child.try_wait().unwrap().is_none(),
            "instance lock child exited early"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(ready_path.exists(), "child did not acquire instance lock");

    let normal_write = update_instance_if_current(
        &paths,
        UpdateInstanceInput {
            id: created.summary.id.clone(),
            bind_ip: baseline.summary.bind_ip.clone(),
            auto_backup_on_stop: baseline.auto_backup_on_stop,
            backup_retention_count: baseline.backup_retention_count,
            settings_json: baseline.settings_json.clone(),
            ports: baseline.ports.clone(),
        },
        &baseline.settings_json,
    )
    .await;
    let player_access_write = apply_instance_player_access_mutation(
        &paths,
        ApplyInstancePlayerAccessMutationInput {
            instance_id: created.summary.id.clone(),
            field_key: String::from("blocklist"),
            operation: PlayerAccessMutationOperation::Add,
            value: json!("KU_cross_process"),
            expected_value: Some(json!("")),
        },
    )
    .await;
    fs::write(&release_path, b"release").unwrap();
    let child_status = child.wait().unwrap();

    assert!(matches!(
        normal_write,
        Err(StorageError::InstanceSettingsLocked { .. })
    ));
    assert!(matches!(
        player_access_write,
        Err(StorageError::InstanceSettingsLocked { .. })
    ));
    assert!(child_status.success());
    assert_eq!(
        read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap()
            .settings_json,
        baseline.settings_json
    );

    cleanup_root(&root);
}

#[tokio::test]
async fn update_instance_rewrites_ports_and_settings() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("DST Gamma"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap();

    update_instance_autostart(&paths, &created.summary.id, true)
        .await
        .unwrap();
    let updated = update_instance(
            &paths,
            UpdateInstanceInput {
                id: created.summary.id.clone(),
                bind_ip: String::from("127.0.0.1"),
                auto_backup_on_stop: true,
                backup_retention_count: 4,
                settings_json: String::from(
                    r#"{
  "cluster_name": "Custom Gamma",
  "cluster_description": "Updated cluster",
  "max_players": 12,
  "cluster_password": "lanpass",
  "cluster_intention": "social",
  "pause_when_empty": true,
  "pvp": true,
  "vote_enabled": false,
  "offline_cluster": false,
  "lan_only_cluster": false,
  "tick_rate": 30,
  "autosaver_enabled": false,
  "enable_caves": true,
  "world_specialevent": "none",
  "world_autumn": "longseason",
  "world_winter": "noseason",
  "world_spring": "shortseason",
  "world_summer": "noseason",
  "world_extrastartingitems": "15",
  "world_seasonalstartingitems": "never",
  "world_spawnprotection": "always",
  "world_dropeverythingondespawn": "always",
  "world_darkness": "nonlethal",
  "world_temperaturedamage": "nonlethal",
  "world_hunger": "nonlethal",
  "world_healthpenalty": "none",
  "world_shadowcreatures": "often",
  "world_brightmarecreatures": "rare",
  "master_world_size": "huge",
  "master_season_start": "winter",
  "master_task_set": "classic",
  "master_start_location": "plus",
  "master_day": "longday",
  "master_branching": "most",
  "master_loop": "always",
  "master_touchstone": "often",
  "master_roads": "never",
  "master_boons": "often",
  "master_prefabswaps_start": "highly random",
  "master_petrification": "many",
  "master_meteorshowers": "often",
  "master_regrowth": "fast",
  "master_weather": "rare",
  "master_frogs": "often",
  "master_hounds": "never",
  "master_lightning": "rare",
  "master_wildfires": "never",
  "master_berrybush": "mostly",
  "master_carrot": "often",
  "master_flint": "rare",
  "master_grass": "always",
  "master_marshbush": "uncommon",
  "master_reeds": "mostly",
  "master_rock": "often",
  "master_sapling": "always",
  "master_trees": "insane",
  "master_flowers": "mostly",
  "master_ponds": "rare",
  "master_tumbleweed": "often",
  "master_bees": "mostly",
  "master_beefalo": "often",
  "master_butterfly": "always",
  "master_buzzard": "rare",
  "master_catcoon": "often",
  "master_moles": "mostly",
  "master_pigs": "often",
  "master_rabbits": "always",
  "master_lightninggoat": "rare",
  "master_spiders": "mostly",
  "master_tallbirds": "often",
  "master_tentacles": "rare",
  "master_penguins": "often",
  "master_perd": "rare",
  "master_angrybees": "mostly",
  "master_chess": "rare",
  "master_krampus": "often",
  "master_walrus": "rare",
  "master_merm": "often",
  "master_houndmound": "rare",
  "master_lureplants": "often",
  "master_bearger": "rare",
  "master_beequeen": "often",
  "master_deerclops": "rare",
  "master_dragonfly": "often",
  "master_klaus": "rare",
  "master_goosemoose": "rare",
  "master_spiderqueen": "often",
  "master_liefs": "rare",
  "master_toadstool": "often",
  "master_beefaloheat": "often",
  "master_deciduoustree_regrowth": "veryfast",
  "master_carrots_regrowth": "slow",
  "master_evergreen_regrowth": "fast",
  "master_flowers_regrowth": "veryslow",
  "master_moon_tree_regrowth": "fast",
  "master_saltstack_regrowth": "slow",
  "master_twiggytrees_regrowth": "veryfast",
  "master_bees_setting": "often",
  "master_birds": "rare",
  "master_bunnymen_setting": "often",
  "master_catcoons": "rare",
  "master_gnarwail": "rare",
  "master_grassgekkos": "often",
  "master_moles_setting": "always",
  "master_fishschools": "often",
  "master_wobsters": "rare",
  "master_bats_setting": "never",
  "master_cookiecutters": "rare",
  "master_mutated_hounds": "never",
  "master_wasps": "often",
  "master_merms": "rare",
  "master_penguins_moon": "never",
  "master_mosquitos": "often",
  "master_sharks": "rare",
  "master_moon_spider": "often",
  "master_squid": "rare",
  "master_spider_warriors": "never",
  "master_antliontribute": "often",
  "master_crabking": "rare",
  "master_fruitfly": "often",
  "master_malbatross": "rare",
  "master_deciduousmonster": "often",
  "master_mushroom": "uncommon",
  "master_moon_fissure": "rare",
  "master_moon_starfish": "mostly",
  "master_moon_bullkelp": "often",
  "master_ocean_bullkelp": "often",
  "master_cactus": "rare",
  "master_moon_hotspring": "often",
  "master_moon_rock": "mostly",
  "master_moon_sapling": "rare",
  "master_moon_tree": "often",
  "master_meteorspawner": "rare",
  "master_rock_ice": "uncommon",
  "master_ocean_seastack": "ocean_rare",
  "master_moon_berrybush": "mostly",
  "master_moon_carrot": "often",
  "master_moon_fruitdragon": "rare",
  "master_ocean_shoal": "often",
  "master_ocean_wobsterden": "rare",
  "master_ocean_waterplant": "ocean_mostly",
  "master_moon_spiders": "mostly",
  "master_world_overrides_extra": "custom_master_override = \"present\",\n",
  "caves_world_size": "small",
  "caves_branching": "least",
  "caves_loop": "always",
  "caves_atriumgate": "never",
  "caves_wormattacks": "rare",
  "caves_earthquakes": "often",
  "caves_regrowth": "slow",
  "caves_banana": "often",
  "caves_cave_ponds": "rare",
  "caves_fern": "mostly",
  "caves_flint": "often",
  "caves_lichen": "always",
  "caves_marshbush": "uncommon",
  "caves_mushroom": "mostly",
  "caves_wormlights": "mostly",
  "caves_flower_cave": "often",
  "caves_mushtree": "rare",
  "caves_rock": "often",
  "caves_sapling": "rare",
  "caves_bunnymen": "mostly",
  "caves_rocky": "rare",
  "caves_slurper": "often",
  "caves_slurtles": "mostly",
  "caves_snurtles": "often",
  "caves_monkey": "often",
  "caves_bats": "rare",
  "caves_worms": "mostly",
  "caves_spiders": "often",
  "caves_cave_spiders": "rare",
  "caves_tentacles": "often",
  "caves_molebats": "rare",
  "caves_nightmarecreatures": "often",
  "caves_spider_dropper": "rare",
  "caves_spider_spitter": "often",
  "caves_fruitfly": "rare",
  "caves_fissure": "often",
  "caves_day": "longdusk",
  "caves_beefaloheat": "rare",
  "caves_krampus": "often",
  "caves_weather": "rare",
  "caves_flower_cave_regrowth": "fast",
  "caves_lightflier_flower_regrowth": "slow",
  "caves_mushtree_moon_regrowth": "veryfast",
  "caves_mushtree_regrowth": "slow",
  "caves_lightfliers": "often",
  "caves_bunnymen_setting": "rare",
  "caves_dustmoths": "often",
  "caves_grassgekkos": "never",
  "caves_moles_setting": "rare",
  "caves_mushgnome": "often",
  "caves_pigs_setting": "never",
  "caves_rocky_setting": "rare",
  "caves_slurtles_setting": "often",
  "caves_monkey_setting": "rare",
  "caves_bats_setting": "often",
  "caves_spider_hider": "often",
  "caves_merms": "rare",
  "caves_spider_warriors": "never",
  "caves_spiders_setting": "often",
  "caves_spiderqueen": "rare",
  "caves_toadstool": "often",
  "caves_liefs": "rare",
  "caves_grass": "mostly",
  "caves_reeds": "rare",
  "caves_trees": "often",
  "caves_chess": "rare",
  "caves_world_overrides_extra": "custom_caves_override = \"present\",\n",
  "caves_worldgenoverride_lua": "return {\n  override_enabled = true,\n  settings_preset = \"DST_CAVE\",\n  worldgen_preset = \"DST_CAVE\",\n  overrides = {\n    world_size = \"default\",\n  }\n}\n",
  "cluster_token": "token-123",
  "admin_list": "KU_admin_a\nKU_admin_b",
  "whitelist": "KU_white_a",
  "blocklist": "KU_block_a",
  "whitelist_slots": 2,
  "steam_group_only": true,
  "steam_group_id": 123456789,
  "steam_group_admins": true,
  "shared_workshop_mod_ids": "2039181790\nworkshop-2039181790\nhttps://steamcommunity.com/sharedfiles/filedetails/?id=1909182187",
  "shared_workshop_collection_ids": "3495871201",
  "master_enabled_workshop_mod_ids": "2039181790\n1909182187",
  "caves_enabled_workshop_mod_ids": "1909182187",
  "master_mod_configuration_options": {
    "2039181790": {
      "language": "zh",
      "range_ring": true,
      "marker_scale": 1.5
    }
  },
  "caves_mod_configuration_options": {
    "1909182187": {
      "language": "zh",
      "show_creature_age": false
    }
  },
  "master_modoverrides_lua": "return {\n}\n",
  "caves_modoverrides_lua": "return {\n}\n"
}"#,
                ),
                ports: vec![
                    PortBinding {
                        name: String::from("master"),
                        protocol: String::from("udp"),
                        port: 12000,
                    },
                    PortBinding {
                        name: String::from("backup"),
                        protocol: String::from("udp"),
                        port: 12001,
                    },
                ],
            },
        )
        .await
        .unwrap();

    let expected_saves_root = root
        .join("instances")
        .join(&created.summary.id)
        .join("config")
        .join("clusters")
        .join("main");

    assert_eq!(updated.summary.bind_ip, "127.0.0.1");
    assert!(updated.summary.autostart);
    assert_eq!(updated.ports[0].port, 12000);
    assert_eq!(updated.ports[1].port, 12001);
    assert!(updated.auto_backup_on_stop);
    assert_eq!(updated.backup_retention_count, 4);
    assert!(updated.settings_json.contains("Custom Gamma"));
    assert_eq!(PathBuf::from(&updated.saves_path), expected_saves_root);
    assert!(updated.backup_uses_declared_saves_path);

    let details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(details.summary.bind_ip, "127.0.0.1");
    assert!(details.auto_backup_on_stop);
    assert_eq!(details.backup_retention_count, 4);
    assert!(details.settings_json.contains("Custom Gamma"));
    assert_eq!(details.ports[0].port, 12000);
    assert_eq!(PathBuf::from(&details.saves_path), expected_saves_root);
    assert!(details.backup_uses_declared_saves_path);

    let config_text = fs::read_to_string(&updated.config_file_path).unwrap();
    assert!(config_text.contains("\"bind_ip\": \"127.0.0.1\""));
    assert!(config_text.contains("\"cluster_name\": \"Custom Gamma\""));

    let config_root = root
        .join("instances")
        .join(&created.summary.id)
        .join("config");
    let cluster_ini_path = config_root.join("cluster.ini");
    let cluster_ini_text = fs::read_to_string(&cluster_ini_path).unwrap();
    assert!(cluster_ini_text.contains("cluster_name = Custom Gamma"));
    assert!(cluster_ini_text.contains("bind_ip = 127.0.0.1"));
    assert!(cluster_ini_text.contains("offline_cluster = false"));
    assert!(cluster_ini_text.contains("lan_only_cluster = false"));
    assert!(cluster_ini_text.contains("pvp = true"));
    assert!(cluster_ini_text.contains("pause_when_empty = true"));
    assert!(cluster_ini_text.contains("vote_enabled = false"));
    assert!(cluster_ini_text.contains("tick_rate = 30"));
    assert!(cluster_ini_text.contains("whitelist_slots = 2"));
    assert!(cluster_ini_text.contains("cluster_intention = social"));
    assert!(cluster_ini_text.contains("autosaver_enabled = false"));
    assert!(cluster_ini_text.contains("master_port = 12000"));
    assert!(cluster_ini_text.contains("steam_group_only = true"));
    assert!(cluster_ini_text.contains("steam_group_id = 123456789"));
    assert!(cluster_ini_text.contains("steam_group_admins = true"));

    let cluster_token_path = config_root.join("cluster_token.txt");
    let admin_list_path = config_root.join("adminlist.txt");
    let whitelist_path = config_root.join("whitelist.txt");
    let blocklist_path = config_root.join("blocklist.txt");
    let shared_mod_setup_path = paths
        .games_root
        .join("dontstarve")
        .join("mods")
        .join("dedicated_server_mods_setup.lua");
    let instance_mod_setup_path = paths
        .instances_root
        .join(&created.summary.id)
        .join("runtime/mods/dedicated_server_mods_setup.lua");
    assert_eq!(
        fs::read_to_string(&cluster_token_path).unwrap(),
        "token-123\n"
    );
    assert_eq!(
        fs::read_to_string(&admin_list_path).unwrap(),
        "KU_admin_a\nKU_admin_b\n"
    );
    assert_eq!(fs::read_to_string(&whitelist_path).unwrap(), "KU_white_a\n");
    assert_eq!(fs::read_to_string(&blocklist_path).unwrap(), "KU_block_a\n");

    let master_worldgen_path = config_root.join("Master").join("worldgenoverride.lua");
    let caves_worldgen_path = config_root.join("Caves").join("worldgenoverride.lua");
    let master_modoverrides_path = config_root.join("Master").join("modoverrides.lua");
    let caves_modoverrides_path = config_root.join("Caves").join("modoverrides.lua");

    let master_worldgen_text = fs::read_to_string(&master_worldgen_path).unwrap();
    assert!(master_worldgen_text.contains("specialevent = \"none\""));
    assert!(master_worldgen_text.contains("autumn = \"longseason\""));
    assert!(master_worldgen_text.contains("winter = \"noseason\""));
    assert!(master_worldgen_text.contains("spring = \"shortseason\""));
    assert!(master_worldgen_text.contains("summer = \"noseason\""));
    assert!(master_worldgen_text.contains("extrastartingitems = \"15\""));
    assert!(master_worldgen_text.contains("seasonalstartingitems = \"never\""));
    assert!(master_worldgen_text.contains("spawnprotection = \"always\""));
    assert!(master_worldgen_text.contains("dropeverythingondespawn = \"always\""));
    assert!(master_worldgen_text.contains("darkness = \"nonlethal\""));
    assert!(master_worldgen_text.contains("temperaturedamage = \"nonlethal\""));
    assert!(master_worldgen_text.contains("hunger = \"nonlethal\""));
    assert!(master_worldgen_text.contains("healthpenalty = \"none\""));
    assert!(master_worldgen_text.contains("shadowcreatures = \"often\""));
    assert!(master_worldgen_text.contains("brightmarecreatures = \"rare\""));
    assert!(master_worldgen_text.contains("world_size = \"huge\""));
    assert!(master_worldgen_text.contains("season_start = \"winter\""));
    assert!(master_worldgen_text.contains("task_set = \"classic\""));
    assert!(master_worldgen_text.contains("start_location = \"plus\""));
    assert!(master_worldgen_text.contains("day = \"longday\""));
    assert!(master_worldgen_text.contains("branching = \"most\""));
    assert!(master_worldgen_text.contains("loop = \"always\""));
    assert!(master_worldgen_text.contains("touchstone = \"often\""));
    assert!(master_worldgen_text.contains("roads = \"never\""));
    assert!(master_worldgen_text.contains("prefabswaps_start = \"highly random\""));
    assert!(master_worldgen_text.contains("petrification = \"many\""));
    assert!(master_worldgen_text.contains("meteorshowers = \"often\""));
    assert!(master_worldgen_text.contains("regrowth = \"fast\""));
    assert!(master_worldgen_text.contains("weather = \"rare\""));
    assert!(master_worldgen_text.contains("frogs = \"often\""));
    assert!(master_worldgen_text.contains("hounds = \"never\""));
    assert!(master_worldgen_text.contains("lightning = \"rare\""));
    assert!(master_worldgen_text.contains("wildfires = \"never\""));
    assert!(master_worldgen_text.contains("berrybush = \"mostly\""));
    assert!(master_worldgen_text.contains("carrot = \"often\""));
    assert!(master_worldgen_text.contains("flint = \"rare\""));
    assert!(master_worldgen_text.contains("grass = \"always\""));
    assert!(master_worldgen_text.contains("marshbush = \"uncommon\""));
    assert!(master_worldgen_text.contains("reeds = \"mostly\""));
    assert!(master_worldgen_text.contains("rock = \"often\""));
    assert!(master_worldgen_text.contains("sapling = \"always\""));
    assert!(master_worldgen_text.contains("trees = \"insane\""));
    assert!(master_worldgen_text.contains("flowers = \"mostly\""));
    assert!(master_worldgen_text.contains("ponds = \"rare\""));
    assert!(master_worldgen_text.contains("tumbleweed = \"often\""));
    assert!(master_worldgen_text.contains("bees = \"mostly\""));
    assert!(master_worldgen_text.contains("beefalo = \"often\""));
    assert!(master_worldgen_text.contains("butterfly = \"always\""));
    assert!(master_worldgen_text.contains("buzzard = \"rare\""));
    assert!(master_worldgen_text.contains("catcoon = \"often\""));
    assert!(master_worldgen_text.contains("moles = \"mostly\""));
    assert!(master_worldgen_text.contains("pigs = \"often\""));
    assert!(master_worldgen_text.contains("rabbits = \"always\""));
    assert!(master_worldgen_text.contains("lightninggoat = \"rare\""));
    assert!(master_worldgen_text.contains("spiders = \"mostly\""));
    assert!(master_worldgen_text.contains("tallbirds = \"often\""));
    assert!(master_worldgen_text.contains("tentacles = \"rare\""));
    assert!(master_worldgen_text.contains("penguins = \"often\""));
    assert!(master_worldgen_text.contains("perd = \"rare\""));
    assert!(master_worldgen_text.contains("angrybees = \"mostly\""));
    assert!(master_worldgen_text.contains("chess = \"rare\""));
    assert!(master_worldgen_text.contains("krampus = \"often\""));
    assert!(master_worldgen_text.contains("walrus = \"rare\""));
    assert!(master_worldgen_text.contains("merm = \"often\""));
    assert!(master_worldgen_text.contains("houndmound = \"rare\""));
    assert!(master_worldgen_text.contains("lureplants = \"often\""));
    assert!(master_worldgen_text.contains("bearger = \"rare\""));
    assert!(master_worldgen_text.contains("beequeen = \"often\""));
    assert!(master_worldgen_text.contains("deerclops = \"rare\""));
    assert!(master_worldgen_text.contains("dragonfly = \"often\""));
    assert!(master_worldgen_text.contains("klaus = \"rare\""));
    assert!(master_worldgen_text.contains("goosemoose = \"rare\""));
    assert!(master_worldgen_text.contains("spiderqueen = \"often\""));
    assert!(master_worldgen_text.contains("liefs = \"rare\""));
    assert!(!master_worldgen_text.contains("toadstool ="));
    for expected in [
        "beefaloheat = \"often\"",
        "deciduoustree_regrowth = \"veryfast\"",
        "carrots_regrowth = \"slow\"",
        "evergreen_regrowth = \"fast\"",
        "flowers_regrowth = \"veryslow\"",
        "moon_tree_regrowth = \"fast\"",
        "saltstack_regrowth = \"slow\"",
        "twiggytrees_regrowth = \"veryfast\"",
        "bees_setting = \"often\"",
        "birds = \"rare\"",
        "bunnymen_setting = \"often\"",
        "catcoons = \"rare\"",
        "gnarwail = \"rare\"",
        "grassgekkos = \"often\"",
        "moles_setting = \"always\"",
        "fishschools = \"often\"",
        "wobsters = \"rare\"",
        "bats_setting = \"never\"",
        "cookiecutters = \"rare\"",
        "mutated_hounds = \"never\"",
        "wasps = \"often\"",
        "merms = \"rare\"",
        "penguins_moon = \"never\"",
        "mosquitos = \"often\"",
        "sharks = \"rare\"",
        "moon_spider = \"often\"",
        "squid = \"rare\"",
        "spider_warriors = \"never\"",
        "antliontribute = \"often\"",
        "crabking = \"rare\"",
        "fruitfly = \"often\"",
        "malbatross = \"rare\"",
        "deciduousmonster = \"often\"",
        "mushroom = \"uncommon\"",
        "moon_fissure = \"rare\"",
        "moon_starfish = \"mostly\"",
        "moon_bullkelp = \"often\"",
        "ocean_bullkelp = \"often\"",
        "cactus = \"rare\"",
        "moon_hotspring = \"often\"",
        "moon_rock = \"mostly\"",
        "moon_sapling = \"rare\"",
        "moon_tree = \"often\"",
        "meteorspawner = \"rare\"",
        "rock_ice = \"uncommon\"",
        "ocean_seastack = \"ocean_rare\"",
        "moon_berrybush = \"mostly\"",
        "moon_carrot = \"often\"",
        "moon_fruitdragon = \"rare\"",
        "ocean_shoal = \"often\"",
        "ocean_wobsterden = \"rare\"",
        "ocean_waterplant = \"ocean_mostly\"",
        "moon_spiders = \"mostly\"",
        "custom_master_override = \"present\"",
    ] {
        assert!(
            master_worldgen_text.contains(expected),
            "missing {expected}"
        );
    }

    let caves_worldgen_text = fs::read_to_string(&caves_worldgen_path).unwrap();
    assert!(caves_worldgen_text.contains("world_size = \"small\""));
    assert!(caves_worldgen_text.contains("branching = \"least\""));
    assert!(caves_worldgen_text.contains("loop = \"always\""));
    assert!(caves_worldgen_text.contains("atriumgate = \"never\""));
    assert!(caves_worldgen_text.contains("wormattacks = \"rare\""));
    assert!(caves_worldgen_text.contains("earthquakes = \"often\""));
    assert!(caves_worldgen_text.contains("regrowth = \"slow\""));
    assert!(caves_worldgen_text.contains("banana = \"often\""));
    assert!(caves_worldgen_text.contains("cave_ponds = \"rare\""));
    assert!(caves_worldgen_text.contains("fern = \"mostly\""));
    assert!(caves_worldgen_text.contains("flint = \"often\""));
    assert!(caves_worldgen_text.contains("lichen = \"always\""));
    assert!(caves_worldgen_text.contains("marshbush = \"uncommon\""));
    assert!(caves_worldgen_text.contains("mushroom = \"mostly\""));
    assert!(caves_worldgen_text.contains("wormlights = \"mostly\""));
    assert!(caves_worldgen_text.contains("flower_cave = \"often\""));
    assert!(caves_worldgen_text.contains("mushtree = \"rare\""));
    assert!(caves_worldgen_text.contains("rock = \"often\""));
    assert!(caves_worldgen_text.contains("sapling = \"rare\""));
    assert!(caves_worldgen_text.contains("bunnymen = \"mostly\""));
    assert!(caves_worldgen_text.contains("rocky = \"rare\""));
    assert!(caves_worldgen_text.contains("slurper = \"often\""));
    assert!(caves_worldgen_text.contains("slurtles = \"mostly\""));
    assert!(caves_worldgen_text.contains("snurtles = \"often\""));
    assert!(caves_worldgen_text.contains("monkey = \"often\""));
    assert!(caves_worldgen_text.contains("bats = \"rare\""));
    assert!(caves_worldgen_text.contains("worms = \"mostly\""));
    assert!(caves_worldgen_text.contains("spiders = \"often\""));
    assert!(caves_worldgen_text.contains("cave_spiders = \"rare\""));
    assert!(caves_worldgen_text.contains("tentacles = \"often\""));
    assert!(caves_worldgen_text.contains("molebats = \"rare\""));
    assert!(caves_worldgen_text.contains("nightmarecreatures = \"often\""));
    assert!(caves_worldgen_text.contains("spider_dropper = \"rare\""));
    assert!(caves_worldgen_text.contains("spider_spitter = \"often\""));
    assert!(caves_worldgen_text.contains("fruitfly = \"rare\""));
    assert!(caves_worldgen_text.contains("fissure = \"often\""));
    for expected in [
        "weather = \"rare\"",
        "flower_cave_regrowth = \"fast\"",
        "lightflier_flower_regrowth = \"slow\"",
        "mushtree_moon_regrowth = \"veryfast\"",
        "mushtree_regrowth = \"slow\"",
        "lightfliers = \"often\"",
        "bunnymen_setting = \"rare\"",
        "dustmoths = \"often\"",
        "grassgekkos = \"never\"",
        "moles_setting = \"rare\"",
        "mushgnome = \"often\"",
        "pigs_setting = \"never\"",
        "rocky_setting = \"rare\"",
        "slurtles_setting = \"often\"",
        "monkey_setting = \"rare\"",
        "bats_setting = \"often\"",
        "spider_hider = \"often\"",
        "merms = \"rare\"",
        "spider_warriors = \"never\"",
        "spiders_setting = \"often\"",
        "spiderqueen = \"rare\"",
        "toadstool = \"often\"",
        "liefs = \"rare\"",
        "grass = \"mostly\"",
        "reeds = \"rare\"",
        "trees = \"often\"",
        "chess = \"rare\"",
        "custom_caves_override = \"present\"",
    ] {
        assert!(caves_worldgen_text.contains(expected), "missing {expected}");
    }
    for inherited in [
        "specialevent = \"none\"",
        "darkness = \"nonlethal\"",
        "healthpenalty = \"none\"",
        "shadowcreatures = \"often\"",
        "day = \"longday\"",
        "beefaloheat = \"often\"",
        "krampus = \"often\"",
    ] {
        assert!(
            caves_worldgen_text.contains(inherited),
            "Caves did not inherit Master option {inherited}"
        );
    }
    for master_only in [
        "season_start =",
        "autumn =",
        "winter =",
        "spring =",
        "summer =",
        "deerclops =",
        "ocean_waterplant =",
        "custom_master_override =",
    ] {
        assert!(
            !caves_worldgen_text.contains(master_only),
            "Caves emitted surface-only key {master_only}"
        );
    }

    assert_file_has_no_utf8_bom(&master_worldgen_path);
    assert_file_has_no_utf8_bom(&caves_worldgen_path);
    assert_file_has_no_utf8_bom(&master_modoverrides_path);
    assert_file_has_no_utf8_bom(&caves_modoverrides_path);

    assert!(!shared_mod_setup_path.exists());
    let shared_mod_setup_text = fs::read_to_string(&instance_mod_setup_path).unwrap();
    assert!(shared_mod_setup_text.contains("ServerModSetup(\"2039181790\")"));
    assert!(shared_mod_setup_text.contains("ServerModSetup(\"1909182187\")"));
    assert!(shared_mod_setup_text.contains("ServerModCollectionSetup(\"3495871201\")"));
    assert_eq!(
        shared_mod_setup_text
            .matches("ServerModSetup(\"2039181790\")")
            .count(),
        1
    );

    assert_eq!(
        fs::read_to_string(&master_modoverrides_path).unwrap(),
        "return {\n  [\"workshop-2039181790\"] = {\n    enabled = true,\n    configuration_options = {\n      [\"language\"] = \"zh\",\n      [\"marker_scale\"] = 1.5,\n      [\"range_ring\"] = true,\n    },\n  },\n  [\"workshop-1909182187\"] = { enabled = true },\n}\n"
    );
    assert_eq!(
        fs::read_to_string(&caves_modoverrides_path).unwrap(),
        "return {\n  [\"workshop-1909182187\"] = {\n    enabled = true,\n    configuration_options = {\n      [\"language\"] = \"zh\",\n      [\"show_creature_age\"] = false,\n    },\n  },\n}\n"
    );

    cleanup_root(&root);
}

#[path = "tests_ark.rs"]
mod ark_tests;
#[path = "tests_dst_runtime_logs.rs"]
mod dst_runtime_log_tests;
#[path = "tests_dst_shared_setup.rs"]
mod dst_shared_setup_tests;
#[path = "tests_game_configs.rs"]
mod game_config_tests;
#[path = "tests_generated_secrets.rs"]
mod generated_secret_tests;
#[path = "tests_native_settings.rs"]
mod native_settings_tests;
#[path = "tests_player_access_integration.rs"]
mod player_access_integration_tests;
#[path = "tests_runtime.rs"]
mod runtime_tests;
#[path = "tests_sevendaystodie_config.rs"]
mod sevendaystodie_config_tests;
#[path = "tests_storage_db.rs"]
mod storage_db_tests;
#[path = "tests_storage_usage.rs"]
mod storage_usage_tests;
