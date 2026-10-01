use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Pinned instances keep their current program bytes until this policy changes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceProgramUpdatePolicy {
    #[default]
    Automatic,
    Pinned,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ProgramUpdateSettings {
    policy: InstanceProgramUpdatePolicy,
}

impl InstanceProgramUpdatePolicy {
    pub fn from_settings(settings: &Map<String, Value>) -> Result<Self, String> {
        let Some(value) = settings.get("program_update") else {
            return Ok(Self::default());
        };
        if !value.is_object() {
            return Err("invalid program_update: must be a JSON object".into());
        }
        serde_json::from_value::<ProgramUpdateSettings>(value.clone())
            .map(|settings| settings.policy)
            .map_err(|error| format!("invalid program_update: {error}"))
    }

    pub fn from_settings_json(settings_json: &str) -> Result<Self, String> {
        let settings = serde_json::from_str::<Map<String, Value>>(settings_json)
            .map_err(|error| format!("invalid instance settings: {error}"))?;
        Self::from_settings(&settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn program_update_defaults_to_automatic_and_accepts_pinning() {
        for value in [
            json!({}),
            json!({"program_update": {}}),
            json!({"program_update": {"policy": "automatic"}}),
        ] {
            assert_eq!(
                InstanceProgramUpdatePolicy::from_settings_json(&value.to_string()).unwrap(),
                InstanceProgramUpdatePolicy::Automatic
            );
        }
        assert_eq!(
            InstanceProgramUpdatePolicy::from_settings_json(
                r#"{"program_update":{"policy":"pinned"}}"#
            )
            .unwrap(),
            InstanceProgramUpdatePolicy::Pinned
        );
    }

    #[test]
    fn program_update_rejects_invalid_or_ambiguous_settings() {
        for value in [
            json!(null),
            json!([]),
            json!(false),
            json!("pinned"),
            json!({"policy": null}),
            json!({"policy": true}),
            json!({"policy": "latest"}),
            json!({"polciy": "pinned"}),
        ] {
            let settings = json!({"program_update": value});
            assert!(
                InstanceProgramUpdatePolicy::from_settings_json(&settings.to_string()).is_err()
            );
        }
        assert!(InstanceProgramUpdatePolicy::from_settings_json("[]").is_err());
    }
}
