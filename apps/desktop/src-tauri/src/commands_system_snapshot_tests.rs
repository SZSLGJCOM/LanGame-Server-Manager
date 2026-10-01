use super::*;
use app_core::{DiskVolumeSnapshot, SystemTelemetry, TelemetryStatus};

#[test]
fn snapshot_preserves_host_quality_timestamp_and_all_storage_volumes() {
    let host = HostMetricsSnapshot {
        telemetry: SystemTelemetry {
            observed_at_unix_ms: Some(123_456),
            cpu: TelemetryStatus::WarmingUp,
            memory: TelemetryStatus::Valid,
            disk_capacity: TelemetryStatus::Unavailable,
            ..SystemTelemetry::default()
        },
        memory_commit_used_bytes: Some(10),
        memory_commit_limit_bytes: Some(40),
        disk_model: "Fixture NVMe".into(),
        disk_volume_id: "fixture-volume".into(),
        disk_volumes: vec![DiskVolumeSnapshot {
            id: "offline-volume".into(),
            paths: vec!["Z:/saves".into()],
            status: TelemetryStatus::Unavailable,
            ..DiskVolumeSnapshot::default()
        }],
        ..HostMetricsSnapshot::default()
    };
    let snapshot = build_system_snapshot(
        host,
        InstanceProcessMemorySnapshot::default(),
        2,
        GlobalPlayerCountSnapshot::default(),
    );
    assert_eq!(snapshot.telemetry.observed_at_unix_ms, Some(123_456));
    assert_eq!(snapshot.telemetry.cpu, TelemetryStatus::WarmingUp);
    assert_eq!(snapshot.memory_commit_used_bytes, Some(10));
    assert_eq!(snapshot.disk_model, "Fixture NVMe");
    assert_eq!(snapshot.disk_volume_id, "fixture-volume");
    assert_eq!(
        snapshot.disk_volumes[0].status,
        TelemetryStatus::Unavailable
    );
    assert_eq!(snapshot.running_instances, 2);
}

#[test]
fn updating_running_instances_does_not_make_cached_telemetry_fresh() {
    let cached = SystemSnapshot {
        telemetry: SystemTelemetry {
            observed_at_unix_ms: Some(10),
            cpu: TelemetryStatus::Valid,
            ..SystemTelemetry::default()
        },
        running_instances: 7,
        total_online_players: 11,
        ..SystemSnapshot::default()
    };
    let current = with_current_running_instances(cached, &AppState::default());
    assert_eq!(current.telemetry.observed_at_unix_ms, Some(10));
    assert_eq!(current.telemetry.cpu, TelemetryStatus::Valid);
    assert_eq!(current.running_instances, 0);
    assert_eq!(current.total_online_players, 11);
}

#[test]
fn metadata_reload_retains_cached_hardware_and_original_observation_time() {
    let cached = SystemSnapshot {
        telemetry: SystemTelemetry {
            observed_at_unix_ms: Some(123_456),
            network: TelemetryStatus::Valid,
            ..SystemTelemetry::default()
        },
        network_receive_bps: 512,
        network_adapters: vec![NetworkAdapterSnapshot {
            name: "Ethernet".into(),
            rate_status: TelemetryStatus::Valid,
            ..NetworkAdapterSnapshot::default()
        }],
        running_instances: 9,
        ..SystemSnapshot::default()
    };
    let mut cache = crate::state::TimedCache::default();
    cache.store(cached);
    let snapshot = lightweight_system_snapshot(&AppState::default(), cache.latest());
    assert_eq!(snapshot.telemetry.observed_at_unix_ms, Some(123_456));
    assert_eq!(snapshot.telemetry.network, TelemetryStatus::Valid);
    assert_eq!(snapshot.network_adapters[0].name, "Ethernet");
    assert_eq!(snapshot.network_receive_bps, 512);
    assert_eq!(snapshot.running_instances, 0);
}

#[test]
fn metadata_reload_without_a_sample_does_not_invent_telemetry() {
    let snapshot = lightweight_system_snapshot(&AppState::default(), None);
    assert_eq!(snapshot.telemetry.observed_at_unix_ms, None);
    assert!(snapshot.network_adapters.is_empty());
}

#[test]
fn snapshot_preserves_each_adapter_quality_even_when_aggregate_network_is_valid() {
    let host = HostMetricsSnapshot {
        telemetry: SystemTelemetry {
            network: TelemetryStatus::Valid,
            ..SystemTelemetry::default()
        },
        network_adapters: vec![app_platform_win::NetworkAdapterMetrics {
            name: "new-adapter".into(),
            rate_status: TelemetryStatus::WarmingUp,
            ..app_platform_win::NetworkAdapterMetrics::default()
        }],
        ..HostMetricsSnapshot::default()
    };
    let snapshot = build_system_snapshot(
        host,
        InstanceProcessMemorySnapshot::default(),
        0,
        GlobalPlayerCountSnapshot::default(),
    );
    assert_eq!(snapshot.telemetry.network, TelemetryStatus::Valid);
    assert_eq!(
        snapshot.network_adapters[0].rate_status,
        TelemetryStatus::WarmingUp
    );
    assert_eq!(snapshot.network_adapters[0].receive_bps, 0);
}
