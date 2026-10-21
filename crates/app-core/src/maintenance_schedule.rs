use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DailyMaintenanceAction {
    Backup,
    Restart,
}

impl DailyMaintenanceAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Backup => "backup",
            Self::Restart => "restart",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DailyMaintenanceRun {
    pub local_date: String,
    pub status: String,
    pub started_at_unix_ms: u64,
    pub completed_at_unix_ms: Option<u64>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DailyMaintenanceSchedule {
    pub instance_id: String,
    pub action: DailyMaintenanceAction,
    pub enabled: bool,
    pub time_local: String,
    pub revision: i64,
    pub last_run: Option<DailyMaintenanceRun>,
}

impl DailyMaintenanceSchedule {
    pub fn disabled(instance_id: &str, action: DailyMaintenanceAction) -> Self {
        Self {
            instance_id: instance_id.to_owned(),
            action,
            enabled: false,
            time_local: "04:00".into(),
            revision: 0,
            last_run: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDailyMaintenanceScheduleInput {
    pub instance_id: String,
    pub action: DailyMaintenanceAction,
    pub enabled: bool,
    pub time_local: String,
    pub expected_revision: i64,
}

/// Strict wall-clock input shared by persistence and the daily scheduler.
pub fn valid_daily_time(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 5
        && bytes[2] == b':'
        && [bytes[0], bytes[1], bytes[3], bytes[4]]
            .iter()
            .all(u8::is_ascii_digit)
        && &value[..2] <= "23"
        && &value[3..] <= "59"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_time_is_exact_and_bounded() {
        for value in ["00:00", "04:00", "23:59"] {
            assert!(valid_daily_time(value));
        }
        for value in [
            "4:00",
            "24:00",
            "23:60",
            " 04:00",
            "04:00:00",
            "é:00",
            "１２:３０",
        ] {
            assert!(!valid_daily_time(value), "{value}");
        }
    }
}
