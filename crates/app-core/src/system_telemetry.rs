use serde::{Deserialize, Serialize};

/// A numeric zero is meaningful only when its channel is valid.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryStatus {
    Valid,
    WarmingUp,
    #[default]
    Unavailable,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SystemTelemetry {
    pub observed_at_unix_ms: Option<u64>,
    pub cpu: TelemetryStatus,
    pub cpu_cores: TelemetryStatus,
    pub memory: TelemetryStatus,
    pub disk_capacity: TelemetryStatus,
    pub disk_io: TelemetryStatus,
    pub network: TelemetryStatus,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiskVolumeSnapshot {
    pub id: String,
    pub label: String,
    pub paths: Vec<String>,
    pub total_bytes: u64,
    /// Space available to this user; quotas may make it smaller than free_bytes.
    pub available_bytes: u64,
    pub free_bytes: u64,
    pub status: TelemetryStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_telemetry_is_unavailable_and_has_no_observation_time() {
        let snapshot = crate::SystemSnapshot::default();
        let serialized = serde_json::to_value(snapshot).unwrap();
        assert_eq!(
            serialized["telemetry"]["observed_at_unix_ms"],
            serde_json::Value::Null
        );
        for channel in [
            "cpu",
            "cpu_cores",
            "memory",
            "disk_capacity",
            "disk_io",
            "network",
        ] {
            assert_eq!(serialized["telemetry"][channel], "unavailable");
        }
        assert_eq!(serialized["disk_volumes"], serde_json::json!([]));
    }

    #[test]
    fn telemetry_round_trip_preserves_quality_and_capture_time() {
        let telemetry = SystemTelemetry {
            observed_at_unix_ms: Some(1_700_000_000_000),
            cpu: TelemetryStatus::WarmingUp,
            memory: TelemetryStatus::Valid,
            ..SystemTelemetry::default()
        };
        let round_trip: SystemTelemetry =
            serde_json::from_value(serde_json::to_value(telemetry).unwrap()).unwrap();
        assert_eq!(round_trip.observed_at_unix_ms, Some(1_700_000_000_000));
        assert_eq!(round_trip.cpu, TelemetryStatus::WarmingUp);
        assert_eq!(round_trip.memory, TelemetryStatus::Valid);
        assert_eq!(round_trip.disk_capacity, TelemetryStatus::Unavailable);
    }
}
