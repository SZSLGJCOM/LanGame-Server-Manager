use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DragonwildsWorldMode {
    Normal,
    Hard,
    Creative,
    Custom,
}

impl DragonwildsWorldMode {
    pub fn native_value(self) -> u16 {
        match self {
            Self::Normal => 0,
            Self::Hard => 1,
            Self::Creative => 2,
            Self::Custom => 3,
        }
    }

    pub fn from_native(value: u16) -> Option<Self> {
        match value {
            0 => Some(Self::Normal),
            1 => Some(Self::Hard),
            2 => Some(Self::Creative),
            3 => Some(Self::Custom),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DragonwildsWorldSettingDefinition {
    pub tag: String,
    pub kind: String,
    pub player_adjustable: String,
    pub can_change_after_creation: bool,
    pub minimum: f64,
    pub maximum: f64,
    pub decimal_places: u32,
    pub gamepad_steps: u32,
    pub postfix: String,
    pub preset_defaults: BTreeMap<DragonwildsWorldMode, f64>,
}

impl DragonwildsWorldSettingDefinition {
    pub fn editable_in(&self, mode: DragonwildsWorldMode) -> bool {
        self.can_change_after_creation
            && match self.player_adjustable.as_str() {
                "OnlyCustom" => mode == DragonwildsWorldMode::Custom,
                "CustomAndCreative" => matches!(
                    mode,
                    DragonwildsWorldMode::Custom | DragonwildsWorldMode::Creative
                ),
                "AllModes" => true,
                _ => false,
            }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DragonwildsWorldSettingsSnapshot {
    pub instance_id: String,
    pub status: String,
    pub world_file: Option<String>,
    pub world_name: Option<String>,
    pub world_mode: Option<DragonwildsWorldMode>,
    pub revision: Option<String>,
    pub values: BTreeMap<String, f64>,
    pub overrides: BTreeMap<String, f64>,
    pub definitions: Vec<DragonwildsWorldSettingDefinition>,
    pub writable: bool,
    pub message: Option<String>,
    pub backup_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteDragonwildsWorldSettingsInput {
    pub instance_id: String,
    pub world_file: String,
    pub expected_revision: String,
    pub world_mode: DragonwildsWorldMode,
    pub values: BTreeMap<String, f64>,
}
