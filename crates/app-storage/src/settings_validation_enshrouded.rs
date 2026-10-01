use std::collections::HashSet;

use serde_json::{Map, Value};

use crate::StorageError;

const CUSTOM_GROUPS_FIELD: &str = "custom_user_groups_json";
const MAX_CUSTOM_GROUPS_BYTES: usize = 65_536;
const MAX_CUSTOM_GROUPS: usize = 64;
const PRESET_GROUPS: [&str; 4] = ["Admin", "Friend", "Guest", "Visitor"];
const PERMISSION_FIELDS: [&str; 5] = [
    "canKickBan",
    "canAccessInventories",
    "canEditWorld",
    "canEditBase",
    "canExtendBase",
];
const PRESET_PERMISSION_SUFFIXES: [&str; 5] = [
    "can_kick_ban",
    "can_access_inventories",
    "can_edit_world",
    "can_edit_base",
    "can_extend_base",
];

struct RolePermissions {
    field: String,
    public: bool,
    enabled: usize,
}

pub(crate) fn validate_enshrouded_settings(
    settings: &Map<String, Value>,
) -> Result<(), StorageError> {
    let max_players = settings
        .get("max_players")
        .and_then(Value::as_u64)
        .unwrap_or(16);
    let mut roles = Vec::new();
    for name in PRESET_GROUPS {
        let prefix = name.to_ascii_lowercase();
        let field = format!("{prefix}_reserved_slots");
        if let Some(value) = settings.get(&field) {
            validate_reserved_slots(&field, value, max_players)?;
        }
        // Preset defaults are supplied by schema normalization; leave incomplete or
        // mistyped preset fields to schema validation instead of guessing defaults.
        let enabled = PRESET_PERMISSION_SUFFIXES
            .iter()
            .try_fold(0, |count, suffix| {
                settings
                    .get(&format!("{prefix}_{suffix}"))
                    .and_then(Value::as_bool)
                    .map(|enabled| count + usize::from(enabled))
            });
        let field = format!("{prefix}_password");
        if let (Some(password), Some(enabled)) =
            (settings.get(&field).and_then(Value::as_str), enabled)
        {
            roles.push(RolePermissions {
                field,
                public: password.is_empty(),
                enabled,
            });
        }
    }
    roles.extend(validate_custom_groups(settings, max_players)?);
    validate_public_role_permissions(&roles)
}

fn validate_custom_groups(
    settings: &Map<String, Value>,
    max_players: u64,
) -> Result<Vec<RolePermissions>, StorageError> {
    let Some(raw) = settings.get(CUSTOM_GROUPS_FIELD) else {
        return Ok(Vec::new());
    };
    let raw = raw
        .as_str()
        .ok_or_else(|| invalid(CUSTOM_GROUPS_FIELD, "must be a JSON string"))?;
    if raw.len() > MAX_CUSTOM_GROUPS_BYTES {
        return Err(invalid(
            CUSTOM_GROUPS_FIELD,
            "must contain at most 65536 UTF-8 bytes",
        ));
    }
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let parsed: Value = serde_json::from_str(raw).map_err(|error| {
        invalid(
            CUSTOM_GROUPS_FIELD,
            &format!(
                "must contain valid JSON at line {}, column {}",
                error.line(),
                error.column()
            ),
        )
    })?;
    let groups = match &parsed {
        Value::Array(groups) => groups.as_slice(),
        Value::Object(_) => std::slice::from_ref(&parsed),
        _ => {
            return Err(invalid(
                CUSTOM_GROUPS_FIELD,
                "must contain an object or an array of objects",
            ));
        }
    };
    if groups.len() > MAX_CUSTOM_GROUPS {
        return Err(invalid(
            CUSTOM_GROUPS_FIELD,
            "must contain at most 64 custom groups",
        ));
    }
    let mut names = PRESET_GROUPS
        .map(str::to_ascii_lowercase)
        .into_iter()
        .collect::<HashSet<_>>();
    let mut roles = Vec::with_capacity(groups.len());
    for (index, group) in groups.iter().enumerate() {
        let field = format!("{CUSTOM_GROUPS_FIELD}[{index}]");
        let group = group
            .as_object()
            .ok_or_else(|| invalid(&field, "must be an object"))?;
        let name = group
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| invalid(&field, "must define a non-empty name"))?;
        if !names.insert(name.trim().to_ascii_lowercase()) {
            return Err(invalid(
                &field,
                "name must be unique across preset and custom groups",
            ));
        }
        let password = group.get("password").and_then(Value::as_str).ok_or_else(|| {
            invalid(
                &field,
                "must define a string password; an empty string explicitly allows public access",
            )
        })?;
        // Validate documented members and retain unknown members for native extensions.
        for permission in PERMISSION_FIELDS {
            if let Some(value) = group.get(permission)
                && !value.is_boolean()
            {
                return Err(invalid(
                    &format!("{field}.{permission}"),
                    "must be a boolean",
                ));
            }
        }
        if let Some(value) = group.get("reservedSlots") {
            validate_reserved_slots(&format!("{field}.reservedSlots"), value, max_players)?;
        }
        // Native build 23178631 enables only canEditWorld when custom permissions
        // are omitted; see the role-permission fixture for native launch evidence.
        let enabled = PERMISSION_FIELDS
            .iter()
            .filter(|permission| {
                group
                    .get(**permission)
                    .and_then(Value::as_bool)
                    .unwrap_or(**permission == "canEditWorld")
            })
            .count();
        roles.push(RolePermissions {
            field: format!("{field}.password"),
            public: password.is_empty(),
            enabled,
        });
    }
    Ok(roles)
}

fn validate_public_role_permissions(roles: &[RolePermissions]) -> Result<(), StorageError> {
    let Some(protected) = roles
        .iter()
        .filter(|role| !role.public)
        .min_by_key(|role| role.enabled)
    else {
        return Ok(());
    };
    // Native role ordering counts enabled permissions, including canKickBan.
    // Equal-sized disjoint permission sets are accepted by the dedicated server.
    if let Some(public) = roles
        .iter()
        .find(|role| role.public && role.enabled > protected.enabled)
    {
        return Err(invalid(
            &public.field,
            &format!(
                "an empty-password role enables {} permissions, exceeding the {} enabled for the password-protected role at {}; set a password or adjust role permissions",
                public.enabled, protected.enabled, protected.field
            ),
        ));
    }
    Ok(())
}

fn validate_reserved_slots(
    field: &str,
    value: &Value,
    max_players: u64,
) -> Result<(), StorageError> {
    if value
        .as_u64()
        .is_some_and(|slots| slots <= max_players.min(16))
    {
        Ok(())
    } else {
        Err(invalid(
            field,
            "must be a non-negative integer no greater than the player limit",
        ))
    }
}

fn invalid(field: &str, message: &str) -> StorageError {
    StorageError::InvalidModuleSetting {
        module_id: "enshrouded".to_owned(),
        field: field.to_owned(),
        message: message.to_owned(),
    }
}

#[cfg(test)]
#[path = "settings_validation_enshrouded_tests.rs"]
mod tests;
