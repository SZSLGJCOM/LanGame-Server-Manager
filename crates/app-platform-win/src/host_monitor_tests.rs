use super::*;

fn adapter(name: &str, received: u64) -> NetworkAdapterRecord {
    serde_json::from_value(serde_json::json!({
        "Name": name,
        "Status": "Up",
        "ReceivedBytes": received,
        "SentBytes": 0,
    }))
    .unwrap()
}

#[test]
fn adapter_churn_releases_old_samples_and_reappearance_starts_a_new_baseline() {
    let mut monitor = WindowsHostMonitor::default();
    let initial = monitor.sample_network_adapter_records(vec![adapter("removed", 500)]);
    assert_eq!(initial[0].receive_bps, 0);
    assert_eq!(initial[0].rate_status, TelemetryStatus::WarmingUp);
    for index in 0..128 {
        let name = format!("temporary-{index}");
        let samples = monitor.sample_network_adapter_records(vec![adapter(&name, 100)]);
        assert_eq!(samples[0].receive_bps, 0);
        assert_eq!(monitor.previous_network_adapters.len(), 1);
        assert!(monitor.previous_network_adapters.contains_key(&name));
    }
    let returned = monitor.sample_network_adapter_records(vec![adapter("removed", 1000)]);
    assert_eq!(returned[0].receive_bps, 0);
    monitor.sample_network_adapter_records(Vec::new());
    assert!(monitor.previous_network_adapters.is_empty());
}

#[test]
fn retained_adapter_uses_its_previous_byte_counter() {
    let mut monitor = WindowsHostMonitor::default();
    monitor.previous_network_adapters.insert(
        "ethernet".into(),
        NetworkAdapterSample {
            received: 100,
            transmitted: 0,
            sampled_at: Instant::now() - Duration::from_secs(2),
        },
    );
    let samples = monitor.sample_network_adapter_records(vec![adapter("ethernet", 2100)]);
    assert!((1..=1000).contains(&samples[0].receive_bps));
    assert_eq!(samples[0].rate_status, TelemetryStatus::Valid);
    assert_eq!(monitor.previous_network_adapters["ethernet"].received, 2100);
}

#[test]
fn missing_adapter_statistics_are_unavailable_and_do_not_seed_a_false_zero_baseline() {
    let mut monitor = WindowsHostMonitor::default();
    let missing: NetworkAdapterRecord = serde_json::from_value(serde_json::json!({
        "Name": "ethernet", "Status": "Up", "ReceivedBytes": null, "SentBytes": null,
    }))
    .unwrap();
    let samples = monitor.sample_network_adapter_records(vec![missing]);
    assert_eq!(samples[0].rate_status, TelemetryStatus::Unavailable);
    assert_eq!(monitor.network_adapter_status, TelemetryStatus::Unavailable);
    assert!(monitor.previous_network_adapters.is_empty());
    monitor.sample_network_adapter_records(vec![adapter("ethernet", 1_000_000)]);
    assert_eq!(monitor.network_adapter_status, TelemetryStatus::WarmingUp);
}

#[test]
fn valid_network_fallback_does_not_make_new_adapter_zero_rate_valid() {
    let mut monitor = WindowsHostMonitor::default();
    let samples = monitor.sample_network_adapter_records(vec![adapter("new-adapter", 900)]);
    let aggregate_status = host_telemetry::combined_network_status(
        monitor.network_adapter_status,
        TelemetryStatus::Valid,
    );
    assert_eq!(aggregate_status, TelemetryStatus::Valid);
    assert_eq!(samples[0].receive_bps, 0);
    assert_eq!(samples[0].rate_status, TelemetryStatus::WarmingUp);
}

#[test]
fn adapter_inventory_failure_clears_baselines_and_marks_channel_unavailable() {
    let mut monitor = WindowsHostMonitor::default();
    monitor.sample_network_adapter_records(vec![adapter("ethernet", 500)]);
    monitor.sample_network_adapter_records(Vec::new());
    assert_eq!(monitor.network_adapter_status, TelemetryStatus::Unavailable);
    assert!(monitor.previous_network_adapters.is_empty());
}

#[test]
fn inactive_adapter_without_counters_does_not_invalidate_active_network_rates() {
    let mut monitor = WindowsHostMonitor::default();
    monitor.previous_network_adapters.insert(
        "ethernet".into(),
        NetworkAdapterSample {
            received: 100,
            transmitted: 0,
            sampled_at: Instant::now() - Duration::from_secs(2),
        },
    );
    let disabled: NetworkAdapterRecord = serde_json::from_value(serde_json::json!({
        "Name": "disabled-bluetooth", "Status": "Down",
        "ReceivedBytes": null, "SentBytes": null,
    }))
    .unwrap();
    let samples = monitor.sample_network_adapter_records(vec![adapter("ethernet", 2100), disabled]);
    assert_eq!(monitor.network_adapter_status, TelemetryStatus::Valid);
    assert_eq!(
        samples
            .iter()
            .find(|sample| sample.name == "disabled-bluetooth")
            .unwrap()
            .rate_status,
        TelemetryStatus::Unavailable
    );
    assert!(
        samples
            .iter()
            .any(|sample| sample.rate_status == TelemetryStatus::Valid && sample.receive_bps > 0)
    );
}

#[test]
fn missing_active_adapter_counters_keep_partial_network_totals_unavailable() {
    let mut monitor = WindowsHostMonitor::default();
    monitor.previous_network_adapters.insert(
        "ethernet".into(),
        NetworkAdapterSample {
            received: 100,
            transmitted: 0,
            sampled_at: Instant::now() - Duration::from_secs(2),
        },
    );
    let missing: NetworkAdapterRecord = serde_json::from_value(serde_json::json!({
        "Name": "second-active", "Status": "Up", "ReceivedBytes": null, "SentBytes": null,
    }))
    .unwrap();
    monitor.sample_network_adapter_records(vec![adapter("ethernet", 2100), missing]);
    assert_eq!(monitor.network_adapter_status, TelemetryStatus::Unavailable);
}

#[test]
fn inactive_adapter_without_counters_preserves_active_adapter_warmup() {
    let mut monitor = WindowsHostMonitor::default();
    let disabled: NetworkAdapterRecord = serde_json::from_value(serde_json::json!({
        "Name": "disabled-bluetooth", "Status": "Down", "ReceivedBytes": null, "SentBytes": null,
    }))
    .unwrap();
    monitor.sample_network_adapter_records(vec![adapter("new-active", 100), disabled]);
    assert_eq!(monitor.network_adapter_status, TelemetryStatus::WarmingUp);
}

#[test]
fn network_counter_parser_accepts_english_and_chinese_byte_rows() {
    let english = "Interface Statistics\r\n\r\n  Received Sent\r\nBytes 2281900541 3901391899\r\nUnicast packets 30661479 16636440\r\n";
    let chinese = "接口统计\r\n\r\n  接收 发送\r\n字节 2281900541 3901391899\r\n单播数据包 30661479 16636440\r\n";
    for output in [english, chinese] {
        assert_eq!(parse_network_totals(output), Some((2281900541, 3901391899)));
    }
    assert_eq!(
        parse_network_totals("Bytes 1,234 56,789"),
        Some((1234, 56789))
    );
}

#[test]
fn network_counter_parser_rejects_non_byte_rows_and_malformed_values() {
    for output in [
        "Received Sent\r\nErrors 2 5",
        "接收 发送\r\n错误 2 5",
        "Bytes -1 3",
        "Bytes 1 2 3",
        "Bytes 1,2 3",
        "Bytes +1 3",
        "Bytes 18446744073709551616 3",
    ] {
        assert_eq!(
            parse_network_totals(output),
            None,
            "incorrectly accepted {output:?}"
        );
    }
}

#[test]
#[ignore = "read-only live Windows capture probe; run explicitly when investigating host telemetry"]
fn live_full_capture_reports_fresh_channels_and_network_inventory() {
    match system_utility::capture(SystemUtility::Network, &["-e"], QUERY_TIMEOUT) {
        Ok(output) => eprintln!(
            "{}",
            serde_json::json!({
                "probe": "native_network_counters",
                "success": output.status.success(),
                "stdout_bytes": output.stdout.len(),
                "counters_parsed": host_telemetry::decode_network_counter_output(&output.stdout)
                    .and_then(|text| parse_network_totals(&text)).is_some(),
            })
        ),
        Err(error) => eprintln!(
            "{}",
            serde_json::json!({
                "probe": "native_network_counters", "error_kind": format!("{:?}", error.kind()),
                "os_error": error.raw_os_error(),
            })
        ),
    }
    let temp = std::env::temp_dir();
    assert!(temp.is_dir(), "the configured OS temp directory must exist");
    let monitored_path = temp.to_string_lossy().into_owned();
    let mut monitor = WindowsHostMonitor::default();
    for sample in 1..=2 {
        let started = Instant::now();
        let snapshot =
            monitor.capture_paths(&monitored_path, std::slice::from_ref(&monitored_path));
        let adapter_quality = snapshot
            .network_adapters
            .iter()
            .map(|adapter| {
                serde_json::json!({
                    "link_status": adapter.status,
                    "rate_status": adapter.rate_status,
                    "receive_bps": adapter.receive_bps,
                    "transmit_bps": adapter.transmit_bps,
                    "has_link_speed": adapter.link_speed_bps > 0,
                })
            })
            .collect::<Vec<_>>();
        eprintln!(
            "{}",
            serde_json::json!({
                "probe": "windows_host_capture", "sample": sample,
                "elapsed_ms": started.elapsed().as_millis(),
                "telemetry": snapshot.telemetry,
                "adapter_count": snapshot.network_adapters.len(),
                "adapters": adapter_quality,
                "network_receive_bps": snapshot.network_receive_bps,
                "network_transmit_bps": snapshot.network_transmit_bps,
            })
        );
        assert_eq!(snapshot.telemetry.memory, TelemetryStatus::Valid);
        assert_eq!(snapshot.telemetry.disk_capacity, TelemetryStatus::Valid);
        assert!(
            !snapshot.network_adapters.is_empty(),
            "live network inventory is missing"
        );
        if sample == 2 {
            assert_eq!(snapshot.telemetry.network, TelemetryStatus::Valid);
            assert!(snapshot.network_adapters.iter().any(|adapter| {
                adapter.status.eq_ignore_ascii_case("up")
                    && adapter.rate_status == TelemetryStatus::Valid
            }));
        }
    }
}
