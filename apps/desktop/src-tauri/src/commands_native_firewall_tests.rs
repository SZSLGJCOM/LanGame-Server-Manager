use super::*;

#[test]
fn native_elevation_requires_scum_and_explicit_acknowledgement() {
    assert_eq!(
        elevated_fixture_allowed("scum", false, Some("true")),
        Ok(false)
    );
    assert_eq!(
        elevated_fixture_allowed("scum", true, Some("true")),
        Ok(true)
    );
    for acknowledgement in [None, Some("false"), Some("1")] {
        assert!(elevated_fixture_allowed("scum", true, acknowledgement).is_err());
    }
    assert!(elevated_fixture_allowed("squad", true, Some("true")).is_err());
}

#[test]
fn native_firewall_removes_only_its_exact_unchanged_rules() {
    let specs = vec![WindowsFirewallRuleSpec {
        rule_name: "LanGame native-fixture game UDP 19777".into(),
        protocol: "UDP".into(),
        local_port: 19777,
        local_address: "Any".into(),
    }];
    // All firewall commands are local functions. This test never accesses the
    // machine firewall and runs without administrator permissions.
    let shim = r#"
$script:items = @([pscustomobject]@{
  DisplayName='LanGame native-fixture game UDP 19777'; Group='LanGame Server Manager'; Direction='Inbound'; Action='Allow'
}, [pscustomobject]@{ DisplayName='unrelated'; Group='LanGame Server Manager'; Direction='Inbound'; Action='Allow' })
function Get-NetFirewallRule { param($DisplayName) $script:items }
function Get-NetFirewallPortFilter { param([Parameter(ValueFromPipeline=$true)]$InputObject) process { [pscustomobject]@{ Protocol='UDP'; LocalPort='19777' } } }
function Get-NetFirewallAddressFilter { param([Parameter(ValueFromPipeline=$true)]$InputObject) process { [pscustomobject]@{ LocalAddress='Any' } } }
function Remove-NetFirewallRule { param([Parameter(ValueFromPipeline=$true)]$InputObject) process { $script:items = @($script:items | Where-Object DisplayName -cne $InputObject.DisplayName) } }
"#;
    let remove = script(&specs, Action::RemoveOwned).unwrap();
    run(&format!("{shim}\n{remove}\nif ($script:items.Count -ne 1 -or $script:items[0].DisplayName -ne 'unrelated') {{ throw 'unrelated rule was changed' }}")).unwrap();
    let absent = script(&specs, Action::CheckAbsent).unwrap();
    assert!(run(&format!("{shim}\n{absent}")).is_err());
    assert!(
        run(&format!(
            "{shim}\n$script:items[0].Group='another owner'\n{remove}"
        ))
        .is_err()
    );
    run(&format!("{shim}\n$script:items=@()\n{absent}\n{remove}")).unwrap();
    let inaccessible = "function Get-NetFirewallRule { throw 'Provider inaccessible' }";
    assert!(run(&format!("{shim}\n{inaccessible}\n{absent}")).is_err());
    assert!(run(&format!("{shim}\n{inaccessible}\n{remove}")).is_err());
}

struct DstFirewallFixture {
    root: PathBuf,
    instance: InstanceDetails,
}

impl DstFirewallFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("ai-{}", uuid::Uuid::new_v4().simple()));
        let id = "instance-1720000000000-1234567890abcdef";
        let config = root.join("i").join(id).join("config");
        fs::create_dir_all(config.join("clusters/main")).unwrap();
        fs::write(config.join("instance.json"), b"{}").unwrap();
        let ports = [
            "master",
            "caves",
            "shard_master",
            "steam_query",
            "steam_auth",
            "caves_steam_query",
            "caves_steam_auth",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, name)| app_core::PortBinding {
            name: name.into(),
            protocol: "udp".into(),
            port: 19777 + index as u16,
        })
        .collect::<Vec<_>>();
        let instance = InstanceDetails {
            summary: app_core::InstanceSummary {
                id: id.into(),
                name: "本地开服验收".into(),
                module_id: "dontstarve".into(),
                status: app_core::InstanceStatus::Stopped,
                active_process_count: 0,
                bind_ip: "127.0.0.1".into(),
                port_count: ports.len(),
                autostart: false,
            },
            config_file_path: config.join("instance.json").to_string_lossy().into_owned(),
            saves_path: config.join("clusters/main").to_string_lossy().into_owned(),
            backup_uses_declared_saves_path: true,
            auto_backup_on_stop: false,
            backup_retention_count: 1,
            settings_json: serde_json::json!({"bind_ip":"127.0.0.1", "offline_cluster":true,
                "lan_only_cluster":true, "cluster_token":"", "enable_caves":false})
            .to_string(),
            ports,
            active_run: None,
        };
        Self { root, instance }
    }
}

impl Drop for DstFirewallFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("remove owned firewall path fixture");
    }
}

#[test]
fn dst_firewall_specs_accept_only_owned_loopback_paths_and_declared_udp_ports() {
    let fixture = DstFirewallFixture::new();
    let specs = dst_loopback_rule_specs(&fixture.instance, &fixture.root).unwrap();
    assert_eq!(specs.len(), 7);
    assert!(
        specs
            .iter()
            .all(|spec| spec.protocol == "UDP" && spec.local_address == "127.0.0.1")
    );
    assert_eq!(specs[0].local_port, 19777);
    assert_ne!(specs[0].rule_name, specs[1].rule_name);
    for invalid_id in [
        "12345678-1234-4321-8123-123456789abc",
        "../1234567890abcdef",
        "slug-1234567890abcdeg",
        "-slug-1234567890abcdef",
        "slug--name-1234567890abcdef",
    ] {
        let mut changed = fixture.instance.clone();
        changed.summary.id = invalid_id.into();
        assert!(
            dst_loopback_rule_specs(&changed, &fixture.root).is_err(),
            "accepted {invalid_id}"
        );
    }
    let mut changed = fixture.instance.clone();
    changed.summary.id = format!("{}-1234567890abcdef", "x".repeat(33));
    assert!(dst_loopback_rule_specs(&changed, &fixture.root).is_err());
    changed = fixture.instance.clone();
    changed.summary.module_id = "scum".into();
    assert!(dst_loopback_rule_specs(&changed, &fixture.root).is_err());
    changed = fixture.instance.clone();
    changed.summary.bind_ip = "0.0.0.0".into();
    assert!(dst_loopback_rule_specs(&changed, &fixture.root).is_err());
    changed = fixture.instance.clone();
    changed.config_file_path = Path::new(&fixture.instance.config_file_path)
        .with_file_name("other.json")
        .to_string_lossy()
        .into_owned();
    fs::write(&changed.config_file_path, b"{}").unwrap();
    assert!(dst_loopback_rule_specs(&changed, &fixture.root).is_err());
    changed = fixture.instance.clone();
    changed.saves_path = fixture.root.to_string_lossy().into_owned();
    assert!(dst_loopback_rule_specs(&changed, &fixture.root).is_err());
    assert!(dst_loopback_rule_specs(&fixture.instance, fixture.root.parent().unwrap()).is_err());
}

#[test]
fn dst_firewall_specs_reject_port_and_saved_network_scope_changes() {
    let fixture = DstFirewallFixture::new();
    for (field, value) in [
        ("bind_ip", serde_json::json!("0.0.0.0")),
        ("offline_cluster", serde_json::json!(false)),
        ("lan_only_cluster", serde_json::json!(false)),
        ("cluster_token", serde_json::json!("synthetic-token")),
        ("enable_caves", serde_json::json!(true)),
    ] {
        let mut changed = fixture.instance.clone();
        let mut settings: serde_json::Value = serde_json::from_str(&changed.settings_json).unwrap();
        settings[field] = value;
        changed.settings_json = settings.to_string();
        assert!(
            dst_loopback_rule_specs(&changed, &fixture.root).is_err(),
            "accepted {field}"
        );
    }
    for variant in 0..6 {
        let mut changed = fixture.instance.clone();
        match variant {
            0 => changed.ports[0].protocol = "tcp".into(),
            1 => changed.ports[0].port = 0,
            2 => changed.ports[0].name = "unrelated".into(),
            3 => changed.ports[1].port = changed.ports[0].port,
            4 => changed.ports[1].name = changed.ports[0].name.clone(),
            _ => changed.ports.push(changed.ports[0].clone()),
        }
        assert!(
            dst_loopback_rule_specs(&changed, &fixture.root).is_err(),
            "accepted port variant {variant}"
        );
    }
    let mut changed = fixture.instance.clone();
    changed.ports.clear();
    assert!(dst_loopback_rule_specs(&changed, &fixture.root).is_err());
}

#[test]
fn native_firewall_changed_identity_refuses_the_whole_batch_without_removing_rules() {
    let specs = [19777, 19778]
        .into_iter()
        .map(|port| WindowsFirewallRuleSpec {
            rule_name: format!("LanGame owned game UDP {port}"),
            protocol: "UDP".into(),
            local_port: port,
            local_address: "127.0.0.1".into(),
        })
        .collect::<Vec<_>>();
    // Each cmdlet is shadowed by a local function. Even error paths and absent
    // checks stay inside the shim and never use the machine firewall provider.
    let shim = r#"
$script:removed = @()
$script:items = @(19777,19778 | ForEach-Object { [pscustomobject]@{
  DisplayName="LanGame owned game UDP $_"; Group='LanGame Server Manager'; Direction='Inbound'; Action='Allow';
  Protocol='UDP'; LocalPort=[string]$_; LocalAddress='127.0.0.1'
} })
$script:items += [pscustomobject]@{ DisplayName='unrelated'; Group='another owner'; Direction='Outbound'; Action='Block'; Protocol='TCP'; LocalPort='444'; LocalAddress='Any' }
function Get-NetFirewallRule { param($DisplayName) $script:items }
function Get-NetFirewallPortFilter { param([Parameter(ValueFromPipeline=$true)]$InputObject) process { [pscustomobject]@{ Protocol=$InputObject.Protocol; LocalPort=$InputObject.LocalPort } } }
function Get-NetFirewallAddressFilter { param([Parameter(ValueFromPipeline=$true)]$InputObject) process { [pscustomobject]@{ LocalAddress=$InputObject.LocalAddress } } }
function Remove-NetFirewallRule { param([Parameter(ValueFromPipeline=$true)]$InputObject) process {
  $script:removed += $InputObject.DisplayName
  $script:items = @($script:items | Where-Object DisplayName -cne $InputObject.DisplayName)
} }
"#;
    let remove = script(&specs, Action::RemoveOwned).unwrap();
    run(&format!("{shim}\n{remove}\nif ($script:removed.Count -ne 2 -or $script:items.Count -ne 1 -or $script:items[0].DisplayName -cne 'unrelated') {{ throw 'exact-rule cleanup changed an unrelated rule' }}")).unwrap();
    for mutation in [
        "$script:items[1].Group='another owner'",
        "$script:items[1].LocalAddress='127.0.0.2'",
        "$script:items[1].LocalPort='19779'",
        "$script:items[1].Protocol='TCP'",
        "$script:items[1].Direction='Outbound'",
        "$script:items[1].Action='Block'",
        "$script:items[1].DisplayName=$script:items[1].DisplayName.ToLowerInvariant()",
    ] {
        run(&format!("{shim}\n{mutation}\n$caught = $false\ntry {{ {remove} }} catch {{ if ($_.Exception.Message -notlike '*rule changed; refusing cleanup*') {{ throw }}; $caught = $true }}\nif (!$caught -or $script:removed.Count -ne 0 -or $script:items.Count -ne 3 -or $script:items[2].DisplayName -cne 'unrelated') {{ throw 'changed-rule cleanup did not refuse without side effects' }}")).unwrap();
    }
    let absent = script(&specs, Action::CheckAbsent).unwrap();
    assert!(run(&format!("{shim}\n$script:items[0].DisplayName=$script:items[0].DisplayName.ToLowerInvariant()\n{absent}")).is_err());
    run(&format!("{shim}\n$script:items=@($script:items[2])\n{absent}\n{remove}\nif ($script:removed.Count -ne 0 -or $script:items.Count -ne 1) {{ throw 'absent owned rules changed unrelated rules' }}")).unwrap();
    run(&format!("{shim}\n$script:items[0].Protocol='17'\n$script:items[1].Protocol='17'\n{remove}\nif ($script:items.Count -ne 1) {{ throw 'Windows numeric UDP identity was rejected' }}")).unwrap();
}
