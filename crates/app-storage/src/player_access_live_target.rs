use serde_json::{Map, Value};

use super::invalid_player_access_input;
use crate::StorageError;

pub(super) fn explicit_object_live_target(
    module_id: &str,
    field_key: &str,
    field_schema: &Map<String, Value>,
    properties: &Map<String, Value>,
    identity_keys: &[&str],
    raw: &Map<String, Value>,
    stored: &Map<String, Value>,
) -> Result<Option<String>, StorageError> {
    let Some(format) = field_schema.get("x-lsgm-player-access-live-target") else {
        return Ok(None);
    };
    let invalid = || {
        invalid_player_access_input(
            module_id,
            field_key,
            "platform_userid live targets require explicit platform and userid identity fields",
        )
    };
    if format.as_str() != Some("platform_userid")
        || !identity_keys.contains(&"platform")
        || !identity_keys.contains(&"userid")
        || properties
            .get("userid")
            .and_then(|property| property.get("x-lsgm-player-access-platform-field"))
            .and_then(Value::as_str)
            != Some("platform")
        || ["platform", "userid"].iter().any(|key| {
            raw.get(*key)
                .and_then(Value::as_str)
                .is_none_or(|value| value.trim().is_empty())
        })
    {
        return Err(invalid());
    }
    let platform = stored
        .get("platform")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    let userid = stored
        .get("userid")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    if !platform
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphabetic)
        || !platform.bytes().all(|byte| byte.is_ascii_alphanumeric())
        || userid.is_empty()
        || userid.len() > 64
        || !userid
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err(invalid());
    }
    Ok(Some(format!("{platform}_{userid}")))
}
