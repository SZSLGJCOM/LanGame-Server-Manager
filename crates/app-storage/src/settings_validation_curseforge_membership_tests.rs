use super::*;
use crate::settings_validation::{SettingsValidationPhase, validate_settings_against_schema};
use serde_json::json;

fn descriptors() -> Vec<ModuleDescriptor> {
    app_modules::discover_modules(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules"),
    )
    .unwrap()
}

#[test]
fn curseforge_membership_is_scoped_to_asa_even_without_a_module_schema() {
    for mut descriptor in descriptors() {
        descriptor.schema_json = None;
        for field in FIELDS {
            let settings = Map::from_iter([(
                field.into(),
                json!(["1", "1346144", "18446744073709551615"]),
            )]);
            for phase in [
                SettingsValidationPhase::Creation,
                SettingsValidationPhase::Complete,
            ] {
                assert_eq!(
                    validate_settings_against_schema(Some(&descriptor), &settings, phase).is_ok(),
                    descriptor.summary.id == "arksurvivalascended",
                    "{}: {field}",
                    descriptor.summary.id,
                );
            }
        }
        validate(Some(&descriptor), &Map::new()).unwrap();
    }
    for field in FIELDS {
        assert!(validate(None, &Map::from_iter([(field.into(), json!([]))])).is_err());
    }
    validate(None, &Map::new()).unwrap();
}

#[test]
fn curseforge_membership_rejects_malformed_duplicate_and_unbounded_ids() {
    let descriptor = descriptors()
        .into_iter()
        .find(|entry| entry.summary.id == "arksurvivalascended")
        .unwrap();
    for field in FIELDS {
        for value in [
            Value::Null,
            json!({}),
            json!("1346144"),
            json!([1346144]),
            json!([false]),
            json!([""]),
            json!(["0"]),
            json!(["01346144"]),
            json!(["-1"]),
            json!([" 1346144"]),
            json!(["1346144\n"]),
            json!(["../1346144"]),
            json!(["https://www.curseforge.com/projects/1346144"]),
            json!(["18446744073709551616"]),
            json!(["1346144", "1346144"]),
            json!((1..=8193).map(|id| id.to_string()).collect::<Vec<_>>()),
        ] {
            let settings = Map::from_iter([(field.into(), value)]);
            assert!(matches!(validate(Some(&descriptor), &settings),
                Err(StorageError::InvalidModuleSetting { field: actual, .. }) if actual == field));
        }
        validate(
            Some(&descriptor),
            &Map::from_iter([(field.into(), json!([]))]),
        )
        .unwrap();
        validate(
            Some(&descriptor),
            &Map::from_iter([(
                field.into(),
                json!((1..=8192).map(|id| id.to_string()).collect::<Vec<_>>()),
            )]),
        )
        .unwrap();
    }
}

#[test]
fn curseforge_membership_is_not_native_enablement() {
    let descriptor = descriptors()
        .into_iter()
        .find(|entry| entry.summary.id == "arksurvivalascended")
        .unwrap();
    // A separate config editor can re-enable an item. Stale membership markers
    // must not prevent that save; the native loading lists remain authoritative.
    let settings = json!({
        "mod_ids_csv": "1346144", "passive_mod_ids_csv": "1346145",
        "curseforge_disabled_mod_ids": ["1346144"],
        "curseforge_removed_mod_ids": ["1346145"]
    })
    .as_object()
    .unwrap()
    .clone();
    validate(Some(&descriptor), &settings).unwrap();
}
