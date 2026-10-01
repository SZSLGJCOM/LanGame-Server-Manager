use std::collections::HashSet;

use app_modules::ModuleDescriptor;
use regex::Regex;
use serde_json::{Map, Value};

use crate::StorageError;
use crate::player_access_delimited::{
    DelimitedEntryRequirement, is_delimited_player_access_codec,
    normalize_delimited_player_access_entry, split_player_access_text_entries,
};
use crate::settings_value_formats::{
    is_date_or_local_datetime, is_iso_calendar_date, validate_platform_account_id,
};

#[path = "settings_validation_curseforge_membership.rs"]
mod curseforge_membership;
#[path = "settings_validation_program_update.rs"]
mod program_update;
#[path = "settings_validation_runtime_recovery.rs"]
mod runtime_recovery;
#[path = "settings_validation_runtime_resources.rs"]
mod runtime_resources;
pub(crate) use program_update::validate_program_update_policy_change;
#[path = "settings_validation_workshop_collections.rs"]
mod workshop_collections;

#[path = "settings_validation_ark.rs"]
mod ark;

#[cfg(test)]
#[path = "settings_validation_ark_tests.rs"]
mod ark_tests;

#[path = "settings_validation_enshrouded.rs"]
mod enshrouded;
pub(crate) use enshrouded::validate_enshrouded_settings;
#[path = "settings_validation_dontstarve.rs"]
mod dontstarve;
pub(crate) use dontstarve::normalize_dontstarve_operational_settings;

const REQUIRED_BEFORE_START_SCHEMA_KEY: &str = "x-lsgm-required-before-start";
const PLAYER_ACCESS_PLATFORM_FIELD_KEY: &str = "x-lsgm-player-access-platform-field";
const DISALLOWED_LINE_PREFIXES_KEY: &str = "x-lsgm-disallowed-line-prefixes";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsValidationPhase {
    Creation,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SettingsSchemaDiagnostic {
    pub field: String,
    pub code: String,
    pub message: String,
}

pub(crate) fn validate_settings_against_schema(
    descriptor: Option<&ModuleDescriptor>,
    settings: &Map<String, Value>,
    phase: SettingsValidationPhase,
) -> Result<(), StorageError> {
    workshop_collections::validate(descriptor, settings)?;
    curseforge_membership::validate(descriptor, settings)?;
    runtime_resources::validate(
        descriptor.map_or("runtime", |descriptor| descriptor.summary.id.as_str()),
        settings,
    )?;
    runtime_recovery::validate_runtime_recovery_settings(
        descriptor.map_or("runtime", |descriptor| descriptor.summary.id.as_str()),
        settings,
    )?;
    program_update::validate(
        descriptor.map_or("runtime", |descriptor| descriptor.summary.id.as_str()),
        settings,
    )?;
    let Some(descriptor) = descriptor else {
        return Ok(());
    };
    if descriptor.summary.id == "enshrouded" {
        validate_enshrouded_settings(settings)?;
    }
    if descriptor.summary.id == "unturned" {
        crate::templates::validate_unturned_native_settings(settings)?;
    }
    if descriptor.summary.id == "dontstarve" {
        dontstarve::validate_dontstarve_mod_settings(settings)?;
        dontstarve::validate_dontstarve_playstyle(settings)?;
    }
    let Some(schema_json) = descriptor.schema_json.as_deref() else {
        return Ok(());
    };
    let schema: Value = serde_json::from_str(schema_json)?;
    let diagnostics = collect_settings_schema_diagnostics(&schema, settings, phase);
    let caves_inactive = descriptor.summary.id == "dontstarve"
        && settings.get("shard_layout").and_then(Value::as_str) != Some("island_adventures")
        && settings.get("enable_caves").and_then(Value::as_bool) != Some(true);
    let diagnostics = diagnostics.into_iter().filter(|diagnostic| {
        // Retained Caves settings do not affect a Master-only cluster. Validate their
        // native semantics when enabled, while enforcing types and size limits now.
        !(caves_inactive
            && diagnostic.field.starts_with("caves_")
            && matches!(
                diagnostic.code.as_str(),
                "enum"
                    | "pattern"
                    | "min_length"
                    | "minimum"
                    | "maximum"
                    | "multiple_of"
                    | "format"
            ))
    });
    if let Some(diagnostic) = diagnostics.into_iter().next() {
        return Err(StorageError::InvalidModuleSetting {
            module_id: descriptor.summary.id.clone(),
            field: diagnostic.field,
            message: diagnostic.message,
        });
    }
    if matches!(
        descriptor.summary.id.as_str(),
        "arksurvivalascended" | "arksurvivalevolved"
    ) {
        ark::validate_ark_settings(&descriptor.summary.id, settings, &schema)?;
    }
    Ok(())
}

pub(crate) fn collect_settings_schema_diagnostics(
    schema: &Value,
    settings: &Map<String, Value>,
    phase: SettingsValidationPhase,
) -> Vec<SettingsSchemaDiagnostic> {
    let mut diagnostics = Vec::new();
    let Some(schema) = schema.as_object() else {
        diagnostics.push(diagnostic(
            "$schema",
            "schema_type",
            "module schema must be a JSON object",
        ));
        return diagnostics;
    };
    collect_object_diagnostics("", schema, settings, phase, &mut diagnostics);
    diagnostics
}

fn collect_object_diagnostics(
    parent_path: &str,
    schema: &Map<String, Value>,
    settings: &Map<String, Value>,
    phase: SettingsValidationPhase,
    diagnostics: &mut Vec<SettingsSchemaDiagnostic>,
) {
    let properties = schema.get("properties").and_then(Value::as_object);
    let mut reported_missing = HashSet::new();

    if let Some(required_fields) = schema.get("required").and_then(Value::as_array) {
        for field in required_fields.iter().filter_map(Value::as_str) {
            if settings.contains_key(field)
                && settings.get(field).is_some_and(|value| !value.is_null())
            {
                continue;
            }
            let property = properties.and_then(|properties| properties.get(field));
            let deferred = property.is_some_and(schema_property_is_required_before_start);
            if phase == SettingsValidationPhase::Creation && deferred {
                continue;
            }
            let field_path = join_setting_path(parent_path, field);
            diagnostics.push(diagnostic(
                &field_path,
                if deferred {
                    "required_before_start"
                } else {
                    "required"
                },
                if deferred {
                    "must be configured before the instance can start"
                } else {
                    "is required by the module schema"
                },
            ));
            reported_missing.insert(field_path);
        }
    }

    let Some(properties) = properties else {
        return;
    };
    for (field, property) in properties {
        let field_path = join_setting_path(parent_path, field);
        let deferred = schema_property_is_required_before_start(property);
        let value = settings.get(field);
        if deferred && setting_is_unconfigured(value) && phase == SettingsValidationPhase::Creation
        {
            continue;
        }
        let Some(value) = value else {
            if deferred && !reported_missing.contains(&field_path) {
                diagnostics.push(diagnostic(
                    &field_path,
                    "required_before_start",
                    "must be configured before the instance can start",
                ));
            }
            continue;
        };

        let diagnostic_count_before = diagnostics.len();
        collect_value_diagnostics(&field_path, property, value, phase, diagnostics);
        if deferred
            && setting_is_unconfigured(Some(value))
            && diagnostics.len() == diagnostic_count_before
        {
            diagnostics.push(diagnostic(
                &field_path,
                "required_before_start",
                "must be configured before the instance can start",
            ));
        }

        if let Some(platform_field) = property
            .get(PLAYER_ACCESS_PLATFORM_FIELD_KEY)
            .and_then(Value::as_str)
            && let (Some(platform), Some(account_id)) = (
                settings.get(platform_field).and_then(Value::as_str),
                value.as_str(),
            )
            && let Err(requirement) = validate_platform_account_id(platform, account_id)
        {
            diagnostics.push(diagnostic(&field_path, "platform_account_id", &requirement));
        }
    }
}

fn collect_value_diagnostics(
    field_path: &str,
    schema: &Value,
    value: &Value,
    phase: SettingsValidationPhase,
    diagnostics: &mut Vec<SettingsSchemaDiagnostic>,
) {
    let Some(schema) = schema.as_object() else {
        diagnostics.push(diagnostic(
            field_path,
            "property_schema",
            "property schema must be a JSON object",
        ));
        return;
    };

    if let Some(expected_type) = schema.get("type")
        && !matches_schema_type(expected_type, value)
    {
        diagnostics.push(diagnostic(
            field_path,
            "type",
            &format!("must be {}", render_expected_type(expected_type)),
        ));
    }

    if let Some(allowed_values) = schema.get("enum").and_then(Value::as_array)
        && !allowed_values
            .iter()
            .any(|allowed| schema_enum_value_matches(allowed, value))
    {
        let rendered = allowed_values
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        diagnostics.push(diagnostic(
            field_path,
            "enum",
            &format!("must be one of [{rendered}]"),
        ));
    }

    if let Some(text) = value.as_str() {
        collect_string_diagnostics(field_path, schema, text, diagnostics);
    }
    if let Some(number) = value.as_f64() {
        collect_numeric_diagnostics(field_path, schema, number, diagnostics);
    }
    if let Some(items) = value.as_array()
        && let Some(item_schema) = schema.get("items")
    {
        for (index, item) in items.iter().enumerate() {
            collect_value_diagnostics(
                &format!("{field_path}[{index}]"),
                item_schema,
                item,
                phase,
                diagnostics,
            );
        }
    }
    if let Some(object) = value.as_object() {
        collect_object_diagnostics(field_path, schema, object, phase, diagnostics);
    }
}

fn collect_string_diagnostics(
    field_path: &str,
    schema: &Map<String, Value>,
    text: &str,
    diagnostics: &mut Vec<SettingsSchemaDiagnostic>,
) {
    let length = text.chars().count() as u64;
    if let Some(min_length) = schema.get("minLength").and_then(Value::as_u64)
        && length < min_length
    {
        diagnostics.push(diagnostic(
            field_path,
            "min_length",
            &format!("must be at least {min_length} characters"),
        ));
    }
    if let Some(max_length) = schema.get("maxLength").and_then(Value::as_u64)
        && length > max_length
    {
        diagnostics.push(diagnostic(
            field_path,
            "max_length",
            &format!("must be at most {max_length} characters"),
        ));
    }
    if let Some(pattern) = schema.get("pattern").and_then(Value::as_str) {
        match Regex::new(pattern) {
            Ok(regex) if !regex.is_match(text) => diagnostics.push(diagnostic(
                field_path,
                "pattern",
                &format!("must match schema pattern {pattern}"),
            )),
            Err(error) => diagnostics.push(diagnostic(
                field_path,
                "schema_pattern",
                &format!("schema pattern is invalid: {error}"),
            )),
            _ => {}
        }
    }
    if schema.get("format").and_then(Value::as_str) == Some("date-or-local-datetime")
        && !is_date_or_local_datetime(text)
    {
        diagnostics.push(diagnostic(
            field_path,
            "format",
            "must be a valid calendar date in YYYY-MM-DD or YYYY-MM-DD HH:mm:ss format",
        ));
    }
    if schema.get("format").and_then(Value::as_str) == Some("date") && !is_iso_calendar_date(text) {
        diagnostics.push(diagnostic(
            field_path,
            "format",
            "must be a valid calendar date in YYYY-MM-DD format",
        ));
    }
    if let Some(prefixes) = schema
        .get(DISALLOWED_LINE_PREFIXES_KEY)
        .and_then(Value::as_array)
    {
        let prefixes = prefixes
            .iter()
            .filter_map(Value::as_str)
            .map(|prefix| prefix.trim().to_ascii_lowercase())
            .filter(|prefix| !prefix.is_empty())
            .collect::<Vec<_>>();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim_start();
            if line.is_empty()
                || line.starts_with(';')
                || line.starts_with('#')
                || line.starts_with("//")
            {
                continue;
            }
            let lower = line.to_ascii_lowercase();
            if let Some(prefix) = prefixes
                .iter()
                .find(|prefix| lower.starts_with(prefix.as_str()))
            {
                diagnostics.push(diagnostic(
                    field_path,
                    "managed_directive",
                    &format!(
                        "line {} starts with managed directive `{prefix}`; edit it in the corresponding managed setting instead",
                        index + 1
                    ),
                ));
            }
        }
    }

    if let Some(codec) = schema
        .get("x-lsgm-player-access-codec")
        .and_then(Value::as_str)
    {
        if is_delimited_player_access_codec(codec) {
            for (index, entry) in text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .enumerate()
            {
                if entry.starts_with('#') || entry.starts_with("//") {
                    continue;
                }
                if let Err(message) = normalize_delimited_player_access_entry(
                    codec,
                    schema,
                    entry,
                    DelimitedEntryRequirement::Stored,
                ) {
                    diagnostics.push(diagnostic(
                        field_path,
                        "player_access_entry",
                        &format!("roster entry {} is invalid: {message}", index + 1),
                    ));
                }
            }
        } else if codec == "humanitz_net_id" {
            for (index, entry) in split_player_access_text_entries(codec, schema, text)
                .into_iter()
                .enumerate()
            {
                if !crate::player_access_normalization::is_stored_humanitz_identity(&entry) {
                    diagnostics.push(diagnostic(field_path, "player_access_entry",
                        &format!("roster entry {} must contain a complete HumanitZ NetID; old 17-digit Steam IDs may only be retained or removed", index + 1)));
                }
            }
        } else if matches!(codec, "steam64" | "uint64") {
            let entries = if schema.get("format").and_then(Value::as_str) == Some("textarea") {
                split_player_access_text_entries(codec, schema, text)
            } else if text.trim().is_empty() {
                Vec::new()
            } else {
                vec![text.trim().to_string()]
            };
            for (index, entry) in entries.into_iter().enumerate() {
                let invalid = if codec == "uint64" {
                    crate::player_access_normalization::normalize_uint64_id(&entry).is_none()
                } else {
                    entry.len() != 17 || !entry.bytes().all(|byte| byte.is_ascii_digit())
                };
                if invalid {
                    let requirement = if codec == "uint64" {
                        "contain only decimal digits in 0..18446744073709551615"
                    } else {
                        "be a 17-digit Steam64 ID"
                    };
                    diagnostics.push(diagnostic(
                        field_path,
                        "player_access_entry",
                        &format!("roster entry {} must {requirement}", index + 1),
                    ));
                }
            }
        }
    }
}

fn collect_numeric_diagnostics(
    field_path: &str,
    schema: &Map<String, Value>,
    number: f64,
    diagnostics: &mut Vec<SettingsSchemaDiagnostic>,
) {
    if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64)
        && number < minimum
    {
        diagnostics.push(diagnostic(
            field_path,
            "minimum",
            &format!("must be at least {minimum}"),
        ));
    }
    if let Some(maximum) = schema.get("maximum").and_then(Value::as_f64)
        && number > maximum
    {
        diagnostics.push(diagnostic(
            field_path,
            "maximum",
            &format!("must be at most {maximum}"),
        ));
    }
    if let Some(multiple) = schema.get("multipleOf").and_then(Value::as_f64) {
        if multiple <= 0.0 {
            diagnostics.push(diagnostic(
                field_path,
                "schema_multiple_of",
                "schema multipleOf must be greater than zero",
            ));
        } else {
            let quotient = number / multiple;
            let tolerance = quotient.abs().max(1.0) * f64::EPSILON * 8.0;
            if (quotient - quotient.round()).abs() > tolerance {
                diagnostics.push(diagnostic(
                    field_path,
                    "multiple_of",
                    &format!("must be a multiple of {multiple}"),
                ));
            }
        }
    }
}

fn matches_schema_type(expected_type: &Value, value: &Value) -> bool {
    match expected_type {
        Value::String(expected) => setting_matches_schema_type(value, expected),
        Value::Array(expected) => expected
            .iter()
            .filter_map(Value::as_str)
            .any(|expected| setting_matches_schema_type(value, expected)),
        _ => false,
    }
}

fn render_expected_type(expected_type: &Value) -> String {
    match expected_type {
        Value::String(expected) => expected.clone(),
        Value::Array(expected) => expected
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" or "),
        _ => String::from("a supported schema type"),
    }
}

fn setting_matches_schema_type(value: &Value, expected: &str) -> bool {
    match expected {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_number().is_some_and(json_number_is_integer),
        "array" => value.is_array(),
        "object" => value.is_object(),
        _ => false,
    }
}

fn json_number_is_integer(number: &serde_json::Number) -> bool {
    number.as_i64().is_some()
        || number.as_u64().is_some()
        || number.as_f64().is_some_and(|value| value.fract() == 0.0)
}

fn schema_enum_value_matches(allowed: &Value, actual: &Value) -> bool {
    allowed == actual
        || allowed
            .as_f64()
            .zip(actual.as_f64())
            .is_some_and(|(allowed, actual)| allowed == actual)
}

fn schema_property_is_required_before_start(property: &Value) -> bool {
    property
        .get(REQUIRED_BEFORE_START_SCHEMA_KEY)
        .and_then(Value::as_bool)
        == Some(true)
}

fn setting_is_unconfigured(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::String(value)) => value.trim().is_empty(),
        Some(Value::Array(value)) => value.is_empty(),
        Some(Value::Object(value)) => value.is_empty(),
        Some(_) => false,
    }
}

fn join_setting_path(parent: &str, field: &str) -> String {
    if parent.is_empty() {
        String::from(field)
    } else {
        format!("{parent}.{field}")
    }
}

fn diagnostic(field: &str, code: &str, message: &str) -> SettingsSchemaDiagnostic {
    SettingsSchemaDiagnostic {
        field: String::from(field),
        code: String::from(code),
        message: String::from(message),
    }
}
