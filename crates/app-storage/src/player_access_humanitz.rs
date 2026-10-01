use std::collections::HashSet;

use serde_json::{Map, Value};

use crate::StorageError;

pub(crate) const HUMANITZ_ROSTER_FIELDS: [&str; 3] = [
    "admin_steam_ids",
    "reserved_player_steam_ids",
    "banned_player_steam_ids",
];

pub(crate) use app_core::is_humanitz_net_id;

pub(crate) fn is_stored_humanitz_identity(value: &str) -> bool {
    is_humanitz_net_id(value)
        || (value.len() == 17 && value.bytes().all(|byte| byte.is_ascii_digit()))
}

pub(crate) fn parse_humanitz_roster(
    settings: &Map<String, Value>,
    key: &str,
) -> Result<Vec<String>, String> {
    let Some(value) = settings.get(key) else {
        return Ok(Vec::new());
    };
    let raw = value
        .as_str()
        .ok_or("HumanitZ roster must be a string list")?;
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for (index, line) in raw.split(['\r', '\n']).enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        if !is_stored_humanitz_identity(line) {
            return Err(format!(
                "roster line {} must contain a complete HumanitZ NetID (EpicAccountId|ProductUserId or |ProductUserId); existing 17-digit Steam IDs may only be retained or removed",
                index + 1
            ));
        }
        // Native EOS ToString copies the complete stored string, including case.
        if seen.insert(line.to_owned()) {
            entries.push(line.to_owned());
        }
    }
    Ok(entries)
}

pub(crate) fn validate_humanitz_rosters(settings: &Map<String, Value>) -> Result<(), StorageError> {
    for key in HUMANITZ_ROSTER_FIELDS {
        parse_humanitz_roster(settings, key).map_err(|message| {
            StorageError::InvalidModuleSetting {
                module_id: "humanitz".to_owned(),
                field: key.to_owned(),
                message,
            }
        })?;
    }
    Ok(())
}

pub(crate) fn validate_humanitz_roster_update(
    persisted: &Map<String, Value>,
    incoming: &Map<String, Value>,
) -> Result<(), StorageError> {
    for key in HUMANITZ_ROSTER_FIELDS {
        let invalid = |message| StorageError::InvalidModuleSetting {
            module_id: "humanitz".to_owned(),
            field: key.to_owned(),
            message,
        };
        let entries = parse_humanitz_roster(incoming, key).map_err(invalid)?;
        if entries.iter().all(|entry| is_humanitz_net_id(entry)) {
            continue;
        }
        let existing = parse_humanitz_roster(persisted, key)
            .map_err(invalid)?
            .into_iter()
            .collect::<HashSet<_>>();
        if entries
            .iter()
            .any(|entry| !is_humanitz_net_id(entry) && !existing.contains(entry))
        {
            return Err(invalid(
                "new roster entries require a complete HumanitZ NetID (EpicAccountId|ProductUserId or |ProductUserId); 17-digit Steam IDs may only be retained in their existing roster or removed".to_owned(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FULL: &str = "0123456789abcdef0123456789ABCDEF|FEDCBA9876543210fedcba9876543210";
    const PRODUCT_ONLY: &str = "|FEDCBA9876543210fedcba9876543210";

    #[test]
    fn native_net_ids_require_complete_parts_and_preserve_case() {
        for value in [FULL, PRODUCT_ONLY] {
            assert!(is_humanitz_net_id(value));
        }
        for value in [
            "76561198000000001",
            "FEDCBA9876543210fedcba9876543210",
            "Host_Offline",
            "|",
            "a|b",
        ] {
            assert!(!is_humanitz_net_id(value), "{value}");
        }
        let settings =
            json!({"admin_steam_ids": format!("76561198000000001\n{FULL}\n{PRODUCT_ONLY}")});
        assert_eq!(
            parse_humanitz_roster(settings.as_object().unwrap(), "admin_steam_ids").unwrap(),
            ["76561198000000001", FULL, PRODUCT_ONLY]
        );
    }

    #[test]
    fn malformed_native_rosters_fail_instead_of_dropping_entries() {
        for invalid in [
            "invalid",
            "Host_Offline",
            "|bad",
            "76561198000000001,76561198000000002",
        ] {
            let settings = json!({"admin_steam_ids": format!("{FULL}\n{invalid}")});
            assert!(validate_humanitz_rosters(settings.as_object().unwrap()).is_err());
        }
    }

    #[test]
    fn humanitz_roster_updates_require_a_same_field_persisted_steam_baseline() {
        let empty = Map::new();
        let old = "76561198000000001";
        for key in HUMANITZ_ROSTER_FIELDS {
            let mut incoming = Map::new();
            incoming.insert(key.to_owned(), json!(old));
            assert!(validate_humanitz_roster_update(&empty, &incoming).is_err());
            assert!(validate_humanitz_roster_update(&incoming, &incoming).is_ok());
            assert!(validate_humanitz_roster_update(&incoming, &empty).is_ok());
            for other in HUMANITZ_ROSTER_FIELDS
                .into_iter()
                .filter(|other| *other != key)
            {
                let mut moved = Map::new();
                moved.insert(other.to_owned(), json!(old));
                assert!(validate_humanitz_roster_update(&incoming, &moved).is_err());
            }
            incoming.insert(key.to_owned(), json!(format!("{FULL}\n{PRODUCT_ONLY}")));
            assert!(validate_humanitz_roster_update(&empty, &incoming).is_ok());
        }
    }

    #[test]
    fn humanitz_roster_updates_do_not_drop_invalid_input_or_require_retaining_it() {
        let malformed = json!({"admin_steam_ids": "Host_Offline"});
        let full = json!({"admin_steam_ids": FULL});
        assert!(
            validate_humanitz_roster_update(&Map::new(), malformed.as_object().unwrap()).is_err()
        );
        assert!(
            validate_humanitz_roster_update(
                malformed.as_object().unwrap(),
                full.as_object().unwrap()
            )
            .is_ok()
        );
        let unproven = json!({"admin_steam_ids": "76561198000000001"});
        assert!(
            validate_humanitz_roster_update(
                malformed.as_object().unwrap(),
                unproven.as_object().unwrap()
            )
            .is_err()
        );
    }
}
