use super::*;

#[test]
fn corekeeper_generated_password_follows_documented_default_length() {
    let descriptors = app_modules::discover_modules(repo_root().join("modules")).unwrap();
    let descriptor = descriptors
        .iter()
        .find(|descriptor| descriptor.summary.id == "corekeeper")
        .unwrap();
    let defaults = collect_schema_defaults_from_schema_json(
        descriptor.schema_json.as_deref(),
        SchemaDefaultContext {
            instance_id: Some("corekeeper-password-regression"),
            instance_name: Some("Core Keeper Password Regression"),
        },
    )
    .unwrap();
    let password = defaults["join_password"].as_str().unwrap();
    assert_eq!(
        password.len(),
        28,
        "new direct-join passwords should follow the documented 28-character limit"
    );
    assert!(password.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

#[test]
fn corekeeper_password_validation_preserves_existing_native_supported_values() {
    let descriptors = app_modules::discover_modules(repo_root().join("modules")).unwrap();
    let descriptor = descriptors
        .iter()
        .find(|descriptor| descriptor.summary.id == "corekeeper")
        .unwrap();
    let mut settings = collect_schema_defaults_from_schema_json(
        descriptor.schema_json.as_deref(),
        SchemaDefaultContext {
            instance_id: Some("corekeeper-password-regression"),
            instance_name: Some("Core Keeper Password Regression"),
        },
    )
    .unwrap();
    for length in [28, 29, 32] {
        let password = "a".repeat(length);
        settings.insert("join_password".into(), Value::String(password.clone()));
        let result = validate_settings_against_schema(
            Some(descriptor),
            &settings,
            SettingsValidationPhase::Complete,
        );
        result.expect(
            "native 1.3.0.4 accepts existing passwords beyond the documented default length",
        );
        assert_eq!(
            settings["join_password"], password,
            "validation must not rotate or truncate existing passwords"
        );
    }
}

const SECRET_FIELDS: &[(&str, &[&str])] = &[
    ("arksurvivalascended", &["admin_password"]),
    ("arksurvivalevolved", &["admin_password"]),
    ("conanexiles", &["admin_password", "rcon_password"]),
    ("corekeeper", &["join_password"]),
    (
        "enshrouded",
        &[
            "admin_password",
            "friend_password",
            "guest_password",
            "visitor_password",
        ],
    ),
    ("humanitz", &["rcon_password"]),
    ("necesse", &["password"]),
    ("palworld", &["admin_password"]),
    ("projectzomboid", &["admin_password", "rcon_password"]),
    ("rust", &["rcon_password"]),
    ("sevendaystodie", &["telnet_password"]),
    ("soulmask", &["admin_password", "rcon_password"]),
    ("squad", &["rcon_password"]),
    ("valheim", &["server_password"]),
    ("vrising", &["rcon_password"]),
];

fn persisted_settings(instance: &InstanceProvisioning) -> Map<String, Value> {
    let document: Value =
        serde_json::from_str(&fs::read_to_string(&instance.config_file_path).unwrap()).unwrap();
    document["settings"].as_object().unwrap().clone()
}

#[tokio::test]
async fn repository_instances_generate_distinct_secrets_and_preserve_operator_values() {
    let root = unique_test_root();
    let mut paths = test_paths(&root);
    paths.modules_root = repo_root().join("modules");
    let descriptors = app_modules::discover_modules(&paths.modules_root).unwrap();
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, &descriptors).await.unwrap();

    for &(module_id, fields) in SECRET_FIELDS {
        let descriptor = descriptors
            .iter()
            .find(|descriptor| descriptor.summary.id == module_id)
            .unwrap();
        replenish_test_library(&paths, descriptor).await;
        let create = |suffix: &str| CreateInstanceInput {
            name: format!("{module_id} {suffix}"),
            module_id: String::from(module_id),
        };
        let first = create_instance(&paths, descriptor, create("First"))
            .await
            .unwrap();
        replenish_test_library(&paths, descriptor).await;
        let second = create_instance(&paths, descriptor, create("Second"))
            .await
            .unwrap();
        let initial = persisted_settings(&first);
        let other = persisted_settings(&second);
        for &field in fields {
            let secret = initial[field].as_str().unwrap();
            let expected_length = if module_id == "corekeeper" { 28 } else { 32 };
            assert!(
                secret.len() == expected_length
                    && secret.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "{module_id}.{field} must have an argument-safe generated secret"
            );
            assert!(
                initial[field] != other[field],
                "{module_id}.{field} must differ between instances"
            );
        }

        materialize_instance_configuration(&paths, &first.summary.id)
            .await
            .unwrap();
        let rematerialized = persisted_settings(&first);
        for &field in fields {
            assert!(
                rematerialized[field] == initial[field],
                "{module_id}.{field} changed during configuration regeneration"
            );
        }

        let mut operator_settings = rematerialized;
        for (index, &field) in fields.iter().enumerate() {
            operator_settings.insert(
                field.into(),
                Value::String(format!("fixture-password-{index}")),
            );
        }
        update_instance(
            &paths,
            UpdateInstanceInput {
                id: first.summary.id.clone(),
                bind_ip: String::from("192.0.2.10"),
                auto_backup_on_stop: false,
                backup_retention_count: 10,
                settings_json: serde_json::to_string(&operator_settings).unwrap(),
                ports: first.ports.clone(),
            },
        )
        .await
        .unwrap();
        materialize_instance_configuration(&paths, &first.summary.id)
            .await
            .unwrap();
        let updated = persisted_settings(&first);
        for (index, &field) in fields.iter().enumerate() {
            assert!(
                updated[field] == format!("fixture-password-{index}"),
                "{module_id}.{field} must retain the operator-provided password"
            );
        }

        let mut settings_without_secrets = updated;
        for &field in fields {
            settings_without_secrets.remove(field);
        }
        update_instance(
            &paths,
            UpdateInstanceInput {
                id: first.summary.id.clone(),
                bind_ip: String::from("192.0.2.10"),
                auto_backup_on_stop: false,
                backup_retention_count: 10,
                settings_json: serde_json::to_string(&settings_without_secrets).unwrap(),
                ports: first.ports.clone(),
            },
        )
        .await
        .unwrap();
        let retained = persisted_settings(&first);
        for (index, &field) in fields.iter().enumerate() {
            assert!(
                retained[field] == format!("fixture-password-{index}"),
                "{module_id}.{field} must not rotate when omitted from a settings update"
            );
        }
    }

    cleanup_root(&root);
}
