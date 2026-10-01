use quick_xml::events::{BytesStart, Event};
use serde_json::{Map, Value};

use super::{MAX_DOCUMENT_BYTES, invalid};
use crate::StorageError;
use crate::settings_value_formats::validate_platform_account_id;

pub(super) struct ConfirmedBan {
    pub entry: Value,
    pub target_is_admin: bool,
}

pub(super) fn account_identity(target: &str) -> Result<(&str, &str), StorageError> {
    let (platform, userid) = target
        .split_once('_')
        .ok_or_else(|| invalid("The ban target must be a canonical Steam_ or EOS_ account."))?;
    if !matches!(platform, "Steam" | "EOS") {
        return Err(invalid(
            "The ban target must use a supported canonical platform.",
        ));
    }
    validate_platform_account_id(platform, userid).map_err(|message| invalid(&message))?;
    Ok((platform, userid))
}

pub(super) fn matches_account(entry: &Value, target: &str) -> bool {
    let Ok((platform, userid)) = account_identity(target) else {
        return false;
    };
    entry
        .get("platform")
        .and_then(Value::as_str)
        .is_some_and(|value| value.trim().eq_ignore_ascii_case(platform))
        && entry
            .get("userid")
            .and_then(Value::as_str)
            .is_some_and(|value| value.trim().eq_ignore_ascii_case(userid))
}

/// Parse the entire bounded document before trusting either presence or absence.
/// No external entities, DTDs, instructions, nested roster entries or text bodies
/// are accepted. Only one target is returned; other native accounts stay native.
pub(super) fn read_confirmed_ban(
    document: &str,
    target: &str,
) -> Result<ConfirmedBan, StorageError> {
    account_identity(target)?;
    if document.len() > MAX_DOCUMENT_BYTES || document.lines().count() > 8_192 {
        return Err(invalid(
            "The native roster exceeds its bounded read budget.",
        ));
    }
    if document
        .chars()
        .any(|value| value.is_control() && !matches!(value, '\t' | '\r' | '\n'))
    {
        return Err(invalid("The native roster contains control characters."));
    }
    let mut reader = quick_xml::Reader::from_str(document.trim_start_matches('\u{feff}'));
    let mut stack: Vec<String> = Vec::new();
    let mut root_seen = false;
    let mut users_seen = false;
    let mut blacklist_seen = false;
    let mut target_is_admin = false;
    let mut entry = None;
    let mut events = 0;
    loop {
        let event = reader
            .read_event()
            .map_err(|error| invalid(&format!("The native roster XML is invalid: {error}")))?;
        events += 1;
        if events > 24_576 {
            return Err(invalid("The native roster has too many XML events."));
        }
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(node) | Event::Empty(node) => {
                let name = node.name().as_ref().to_string();
                let attributes = attributes(&node)?;
                if stack.is_empty() {
                    if root_seen || name != "adminTools" {
                        return Err(invalid(
                            "The native roster must have exactly one adminTools root.",
                        ));
                    }
                    root_seen = true;
                } else if stack.len() == 1 {
                    match name.as_str() {
                        "users" if !users_seen => users_seen = true,
                        "blacklist" if !blacklist_seen => blacklist_seen = true,
                        "users" | "blacklist" => {
                            return Err(invalid("Duplicate native roster sections are ambiguous."));
                        }
                        _ => {}
                    }
                } else if stack.len() == 2 && stack[1] == "blacklist" && name == "blacklisted" {
                    let row = Value::Object(attributes);
                    if matches_account(&row, target) {
                        if entry.is_some() {
                            return Err(invalid(
                                "Duplicate native bans for the target account are ambiguous.",
                            ));
                        }
                        let mut confirmed = Map::new();
                        for key in ["platform", "userid", "unbandate", "reason"] {
                            let value = row.get(key).and_then(Value::as_str)
                                .ok_or_else(|| invalid("The native ban is missing an identity, expiry or reason attribute."))?;
                            confirmed.insert(key.to_string(), Value::String(value.to_string()));
                        }
                        confirmed.insert(
                            "name".to_string(),
                            row.get("name")
                                .cloned()
                                .unwrap_or_else(|| Value::String(String::new())),
                        );
                        entry = Some(Value::Object(confirmed));
                    }
                } else if stack.len() == 2 && stack[1] == "users" && name == "user" {
                    target_is_admin |= matches_account(&Value::Object(attributes), target);
                } else if stack.len() > 2 && matches!(stack[1].as_str(), "users" | "blacklist") {
                    return Err(invalid(
                        "Native roster entries cannot contain nested elements.",
                    ));
                }
                if !empty {
                    if stack.len() >= 16 {
                        return Err(invalid("The native XML nesting exceeds the limit."));
                    }
                    stack.push(name);
                }
            }
            Event::End(node) => {
                if stack.last().map(String::as_str) != Some(node.name().as_ref()) {
                    return Err(invalid(
                        "The native roster has mismatched closing elements.",
                    ));
                }
                stack.pop();
            }
            Event::Text(text) if text.as_ref().chars().all(char::is_whitespace) => {}
            Event::Comment(_) => {}
            Event::Decl(_) if !root_seen => {}
            Event::Eof => break,
            _ => {
                return Err(invalid(
                    "The native roster must not contain DTDs, entities, instructions or text content.",
                ));
            }
        }
    }
    if !stack.is_empty() || !root_seen || !users_seen || !blacklist_seen {
        return Err(invalid(
            "The native roster must contain complete users and blacklist sections.",
        ));
    }
    Ok(ConfirmedBan {
        entry: entry.ok_or_else(|| invalid("The target ban was not found in the native serveradmin.xml; stored settings were not changed."))?,
        target_is_admin,
    })
}

fn attributes(node: &BytesStart<'_>) -> Result<Map<String, Value>, StorageError> {
    let mut values = Map::new();
    for attribute in node.attributes() {
        let attribute = attribute
            .map_err(|error| invalid(&format!("Invalid native roster attribute: {error}")))?;
        if attribute.value.chars().any(char::is_control) {
            return Err(invalid(
                "Native roster attributes must not contain control characters.",
            ));
        }
        let value = attribute
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|error| invalid(&format!("Invalid native roster attribute value: {error}")))?;
        if value.chars().any(char::is_control) {
            return Err(invalid(
                "Native roster attributes must not contain control characters.",
            ));
        }
        values.insert(
            attribute.key.as_ref().to_string(),
            Value::String(value.into_owned()),
        );
    }
    Ok(values)
}
