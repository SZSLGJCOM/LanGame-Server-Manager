use super::*;
use crate::settings_validation::{SettingsValidationPhase, validate_settings_against_schema};
use serde_json::json;

fn descriptor(module_id: &str) -> ModuleDescriptor {
    app_modules::discover_modules(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules"),
    )
    .unwrap()
    .into_iter()
    .find(|descriptor| descriptor.summary.id == module_id)
    .unwrap()
}

fn record(id: &str) -> Value {
    json!({"id": id, "title": "Saved collection", "member_ids": ["2039181790"]})
}

fn check(descriptor: Option<&ModuleDescriptor>, value: Value) -> Result<(), StorageError> {
    validate(descriptor, &Map::from_iter([(FIELD.into(), value)]))
}

#[test]
fn workshop_collections_are_manager_metadata_for_only_steam_modules() {
    let modules = app_modules::discover_modules(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules"),
    )
    .unwrap();
    let mut steam_count = 0;
    for descriptor in modules {
        // No schema declaration is needed; manager policy is validated separately.
        let steam = descriptor
            .workshop
            .as_ref()
            .is_some_and(|workshop| workshop.provider == "steam");
        assert_eq!(
            check(Some(&descriptor), json!([record("3495871201")])).is_ok(),
            steam,
            "{}",
            descriptor.summary.id
        );
        if steam {
            steam_count += 1;
        }
        validate(Some(&descriptor), &Map::new()).unwrap();
    }
    assert!(steam_count >= 5);
    validate(None, &Map::new()).unwrap();
    assert!(check(None, json!([])).is_err());
}

#[test]
fn workshop_collections_reject_malformed_records_before_schema_checks() {
    let mut descriptor = descriptor("dontstarve");
    descriptor.schema_json = None;
    let records = json!([record("3495871201")]);
    let mut invalid = vec![Value::Null, json!({}), json!([false]), json!([{}])];
    for (pointer, value) in [
        ("/0/id", json!("000001")),
        ("/0/id", json!("12345")),
        ("/0/id", json!("18446744073709551616")),
        ("/0/id", json!(123456)),
        ("/0/title", Value::Null),
        ("/0/title", json!("集".repeat(513))),
        ("/0/member_ids", json!(["2039181790", "2039181790"])),
        (
            "/0/member_ids",
            json!(["https://steamcommunity.com/sharedfiles/filedetails/?id=123456"]),
        ),
        ("/0/member_ids", json!(false)),
    ] {
        let mut value_records = records.clone();
        *value_records.pointer_mut(pointer).unwrap() = value;
        invalid.push(value_records);
    }
    let mut unknown = record("3495871201");
    unknown["payload"] = json!({"arbitrary":"metadata"});
    invalid.push(json!([unknown]));
    invalid.push(json!([record("3495871201"), record("3495871201")]));
    for value in invalid {
        let settings = Map::from_iter([(FIELD.into(), value)]);
        for phase in [
            SettingsValidationPhase::Creation,
            SettingsValidationPhase::Complete,
        ] {
            assert!(matches!(
                validate_settings_against_schema(Some(&descriptor), &settings, phase),
                Err(StorageError::InvalidModuleSetting { field, .. }) if field.starts_with(FIELD)
            ));
        }
    }
    check(
        Some(&descriptor),
        json!([{"id":"18446744073709551615", "title":"集".repeat(512), "member_ids":[]}]),
    )
    .unwrap();
}

#[test]
fn workshop_collections_enforce_record_member_and_aggregate_limits() {
    let descriptor = descriptor("dontstarve");
    let records = (0..128)
        .map(|index| record(&(1_000_000 + index).to_string()))
        .collect::<Vec<_>>();
    check(Some(&descriptor), json!(records)).unwrap();
    let mut overflow = records.clone();
    overflow.push(record("9000000"));
    assert!(check(Some(&descriptor), json!(overflow)).is_err());
    let mut records = (0..8)
        .map(|index| record(&(1_000_000 + index).to_string()))
        .collect::<Vec<_>>();
    for record in &mut records {
        record["member_ids"] = json!(
            (0..8192)
                .map(|index| (2_000_000 + index).to_string())
                .collect::<Vec<_>>()
        );
    }
    check(Some(&descriptor), json!(records)).unwrap();
    let mut aggregate_overflow = records.clone();
    aggregate_overflow.push(record("9000000"));
    assert!(check(Some(&descriptor), json!(aggregate_overflow)).is_err());
    records[0]["member_ids"]
        .as_array_mut()
        .unwrap()
        .push(json!("9999999"));
    assert!(check(Some(&descriptor), json!([records[0]])).is_err());
}

#[test]
fn workshop_collections_removed_dst_options_do_not_reintroduce_membership() {
    let dst = descriptor("dontstarve");
    let squad = descriptor("squad");
    let valid = Map::from_iter([("dst_removed_workshop_mod_ids".into(), json!(["123456"]))]);
    validate(Some(&dst), &valid).unwrap();
    assert!(validate(Some(&squad), &valid).is_err());
    assert!(validate(None, &valid).is_err());
    for value in [
        json!("123456"),
        json!(["123456", "123456"]),
        json!(["../123456"]),
        json!(["18446744073709551616"]),
        json!(
            (0..8193)
                .map(|index| (1_000_000 + index).to_string())
                .collect::<Vec<_>>()
        ),
    ] {
        assert!(
            validate(
                Some(&dst),
                &Map::from_iter([("dst_removed_workshop_mod_ids".into(), value)])
            )
            .is_err()
        );
    }
}

#[test]
fn workshop_collections_control_markers_are_scoped_bounded_and_canonical() {
    for (field, allowed) in [
        (
            "steam_workshop_disabled_mod_ids",
            &["barotrauma", "conanexiles", "soulmask"][..],
        ),
        ("steam_workshop_removed_mod_ids", &["palworld"][..]),
    ] {
        for module in [
            "barotrauma",
            "conanexiles",
            "soulmask",
            "palworld",
            "dontstarve",
            "squad",
            "arksurvivalevolved",
        ] {
            let descriptor = descriptor(module);
            let settings =
                Map::from_iter([(field.into(), json!(["123456", "18446744073709551615"]))]);
            assert_eq!(
                validate(Some(&descriptor), &settings).is_ok(),
                allowed.contains(&module),
                "{field}: {module}"
            );
        }
        let descriptor = descriptor(allowed[0]);
        let valid = json!(
            (0..8192)
                .map(|index| (1_000_000 + index).to_string())
                .collect::<Vec<_>>()
        );
        validate(Some(&descriptor), &Map::from_iter([(field.into(), valid)])).unwrap();
        for value in [
            json!(null),
            json!("123456"),
            json!([123456]),
            json!(["0123456"]),
            json!(["12345"]),
            json!(["123456", "123456"]),
            json!(["../123456"]),
            json!(["18446744073709551616"]),
            json!(
                (0..8193)
                    .map(|index| (1_000_000 + index).to_string())
                    .collect::<Vec<_>>()
            ),
        ] {
            assert!(
                validate(Some(&descriptor), &Map::from_iter([(field.into(), value)])).is_err(),
                "{field}"
            );
        }
        assert!(validate(None, &Map::from_iter([(field.into(), json!([]))])).is_err());
    }
}
