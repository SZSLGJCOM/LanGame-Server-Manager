use super::*;
use serde_json::json;

fn master(raw: &str) -> Value {
    json!({"master_modoverrides_lua": raw})
}

#[test]
fn island_adventures_preserves_every_shard_and_its_configuration() {
    let before = json!({"shard_layout":"island_adventures", "volcano_enabled_workshop_mod_ids":"123456", "volcano_mod_configuration_options":{"123456":{"flag":true}}});
    let mut changed = before.clone();
    changed["volcano_mod_configuration_options"]["123456"]["flag"] = json!(false);
    assert!(validate_dst_mod_preservation(&before, &changed).is_err());
    changed = before.clone();
    changed["shard_layout"] = json!("standard");
    assert!(validate_dst_mod_preservation(&before, &changed).is_err());
    validate_dst_mod_preservation(&before, &before).unwrap();
}

#[test]
fn dst_mod_preservation_allows_adding_and_enabling_dependencies() {
    let before = master(
        "return {consumer={enabled=true,configuration_options={mode='normal'}},dependency={enabled=false,configuration_options={level=2}}}",
    );
    let after = master(
        "return {dependency={configuration_options={level=2},enabled=true},consumer={configuration_options={mode='normal'},enabled=true},extra={enabled=true}}",
    );
    validate_dst_mod_preservation(&before, &after).unwrap();
}

#[test]
fn dst_mod_preservation_rejects_removing_or_disabling_enabled_mods() {
    let before = master("return {consumer={enabled=true}}");
    for raw in [
        "return {}",
        "return {consumer={enabled=false}}",
        "return {consumer={enabled=nil}}",
        "return {consumer={}}",
        "return {other={enabled=true}}",
    ] {
        assert!(validate_dst_mod_preservation(&before, &master(raw)).is_err());
    }
}

#[test]
fn dst_mod_preservation_protects_unspecified_enablement_without_assuming_disabled() {
    for raw in ["return {consumer={}}", "return {consumer={enabled=nil}}"] {
        let before = master(raw);
        for changed in ["return {}", "return {consumer={enabled=false}}"] {
            assert!(validate_dst_mod_preservation(&before, &master(changed)).is_err());
        }
        for preserved in [
            "return {consumer={},dependency={enabled=true}}",
            "return {consumer={enabled=nil},dependency={enabled=true}}",
            "return {consumer={enabled=true},dependency={enabled=true}}",
        ] {
            validate_dst_mod_preservation(&before, &master(preserved)).unwrap();
        }
    }
}

#[test]
fn dst_mod_preservation_rejects_option_loss_and_rewrites() {
    let before = master(
        "return {consumer={enabled=true,configuration_options={mode='normal',nested={amount=2}},custom_flag=true}}",
    );
    for raw in [
        "return {consumer={enabled=true}}",
        "return {consumer={enabled=true,configuration_options={mode='normal'},custom_flag=true}}",
        "return {consumer={enabled=true,configuration_options={mode='normal',nested={amount=3}},custom_flag=true}}",
        "return {consumer={enabled=true,configuration_options={mode='normal',nested={amount=2}}}}",
    ] {
        assert!(validate_dst_mod_preservation(&before, &master(raw)).is_err());
    }
}

#[test]
fn dst_mod_preservation_preserves_disabled_options_and_client_policy() {
    let before = master(
        "return {dependency={enabled=false,configuration_options={level=2}},client_mods_disabled=true}",
    );
    for raw in [
        "return {dependency={enabled=true},client_mods_disabled=true}",
        "return {dependency={enabled=true,configuration_options={level=2}},client_mods_disabled=false}",
        "return {dependency={enabled=true,configuration_options={level=2}}}",
    ] {
        assert!(validate_dst_mod_preservation(&before, &master(raw)).is_err());
    }
}

#[test]
fn dst_mod_preservation_compares_static_meaning_not_source_formatting() {
    let before = master(
        "return {['consumer']={enabled=true,configuration_options={a='value',b={nested=false}}}};",
    );
    let after = master(
        "-- updated formatting\nreturn { consumer = { configuration_options = { b={nested=false}, ['a'] = \"value\" }, enabled = true } }",
    );
    validate_dst_mod_preservation(&before, &after).unwrap();
}

#[test]
fn dst_mod_preservation_accepts_equivalent_raw_and_structured_configs() {
    let structured = json!({
        "master_enabled_workshop_mod_ids": "123456",
        "master_mod_configuration_options": {"123456": {"mode": "normal", "level": 2}}
    });
    let raw = master(
        "return {['workshop-123456']={configuration_options={level=2,mode='normal'},enabled=true}}",
    );
    validate_dst_mod_preservation(&structured, &raw).unwrap();
    validate_dst_mod_preservation(&raw, &structured).unwrap();
}

#[test]
fn dst_mod_preservation_uses_actual_raw_precedence_and_fallback() {
    let before = json!({
        "master_modoverrides_lua": "return {consumer={enabled=true}}",
        "master_enabled_workshop_mod_ids": "123456"
    });
    let mut after = before.clone();
    after["master_enabled_workshop_mod_ids"] = json!("654321");
    validate_dst_mod_preservation(&before, &after).unwrap();
    after["master_modoverrides_lua"] = json!("return {\n}\n");
    assert!(validate_dst_mod_preservation(&before, &after).is_err());

    let generated = json!({"master_enabled_workshop_mod_ids": "123456"});
    let default_raw = json!({
        "master_enabled_workshop_mod_ids": "123456",
        "master_modoverrides_lua": "return {\r\n}\r\n"
    });
    validate_dst_mod_preservation(&generated, &default_raw).unwrap();
}

#[test]
fn dst_mod_preservation_rejects_closing_initially_active_caves() {
    let before = json!({"enable_caves": true});
    for after in [
        json!({}),
        json!({"enable_caves": false}),
        json!({"enable_caves": "true"}),
    ] {
        assert!(validate_dst_mod_preservation(&before, &after).is_err());
    }
}

#[test]
fn dst_mod_preservation_checks_each_initially_active_shard() {
    let before = json!({
        "enable_caves": true,
        "master_modoverrides_lua": "return {surface={enabled=true}}",
        "caves_modoverrides_lua": "return {cave={enabled=true,custom_flag=true}}"
    });
    let mut after = before.clone();
    after["caves_modoverrides_lua"] = json!("return {cave={enabled=false,custom_flag=true}}");
    assert!(validate_dst_mod_preservation(&before, &after).is_err());
    let mut inactive = before.clone();
    inactive["enable_caves"] = json!(false);
    after["enable_caves"] = json!(false);
    validate_dst_mod_preservation(&inactive, &after).unwrap();
}

#[test]
fn dst_mod_preservation_unknown_sources_only_allow_identical_rendering() {
    for raw in [
        "local mods = build_mods(); return mods",
        "return {consumer={enabled=true},consumer={enabled=false}}",
        "return {consumer={enabled=1}}",
        "return {consumer={enabled=true,configuration_options={order={[2]='x'}}}}",
    ] {
        let before = master(raw);
        let mut unchanged = before.clone();
        unchanged["cluster_name"] = json!("Renamed server");
        validate_dst_mod_preservation(&before, &unchanged).unwrap();
        assert!(validate_dst_mod_preservation(&before, &master("return {}")).is_err());
    }
}

#[test]
fn dst_mod_preservation_does_not_confuse_numeric_table_keys() {
    let before = master("return {consumer={enabled=true,configuration_options={order={[1]='x'}}}}");
    let after = master("return {consumer={enabled=true,configuration_options={order={[2]='x'}}}}");
    assert!(validate_dst_mod_preservation(&before, &after).is_err());
}

#[test]
fn dst_mod_preservation_bounds_input_and_requires_objects() {
    let excessive = json!({"unused": "x".repeat(128 * 1024)});
    for (before, after) in [
        (json!(null), json!({})),
        (json!({}), json!([])),
        (excessive.clone(), json!({})),
        (json!({}), excessive),
    ] {
        assert!(validate_dst_mod_preservation(&before, &after).is_err());
    }
}

#[test]
fn dst_mod_preservation_unknown_node_or_name_budget_cannot_authorize_changes() {
    let many = (0..101)
        .map(|i| format!("m{i}={{enabled=true}}"))
        .collect::<Vec<_>>()
        .join(",");
    let deep = format!(
        "return {{consumer={{enabled=true,options={}true{}}}}}",
        "{value=".repeat(33),
        "}".repeat(33)
    );
    for raw in [format!("return {{{many}}}"), deep] {
        let before = master(&raw);
        let mut same = before.clone();
        same["cluster_name"] = json!("Ordinary change");
        validate_dst_mod_preservation(&before, &same).unwrap();
        assert!(validate_dst_mod_preservation(&before, &master("return {}")).is_err());
    }
}

#[test]
fn dst_mod_preservation_errors_do_not_include_option_values() {
    let private_value = ["private", "option", "value"].join("-");
    let before = master(&format!(
        "return {{consumer={{enabled=true,configuration_options={{note='{private_value}'}}}}}}"
    ));
    let error = validate_dst_mod_preservation(&before, &master("return {consumer={enabled=true}}"))
        .expect_err("removing an option must fail");
    assert!(!error.contains(&private_value));
    assert!(error.len() < 512);
}
