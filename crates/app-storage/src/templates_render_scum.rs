use serde::Deserialize;
use serde_json::{Map, Value};

const SCUM_GENERAL_INVENTORY: &str =
    include_str!("../../../modules/scum/server-settings-v7/general.json");
const SCUM_WORLD_INVENTORY: &str =
    include_str!("../../../modules/scum/server-settings-v7/world.json");
const SCUM_FEATURES_INVENTORY: &str =
    include_str!("../../../modules/scum/server-settings-v7/features.json");
const SCUM_RESPAWN_INVENTORY: &str =
    include_str!("../../../modules/scum/server-settings-v7/respawn.json");
const SCUM_VEHICLES_INVENTORY: &str =
    include_str!("../../../modules/scum/server-settings-v7/vehicles.json");
const SCUM_DAMAGE_INVENTORY: &str =
    include_str!("../../../modules/scum/server-settings-v7/damage.json");
const SCUM_ECONOMY_DEFAULTS: &str =
    include_str!("../../../modules/scum/native-defaults/EconomyOverride.json");
const SCUM_RAID_DEFAULTS: &str =
    include_str!("../../../modules/scum/native-defaults/RaidTimes.json");
const SCUM_NOTIFICATION_DEFAULTS: &str =
    include_str!("../../../modules/scum/native-defaults/Notifications.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScumNativeSetting {
    key: String,
    native_key: String,
    native_default: String,
    #[serde(rename = "type")]
    value_type: ScumNativeValueType,
    default: Value,
    presentation: ScumSettingPresentation,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ScumNativeValueType {
    Boolean,
    Integer,
    Number,
    String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum ScumSettingPresentation {
    Specialized,
    Generated,
}

const SECTION_INVENTORIES: &[(&str, &str, &str)] = &[
    ("General", "server_general", SCUM_GENERAL_INVENTORY),
    ("World", "server_world", SCUM_WORLD_INVENTORY),
    ("Features", "server_features", SCUM_FEATURES_INVENTORY),
    ("Respawn", "server_respawn", SCUM_RESPAWN_INVENTORY),
    ("Vehicles", "server_vehicles", SCUM_VEHICLES_INVENTORY),
    ("Damage", "server_damage", SCUM_DAMAGE_INVENTORY),
];

pub(super) fn render_scum_server_settings_ini(settings: &Map<String, Value>) -> Option<String> {
    let mut lines = Vec::new();
    for (section, settings_field, inventory_json) in SECTION_INVENTORIES {
        let inventory = serde_json::from_str::<Vec<ScumNativeSetting>>(inventory_json).ok()?;
        lines.push(format!("[{section}]"));
        let configured = settings.get(*settings_field).and_then(Value::as_object);
        for setting in inventory {
            if setting.presentation == ScumSettingPresentation::Generated {
                continue;
            }
            let value = configured
                .and_then(|section| section.get(&setting.key))
                .unwrap_or(&setting.default);
            lines.push(format!(
                "{}={}",
                setting.native_key,
                render_native_value(&setting, value)
            ));
        }
        lines.push(String::new());
    }
    Some(lines.join("\n"))
}

pub(super) fn render_scum_economy_override(settings: &Map<String, Value>) -> Option<String> {
    render_structured_json_root(
        settings.get("economy_override"),
        SCUM_ECONOMY_DEFAULTS,
        "economy-override",
    )
}

pub(super) fn render_scum_raid_times(settings: &Map<String, Value>) -> Option<String> {
    render_structured_json_root(
        settings.get("raid_times"),
        SCUM_RAID_DEFAULTS,
        "raiding-times",
    )
}

pub(super) fn render_scum_notifications(settings: &Map<String, Value>) -> Option<String> {
    render_structured_json_root(
        settings.get("notifications"),
        SCUM_NOTIFICATION_DEFAULTS,
        "Notifications",
    )
}

fn render_native_value(setting: &ScumNativeSetting, value: &Value) -> String {
    match setting.value_type {
        ScumNativeValueType::Boolean => value
            .as_bool()
            .map(|value| if value { "True" } else { "False" }.to_string())
            .unwrap_or_else(|| setting.native_default.clone()),
        ScumNativeValueType::Integer => value
            .as_i64()
            .map(|value| value.to_string())
            .unwrap_or_else(|| setting.native_default.clone()),
        ScumNativeValueType::Number => value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(|value| value.to_string())
            .unwrap_or_else(|| setting.native_default.clone()),
        ScumNativeValueType::String => value
            .as_str()
            .map(sanitize_ini_value)
            .unwrap_or_else(|| setting.native_default.clone()),
    }
}

fn sanitize_ini_value(value: &str) -> String {
    value
        .replace("\r\n", " ")
        .replace(['\r', '\n'], " ")
        .trim_end()
        .to_string()
}

fn render_structured_json_root(
    configured: Option<&Value>,
    default_document: &str,
    root_key: &str,
) -> Option<String> {
    let defaults = serde_json::from_str::<Value>(default_document).ok()?;
    let value = configured
        .filter(|value| match root_key {
            "economy-override" => {
                value.is_object() && value.as_object().is_some_and(|v| !v.is_empty())
            }
            _ => value.is_array(),
        })
        .or_else(|| defaults.get(root_key))?
        .clone();
    let document = Value::Object(Map::from_iter([(root_key.to_string(), value)]));
    serde_json::to_string_pretty(&document)
        .ok()
        .map(|mut rendered| {
            rendered.push('\n');
            rendered
        })
}
