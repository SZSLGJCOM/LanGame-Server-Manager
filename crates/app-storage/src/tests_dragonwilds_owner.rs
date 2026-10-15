use super::*;
use crate::settings_validation::{SettingsValidationPhase, collect_settings_schema_diagnostics};

const MODULE: &str = "runescapedragonwilds";
const NATIVE_INI: &str = "RSDragonwilds/Saved/Config/WindowsServer/DedicatedServer.ini";

#[test]
fn dragonwilds_owner_native_limit_does_not_change_standard_max_length_scalar_semantics() {
    let schema = json!({"properties": {"text": {"type": "string", "maxLength": 2}}});
    for (value, accepted) in [("😀😀", true), ("😀😀😀", false)] {
        let settings = json!({"text": value}).as_object().unwrap().clone();
        let diagnostics = collect_settings_schema_diagnostics(
            &schema,
            &settings,
            SettingsValidationPhase::Complete,
        );
        assert_eq!(diagnostics.is_empty(), accepted);
    }
}

#[test]
fn dragonwilds_owner_schema_rejects_overlong_ascii_ids() {
    let schema: Value = serde_json::from_str(include_str!(
        "../../../modules/runescapedragonwilds/schema.json"
    ))
    .unwrap();
    for (owner, accepted) in [
        (String::new(), true),
        ("a".repeat(32), true),
        ("a".repeat(33), false),
    ] {
        let settings = json!({"owner_id": owner}).as_object().unwrap().clone();
        for phase in [
            SettingsValidationPhase::Creation,
            SettingsValidationPhase::Complete,
        ] {
            let diagnostics = collect_settings_schema_diagnostics(&schema, &settings, phase);
            assert_eq!(
                diagnostics.is_empty(),
                accepted,
                "native ASCII OwnerId length boundary"
            );
            if !accepted {
                assert!(
                    diagnostics
                        .iter()
                        .any(|issue| issue.field == "owner_id" && issue.code == "max_length")
                );
            }
        }
    }
}

#[tokio::test]
async fn dragonwilds_owner_save_accepts_empty_and_native_utf16_boundaries() {
    let fixture = NativeSettingsFixture::new(MODULE).await;
    let native = instance_private_runtime_root(&fixture.created).join(NATIVE_INI);
    for owner in [
        String::new(),
        "a".repeat(32),
        "界".repeat(32),
        "😀".repeat(16),
    ] {
        let before = fixture.details().await;
        let mut settings: Value = serde_json::from_str(&before.settings_json).unwrap();
        settings["owner_id"] = json!(owner);
        let changed = update_instance(&fixture.paths, fixture.input(&before, settings))
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&changed.settings_json).unwrap()["owner_id"],
            owner
        );
        let expected = if owner.is_empty() {
            "LGSM_UNASSIGNED_OWNER"
        } else {
            &owner
        };
        assert!(
            fs::read_to_string(&native)
                .unwrap()
                .lines()
                .any(|line| line.trim_end() == format!("OwnerId={expected}"))
        );
    }
}

async fn database_state(fixture: &NativeSettingsFixture) -> (String, Vec<String>) {
    let pool = crate::storage_db::connect_pool(&fixture.paths)
        .await
        .unwrap();
    let instance = sqlx::query_scalar::<_, String>(
        "SELECT json_array(id,name,module_id,bind_ip,install_id,status,data_path,config_path,logs_path,saves_path,env_json,args_json,autostart,crash_restart_limit,auto_backup_on_stop,backup_retention_count,created_at,updated_at,runtime_mode) FROM instances WHERE id=?1",
    ).bind(&fixture.created.summary.id).fetch_one(&pool).await.unwrap();
    let ports = sqlx::query_scalar::<_, String>(
        "SELECT json_array(id,instance_id,name,port,protocol,created_at) FROM instance_ports WHERE instance_id=?1 ORDER BY id",
    ).bind(&fixture.created.summary.id).fetch_all(&pool).await.unwrap();
    pool.close().await;
    (instance, ports)
}

#[tokio::test]
async fn dragonwilds_owner_overlong_save_preserves_configuration_world_and_database() {
    let fixture = NativeSettingsFixture::new(MODULE).await;
    let runtime = instance_private_runtime_root(&fixture.created);
    let native = runtime.join(NATIVE_INI);
    let save = runtime.join("RSDragonwilds/Saved/SaveGames/Preserved.sav");
    fs::create_dir_all(save.parent().unwrap()).unwrap();
    fs::write(
        &save,
        b"retained world bytes outside the configuration transaction",
    )
    .unwrap();
    let before = fixture.details().await;
    let config_bytes = fs::read(&before.config_file_path).unwrap();
    let native_bytes = fs::read(&native).unwrap();
    let save_bytes = fs::read(&save).unwrap();
    let database = database_state(&fixture).await;
    for owner in ["a".repeat(33), "😀".repeat(17)] {
        let mut settings: Value = serde_json::from_str(&before.settings_json).unwrap();
        settings["owner_id"] = json!(owner);
        let mut input = fixture.input(&before, settings);
        input.bind_ip = "127.0.0.1".into();
        input.auto_backup_on_stop = !before.auto_backup_on_stop;
        input.backup_retention_count = before.backup_retention_count + 1;
        let error = update_instance(&fixture.paths, input).await.expect_err(
            "an online Dragonwilds OwnerId exceeding 32 UTF-16 units must fail before materialization",
        );
        assert!(
            matches!(error, StorageError::InvalidModuleSetting { ref field, .. } if field == "owner_id")
        );
        assert_eq!(fs::read(&before.config_file_path).unwrap(), config_bytes);
        assert_eq!(fs::read(&native).unwrap(), native_bytes);
        assert_eq!(fs::read(&save).unwrap(), save_bytes);
        assert_eq!(database_state(&fixture).await, database);
    }
}
