use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScumZoneHandling {
    Ignore,
    Allow,
    Block,
}

impl ScumZoneHandling {
    pub fn native_value(self) -> u64 {
        match self {
            Self::Ignore => 0,
            Self::Allow => 1,
            Self::Block => 2,
        }
    }

    pub fn from_native(value: u64) -> Option<Self> {
        match value {
            0 => Some(Self::Ignore),
            1 => Some(Self::Allow),
            2 => Some(Self::Block),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScumNpcVehicleDamage {
    pub melee: ScumZoneHandling,
    pub melee_weapon: ScumZoneHandling,
    pub projectile: ScumZoneHandling,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScumCustomZoneConfiguration {
    pub map_id: i64,
    pub configuration_id: i64,
    pub name: String,
    pub global: bool,
    pub regions: Vec<String>,
    pub damage: ScumNpcVehicleDamage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScumCustomZonesSnapshot {
    pub instance_id: String,
    pub status: ScumCustomZonesStatus,
    pub database_identity: Option<String>,
    pub revision: Option<String>,
    pub configurations: Vec<ScumCustomZoneConfiguration>,
    pub writable: bool,
    pub message: Option<String>,
    pub backup_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScumCustomZonesStatus {
    Ready,
    Empty,
    Running,
    Unsupported,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteScumCustomZoneDamageInput {
    pub instance_id: String,
    pub database_identity: String,
    pub expected_revision: String,
    pub map_id: i64,
    pub configuration_id: i64,
    pub damage: ScumNpcVehicleDamage,
}
