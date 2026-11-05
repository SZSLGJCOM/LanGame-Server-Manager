//! Compare explicit INI overrides with the independent native HTTPS readback.
use app_core::satisfactory_world::SatisfactoryWorldSnapshot;
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) fn verify(
    snapshot: &SatisfactoryWorldSnapshot,
    settings: &str,
) -> Result<BTreeMap<&'static str, i64>, String> {
    let settings: BTreeMap<String, Value> =
        serde_json::from_str(settings).map_err(|error| error.to_string())?;
    let mut verified = BTreeMap::new();
    for (setting, native, boolean) in [
        ("auto_pause_when_empty", "FG.DSAutoPause", true),
        ("network_quality", "FG.NetworkQuality", false),
        ("weather_preset", "FG.WeatherPreset", false),
        ("send_gameplay_data", "FG.SendGameplayData", true),
    ] {
        let Some(expected) = settings.get(setting) else {
            continue;
        };
        let expected = if boolean {
            expected.as_bool().map(i64::from)
        } else {
            expected.as_i64()
        }
        .ok_or_else(|| format!("Native Satisfactory fixture has an invalid {setting} override"))?;
        let actual = snapshot.server_options.get(native).and_then(|value| {
            match (boolean, value.to_ascii_lowercase().as_str()) {
                (true, "true") => Some(1),
                (true, "false") => Some(0),
                _ => value.parse::<i64>().ok(),
            }
        });
        if actual != Some(expected) {
            // Only report the known option name; arbitrary native values can
            // contain server identifiers and must not enter public evidence.
            return Err(format!(
                "Native Satisfactory effective option {native} is absent or differs from its explicit override"
            ));
        }
        verified.insert(native, expected);
    }
    Ok(verified)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_readback_requires_effective_false_and_rejects_missing_or_pending_only_values() {
        let mut snapshot = super::super::tests::snapshot();
        let settings = r#"{"send_gameplay_data":false,"network_quality":2}"#;
        snapshot
            .server_options
            .insert("FG.NetworkQuality".into(), "2".into());
        snapshot
            .pending_server_options
            .insert("FG.SendGameplayData".into(), "0".into());
        assert!(verify(&snapshot, settings).is_err());
        for invalid in ["true", "1", "false trailing", ""] {
            snapshot
                .server_options
                .insert("FG.SendGameplayData".into(), invalid.into());
            assert!(verify(&snapshot, settings).is_err());
        }
        for valid in ["false", "False", "0"] {
            snapshot
                .server_options
                .insert("FG.SendGameplayData".into(), valid.into());
            let verified = verify(&snapshot, settings).unwrap();
            assert_eq!(verified["FG.SendGameplayData"], 0);
            assert_eq!(verified["FG.NetworkQuality"], 2);
        }
        assert!(verify(&snapshot, r#"{"send_gameplay_data":"false"}"#).is_err());
        assert!(verify(&snapshot, "{}").unwrap().is_empty());
    }
}
