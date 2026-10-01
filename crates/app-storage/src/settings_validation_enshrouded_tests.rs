use super::*;
use serde_json::json;

fn settings(raw: Value) -> Map<String, Value> {
    let schema: Value =
        serde_json::from_str(include_str!("../../../modules/enshrouded/schema.json")).unwrap();
    let mut settings = schema["properties"]
        .as_object()
        .unwrap()
        .iter()
        .filter_map(|(field, property)| {
            property
                .get("default")
                .cloned()
                .or_else(|| {
                    (property["x-lsgm-default-source"] == "generated_secret")
                        .then(|| json!("fixture-password"))
                })
                .map(|value| (field.clone(), value))
        })
        .collect::<Map<_, _>>();
    settings.insert(CUSTOM_GROUPS_FIELD.to_owned(), raw);
    settings
}

#[test]
fn custom_role_errors_reach_the_template_boundary() {
    let input = settings(json!("[42]"));
    let root = std::path::Path::new(".");
    let render_input = crate::templates::ModuleTemplateRenderInput {
        config_dir: root,
        install_root: root,
        saves_dir: root,
        instance_id: "fixture-instance",
        instance_name: "Fixture",
        module_id: "enshrouded",
        bind_ip: "0.0.0.0",
        autostart: false,
        settings: &input,
        ports: &[],
    };
    assert!(crate::templates::render_module_templates(root, &render_input).is_err());
}

#[test]
fn invalid_custom_roles_are_rejected_instead_of_silently_dropped() {
    for raw in [
        json!("{"),
        json!("true"),
        json!("[42]"),
        json!(r#"[{"name":"Helper","password":""},null]"#),
        json!(r#"{"name":" ","password":""}"#),
        json!(r#"{"name":"Helper"}"#),
        json!(r#"{"name":"Helper","password":false}"#),
        json!(r#"{"name":"Helper","password":"","canKickBan":"false"}"#),
        json!(r#"{"name":"Helper","password":"","reservedSlots":1.5}"#),
        json!(r#"{"name":"Helper","password":"","reservedSlots":-1}"#),
        json!(r#"{"name":"Helper","password":"","reservedSlots":17}"#),
        json!(r#"{"name":"Admin","password":""}"#),
        json!(r#"[{"name":"Helper","password":""},{"name":"helper","password":""}]"#),
        json!(42),
        json!(" ".repeat(MAX_CUSTOM_GROUPS_BYTES + 1)),
    ] {
        assert!(validate_enshrouded_settings(&settings(raw)).is_err());
    }
}

#[test]
fn custom_roles_keep_explicit_public_access_and_unknown_native_members() {
    for raw in [
        "",
        "[]",
        r#"{"name":"Helper","password":"","canEditWorld":false,"reservedSlots":0,"futureNativeOption":{"value":true}}"#,
        r#"[{"name":"Builder","password":"fixture-admin-password","canEditBase":true}]"#,
    ] {
        let input = settings(json!(raw));
        let original = input.clone();
        validate_enshrouded_settings(&input).unwrap();
        assert_eq!(input, original);
    }
}

#[test]
fn reserved_slots_cannot_exceed_instance_capacity() {
    for (key, value) in [
        ("friend_reserved_slots", json!(5)),
        (
            CUSTOM_GROUPS_FIELD,
            json!(r#"{"name":"Helper","password":"","reservedSlots":5}"#),
        ),
    ] {
        let input = Map::from_iter([
            ("max_players".to_owned(), json!(4)),
            (key.to_owned(), value),
        ]);
        assert!(validate_enshrouded_settings(&input).is_err());
    }
}

#[test]
fn custom_role_limits_bound_parsing_without_disclosing_passwords() {
    let groups = (0..=MAX_CUSTOM_GROUPS)
        .map(|index| json!({"name": format!("Group{index}"), "password": "fixture-password"}))
        .collect::<Vec<_>>();
    assert!(validate_enshrouded_settings(&settings(json!(json!(groups).to_string()))).is_err());

    let input = settings(json!(r#"{"name":"Helper","password":"fixture-password",}"#));
    let message = validate_enshrouded_settings(&input)
        .unwrap_err()
        .to_string();
    assert!(!message.contains("fixture-password"));
    assert!(message.contains("line 1"));
}

#[test]
fn role_permission_order_matches_native_build_fixture() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../modules/enshrouded/reference-fixtures/2026-09-08-native_role_permissions.json"
    ))
    .unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let groups = case["roles"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, role)| {
                let mut group = role["input_permissions"].as_object().unwrap().clone();
                group.insert("name".to_owned(), json!(format!("Group {index}")));
                group.insert(
                    "password".to_owned(),
                    if role["password_present"] == true {
                        json!("fixture-password")
                    } else {
                        json!("")
                    },
                );
                Value::Object(group)
            })
            .collect::<Vec<_>>();
        let mut input = settings(json!(json!(groups).to_string()));
        for prefix in PRESET_GROUPS.map(str::to_ascii_lowercase) {
            for suffix in PRESET_PERMISSION_SUFFIXES {
                input.insert(format!("{prefix}_{suffix}"), json!(true));
            }
        }
        assert_eq!(
            validate_enshrouded_settings(&input).is_err(),
            case["native_permission_error"].as_bool().unwrap(),
            "native role case {}",
            case["name"]
        );
    }
}

#[test]
fn preset_public_permissions_are_checked_without_custom_groups() {
    for raw in [None, Some(""), Some("[]")] {
        let mut input = settings(json!(raw.unwrap_or_default()));
        if raw.is_none() {
            input.remove(CUSTOM_GROUPS_FIELD);
        }
        input.insert("friend_password".to_owned(), json!(""));
        let error = validate_enshrouded_settings(&input).unwrap_err();
        assert!(matches!(
            error,
            StorageError::InvalidModuleSetting { field, .. } if field == "friend_password"
        ));
    }
}

#[test]
fn public_role_can_match_the_least_privileged_protected_role() {
    let mut input = settings(json!(""));
    input.insert("visitor_password".to_owned(), json!(""));
    validate_enshrouded_settings(&input).unwrap();
    input.insert("guest_can_edit_world".to_owned(), json!(false));
    validate_enshrouded_settings(&input).unwrap();
}

#[test]
fn all_public_roles_can_have_different_permissions() {
    let mut input = settings(json!(r#"{"name":"Helper","password":""}"#));
    for prefix in PRESET_GROUPS.map(str::to_ascii_lowercase) {
        input.insert(format!("{prefix}_password"), json!(""));
    }
    validate_enshrouded_settings(&input).unwrap();
}

#[test]
fn public_role_errors_do_not_disclose_role_names_or_passwords() {
    let input = settings(json!(
        r#"[{"name":"Private fixture name","password":"fixture-password","canEditWorld":false},{"name":"Public fixture name","password":""}]"#
    ));
    let error = validate_enshrouded_settings(&input).unwrap_err();
    let message = error.to_string();
    assert!(!message.contains("fixture-password"));
    assert!(!message.contains("Private fixture name"));
    assert!(!message.contains("Public fixture name"));
    assert!(matches!(
        error,
        StorageError::InvalidModuleSetting { field, .. }
            if field == "custom_user_groups_json[1].password"
    ));
}
