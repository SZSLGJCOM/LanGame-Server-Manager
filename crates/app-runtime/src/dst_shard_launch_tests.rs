use super::*;
use serde_json::json;

fn game_ports() -> Vec<PortBinding> {
    app_core::dst_shards::DST_SHARDS
        .iter()
        .enumerate()
        .map(|(index, shard)| PortBinding {
            name: String::from(shard.game_port),
            protocol: String::from("udp"),
            port: 11_019 + index as u16,
        })
        .collect()
}

#[test]
fn island_adventures_offline_discovery_checks_all_four_enabled_game_ports() {
    let issues = collect_dontstarve_launch_setting_issues(
        &json!({
            "shard_layout": "island_adventures",
            "enable_caves": false,
            "offline_cluster": true
        }),
        &game_ports(),
    );
    let rejected = issues
        .iter()
        .filter(|issue| issue.code == "dst_lan_port_out_of_range")
        .map(|issue| issue.context["port_name"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(rejected, ["master", "caves", "islands", "volcano"]);
}

#[test]
fn standard_offline_discovery_ignores_disabled_islands_and_volcano_ports() {
    let issues = collect_dontstarve_launch_setting_issues(
        &json!({ "enable_caves": true, "offline_cluster": true }),
        &game_ports(),
    );
    let rejected = issues
        .iter()
        .filter(|issue| issue.code == "dst_lan_port_out_of_range")
        .map(|issue| issue.context["port_name"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(rejected, ["master", "caves"]);
}

#[test]
fn island_adventures_accepts_four_distinct_ports_in_the_native_discovery_range() {
    let mut ports = game_ports();
    for (index, binding) in ports.iter_mut().enumerate() {
        binding.port = 10_999 + index as u16;
    }
    let issues = collect_dontstarve_launch_setting_issues(
        &json!({ "shard_layout": "island_adventures", "offline_cluster": true }),
        &ports,
    );
    assert!(issues.is_empty(), "{issues:?}");
}

#[test]
fn invalid_shard_layout_is_a_blocking_launch_issue() {
    let issues = collect_dontstarve_launch_setting_issues(
        &json!({ "shard_layout": "invalid", "offline_cluster": true }),
        &game_ports(),
    );
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].code, "dst_shard_layout_invalid");
    assert_eq!(issues[0].severity, "error");
}
