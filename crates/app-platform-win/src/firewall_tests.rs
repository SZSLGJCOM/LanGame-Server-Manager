use super::*;
use std::cell::Cell;

fn specs() -> Vec<WindowsFirewallRuleSpec> {
    vec![
        WindowsFirewallRuleSpec {
            rule_name: "LanGame alpha game UDP 7778".into(),
            protocol: "UDP".into(),
            local_port: 7778,
            local_address: "192.168.31.150".into(),
        },
        WindowsFirewallRuleSpec {
            rule_name: "LanGame alpha query TCP 27017".into(),
            protocol: "TCP".into(),
            local_port: 27017,
            local_address: "192.168.31.150".into(),
        },
    ]
}

fn reply(specs: &[WindowsFirewallRuleSpec], state: &str) -> Reply {
    Reply {
        state: state.into(),
        rules: specs
            .iter()
            .map(|spec| WindowsFirewallRuleApplyResult {
                rule_name: spec.rule_name.clone(),
                protocol: spec.protocol.clone(),
                local_port: spec.local_port,
                local_address: spec.local_address.clone(),
                status: "existing".into(),
                message: None,
            })
            .collect(),
    }
}

fn reason(error: String) -> String {
    let value: Value = serde_json::from_str(&error).unwrap();
    assert_eq!(value["code"], "windows_firewall_setup_failed");
    value["reason"].as_str().unwrap().into()
}

#[test]
fn firewall_existing_verified_rules_need_neither_token_query_nor_uac() {
    let specs = specs();
    let mut calls = Vec::new();
    let result = apply_with(
        &specs,
        |apply| {
            calls.push(apply);
            Ok(reply(&specs, "ready"))
        },
        || panic!("existing rules need no token query"),
        |_| panic!("existing rules need no UAC"),
    );
    assert_eq!(result.unwrap().len(), 2);
    assert_eq!(calls, [false]);
}

#[test]
fn firewall_missing_rules_use_one_uac_and_independent_readback() {
    let specs = specs();
    let mut calls = Vec::new();
    let elevated = Cell::new(0);
    let result = apply_with(
        &specs,
        |apply| {
            calls.push(apply);
            Ok(reply(
                &specs,
                if calls.len() == 1 { "repair" } else { "ready" },
            ))
        },
        || Ok(false),
        |rules| {
            elevated.set(elevated.get() + 1);
            assert_eq!(parse_rules(rules).unwrap(), specs);
            Ok(json!({"state":"ready", "rules":reply(&specs, "ready").rules}))
        },
    );
    assert_eq!(result.unwrap().len(), 2);
    assert_eq!(calls, [false, false]);
    assert_eq!(elevated.get(), 1);
}

#[test]
fn firewall_elevated_owner_applies_without_another_uac() {
    let specs = specs();
    let mut calls = Vec::new();
    apply_with(
        &specs,
        |apply| {
            calls.push(apply);
            Ok(reply(
                &specs,
                if calls.len() == 1 { "repair" } else { "ready" },
            ))
        },
        || Ok(true),
        |_| panic!("elevated owner does not request UAC"),
    )
    .unwrap();
    assert_eq!(calls, [false, true, false]);
}

#[test]
fn firewall_foreign_rule_rejects_before_uac_and_unknown_errors_are_redacted() {
    let specs = specs();
    for (state, expected) in [
        ("rule_conflict", "rule_conflict"),
        ("https://private.invalid/?token=secret", "apply_failed"),
    ] {
        let error = apply_with(
            &specs,
            |_| Ok(reply(&specs, state)),
            || panic!("conflict needs no token query"),
            |_| panic!("conflict needs no UAC"),
        )
        .unwrap_err();
        assert!(!error.contains("secret"));
        assert_eq!(reason(error), expected);
    }
}

#[test]
fn firewall_cancelled_uac_and_unverified_receipts_never_report_success() {
    let specs = specs();
    let error = apply_with(
        &specs,
        |_| Ok(reply(&specs, "repair")),
        || Ok(false),
        |_| Err(std::io::Error::from_raw_os_error(1223)),
    )
    .unwrap_err();
    assert_eq!(reason(error), "elevation_cancelled");
    let error = apply_with(
        &specs,
        |_| Ok(reply(&specs, "repair")),
        || Ok(false),
        |_| Ok(json!({"state":"ready", "rules":[]})),
    )
    .unwrap_err();
    assert_eq!(reason(error), "verification_failed");
    // Even a well-shaped receipt cannot replace the final non-elevated readback.
    let error = apply_with(
        &specs,
        |_| Ok(reply(&specs, "repair")),
        || Ok(false),
        |_| Ok(json!({"state":"ready", "rules":reply(&specs,"ready").rules})),
    )
    .unwrap_err();
    assert_eq!(reason(error), "verification_failed");
}

#[test]
fn firewall_helper_rejects_extra_fields_commands_ports_and_unbounded_rules() {
    let rules = serde_json::to_value(specs()).unwrap();
    assert_eq!(parse_rules(&rules).unwrap().len(), 2);
    let mut extra = rules.clone();
    extra[0]["script"] = json!("not a supported operation");
    assert!(parse_rules(&extra).is_err());
    for (field, value) in [
        ("local_port", json!(0)),
        ("protocol", json!("Any")),
        ("local_address", json!("Any; other-command")),
        ("rule_name", json!("LanGame ' \n'@\nother-command")),
    ] {
        let mut invalid = rules.clone();
        invalid[0][field] = value;
        assert!(parse_rules(&invalid).is_err(), "{field}");
    }
    assert!(parse_rules(&json!({"rules":rules})).is_err());
    assert!(parse_rules(&json!([])).is_err());
    assert!(parse_rules(&json!(vec![rules[0].clone(); MAX_RULES + 1])).is_err());
    let mut duplicate = rules.clone();
    duplicate[1] = rules[0].clone();
    assert!(parse_rules(&duplicate).is_err());
}

#[test]
fn firewall_valid_schema_is_rejected_before_exceeding_windows_command_capacity() {
    let specs = (0..MAX_RULES)
        .map(|index| WindowsFirewallRuleSpec {
            rule_name: format!("LanGame {index:03} {}", "x".repeat(146)),
            protocol: "UDP".into(),
            local_port: 7000 + index as u16,
            local_address: "Any".into(),
        })
        .collect::<Vec<_>>();
    assert!(
        validate(&specs).is_ok(),
        "schema and IPC bounds alone permit this batch"
    );
    assert_eq!(script(&specs, false).unwrap_err(), "invalid_rules");
    assert_eq!(script(&specs, true).unwrap_err(), "invalid_rules");
    assert!(check_script_argument(&"a".repeat(MAX_SCRIPT_ARGUMENT_UNITS - 2)).is_ok());
    assert!(check_script_argument(&"a".repeat(MAX_SCRIPT_ARGUMENT_UNITS - 1)).is_err());
    assert!(check_script_argument(&"🙂".repeat((MAX_SCRIPT_ARGUMENT_UNITS - 2) / 2)).is_ok());
    assert!(check_script_argument(&"🙂".repeat(MAX_SCRIPT_ARGUMENT_UNITS / 2)).is_err());
    assert!(
        check_script_argument(&"\\\"".repeat(MAX_SCRIPT_ARGUMENT_UNITS / 3)).is_err(),
        "Windows quoting overhead must be counted"
    );
}

// These fixtures replace Windows firewall cmdlets, never the orchestration or
// production script. No test here reads or modifies the host's firewall.
const SHIMS: &str = r#"
$script:Items = @()
$script:Created = 0
$script:Removed = 0
$script:FailCreate = $false
$script:ForgetCreate = $false
$script:FailRemove = 0
$script:FailCleanup = $false
$script:ChangeCreatedOnRemove = $false
$script:CorruptCreate = $false
$script:RemoveThenFail = 0
$script:FilterReads = @{}
$script:BulkReads = @{}
$script:AssociatedReads = @{}
$script:DeniedBulkTypes = @()
$script:DeniedBulkException = $false
$script:DeniedAssociatedType = ''
$script:FailedBulkType = ''
$script:FailedAssociatedType = ''
$script:RuleReads = 0
$script:FailCreateAt = 0
$script:CorruptCreateAt = 0
$script:FailOldRemoveAt = 0
$script:OldRemoves = 0
$script:DuplicatePortFilter = $false
$script:OmitPortFilter = $false
$script:WrongPortFilterId = $false
$script:ReverseFilters = $false
$script:ConflictBeforeCreation = $false
function New-TestRule($spec, $group) {
  $id=[guid]::NewGuid().ToString()
  [pscustomobject]@{ Name = $id; InstanceID = $id; DisplayName = $spec.rule_name;
    Group = $group; Description = ''; Enabled = 'True'; Direction = 'Inbound'; Action = 'Allow'; Profile = 'Any'; EdgeTraversalPolicy = 'Block';
    Port = [pscustomobject]@{Protocol=$spec.protocol;LocalPort=$spec.local_port;RemotePort='Any'};
    Address = [pscustomobject]@{LocalAddress=$spec.local_address;RemoteAddress='Any'} }
}
function Get-NetFirewallRule {
  [CmdletBinding()] param([string]$PolicyStore, [string]$Name)
  $script:RuleReads++
  if ($script:ConflictBeforeCreation -and $script:RuleReads -eq 2) { $script:Items += New-TestRule $fixtureSpecs[1] 'Operator owned' }
  if ($Name) { @($script:Items | Where-Object { $_.Name -eq $Name }) }
  else { $script:Items }
}
function Read-TestFilters([string]$type, $associatedRule) {
  $script:FilterReads[$type]++
  if ($null -eq $associatedRule) {
    $script:BulkReads[$type]++
    if ($script:DeniedBulkTypes -contains $type) {
      if ($script:DeniedBulkException) {
        throw [UnauthorizedAccessException]::new('simulated denied bulk read with private details')
      }
      Write-Error -Message 'simulated denied bulk read with private details' -Category PermissionDenied -ErrorAction Stop
    }
    if ($script:FailedBulkType -eq $type) { throw 'simulated unknown failure: PermissionDenied Error 5 with private details' }
    $items = $script:Items
  } else {
    $script:AssociatedReads[$type]++
    if ($associatedRule -is [array]) { throw 'association accepts one rule only' }
    if ($script:DeniedAssociatedType -eq $type) {
      throw [UnauthorizedAccessException]::new('simulated denied associated read with private details')
    }
    if ($script:FailedAssociatedType -eq $type) { throw 'simulated unknown associated failure with private details' }
    $items = @($associatedRule)
  }
  $output=@(foreach($rule in $items) {
    $id=$rule.Name
    if($type -eq 'Port' -and $script:OmitPortFilter) { continue }
    if($type -eq 'Port' -and $script:WrongPortFilterId) { $id='unrelated-'+$id }
    switch($type) {
      'Port' { [pscustomobject]@{InstanceID=$id;Protocol=$rule.Port.Protocol;LocalPort=$rule.Port.LocalPort;RemotePort=$rule.Port.RemotePort} }
      'Address' { [pscustomobject]@{InstanceID=$id;LocalAddress=$rule.Address.LocalAddress;RemoteAddress=$rule.Address.RemoteAddress} }
      'Application' { [pscustomobject]@{InstanceID=$id;Program='Any'} }
      'Service' { [pscustomobject]@{InstanceID=$id;Service='Any'} }
      'Interface' { [pscustomobject]@{InstanceID=$id;InterfaceAlias='Any'} }
      'InterfaceType' { [pscustomobject]@{InstanceID=$id;InterfaceType='Any'} }
      'Security' { [pscustomobject]@{InstanceID=$id;Authentication='NotRequired';Encryption='NotRequired';OverrideBlockRules='False'} }
    }
  })
  if($type -eq 'Port' -and $script:DuplicatePortFilter -and $output.Count -gt 0) { $output += $output[0] }
  if($script:ReverseFilters) { [array]::Reverse($output) }
  $output
}
function Get-NetFirewallPortFilter {
  [CmdletBinding()] param([string]$PolicyStore, $AssociatedNetFirewallRule) Read-TestFilters 'Port' $AssociatedNetFirewallRule
}
function Get-NetFirewallAddressFilter {
  [CmdletBinding()] param([string]$PolicyStore, $AssociatedNetFirewallRule) Read-TestFilters 'Address' $AssociatedNetFirewallRule
}
function Get-NetFirewallApplicationFilter {
  [CmdletBinding()] param([string]$PolicyStore, $AssociatedNetFirewallRule) Read-TestFilters 'Application' $AssociatedNetFirewallRule
}
function Get-NetFirewallServiceFilter {
  [CmdletBinding()] param([string]$PolicyStore, $AssociatedNetFirewallRule) Read-TestFilters 'Service' $AssociatedNetFirewallRule
}
function Get-NetFirewallInterfaceFilter {
  [CmdletBinding()] param([string]$PolicyStore, $AssociatedNetFirewallRule) Read-TestFilters 'Interface' $AssociatedNetFirewallRule
}
function Get-NetFirewallInterfaceTypeFilter {
  [CmdletBinding()] param([string]$PolicyStore, $AssociatedNetFirewallRule) Read-TestFilters 'InterfaceType' $AssociatedNetFirewallRule
}
function Get-NetFirewallSecurityFilter {
  [CmdletBinding()] param([string]$PolicyStore, $AssociatedNetFirewallRule) Read-TestFilters 'Security' $AssociatedNetFirewallRule
}
function New-NetFirewallRule {
  [CmdletBinding()] param([string]$Name, [string]$DisplayName, [string]$Direction, [string]$Action,
    [string]$Protocol, [int]$LocalPort, [string]$LocalAddress, [string]$Profile,
    [string]$Enabled, [string]$Group, [string]$Description)
  if ($script:FailCreate -or ($script:FailCreateAt -gt 0 -and $script:Created + 1 -eq $script:FailCreateAt)) { throw 'simulated backend failure with private details' }
  $script:Created++
  $spec=[pscustomobject]@{rule_name=$DisplayName;protocol=$Protocol;local_port=$LocalPort;local_address=$LocalAddress}
  $rule=New-TestRule $spec $Group
  $rule.Name=$Name
  $rule.InstanceID=$Name
  $rule.Description=$Description
  if ($script:CorruptCreate -or $script:Created -eq $script:CorruptCreateAt) { $rule.Enabled='False' }
  if (!$script:ForgetCreate) { $script:Items += $rule }
  $rule
}
function Remove-NetFirewallRule {
  [CmdletBinding()] param([Parameter(ValueFromPipeline=$true)]$InputObject) process {
    if($InputObject.Name -notlike 'LanGame-*') {
      $script:OldRemoves++
      if($script:OldRemoves -eq $script:FailOldRemoveAt) { throw 'simulated middle old-removal failure' }
    }
    if ($script:FailRemove -gt 0) {
      $script:FailRemove--
      if ($script:ChangeCreatedOnRemove) {
        foreach($rule in $script:Items) { if($rule.Name -like 'LanGame-*') { $rule.Description='Operator changed' } }
      }
      throw 'simulated transient old-rule deletion failure'
    }
    if ($script:FailCleanup -and $InputObject.Name -like 'LanGame-*') { throw 'simulated rollback failure' }
    $script:Removed++
    $script:Items = @($script:Items | Where-Object { $_.Name -ne $InputObject.Name })
    if ($script:RemoveThenFail -gt 0) { $script:RemoveThenFail--; throw 'simulated failure after old rule removal' }
  }
}
"#;

fn simulate(setup: &str, apply: bool) -> Value {
    simulate_with_retry(setup, apply, false)
}

fn simulate_with_retry(setup: &str, apply: bool, retry: bool) -> Value {
    simulate_rules(&specs(), setup, apply, retry)
}

fn simulate_rules(
    rules: &[WindowsFirewallRuleSpec],
    setup: &str,
    apply: bool,
    retry: bool,
) -> Value {
    let rules_json = serde_json::to_string(rules).unwrap();
    let script = script(rules, apply).unwrap();
    let retry = if retry {
        "$result = Invoke-TestFirewall".to_owned()
    } else {
        String::new()
    };
    let fixture = format!(
        "{SHIMS}\n$fixtureSpecs = '{rules_json}' | ConvertFrom-Json\n{setup}\nfunction Invoke-TestFirewall {{\n{script}\n}}\n$result = Invoke-TestFirewall\n$first = ($result|ConvertFrom-Json)\n$firstRemaining=$script:Items.Count\n$firstRemoved=$script:Removed\n{retry}\n[pscustomobject]@{{reply=($result|ConvertFrom-Json);first_reply=$first;first_remaining=$firstRemaining;first_removed=$firstRemoved;created=$script:Created;removed=$script:Removed;remaining=$script:Items.Count;filter_reads=$script:FilterReads;bulk_reads=$script:BulkReads;associated_reads=$script:AssociatedReads;rule_reads=$script:RuleReads}} | ConvertTo-Json -Compress -Depth 6"
    );
    let output = system_utility::capture(
        SystemUtility::PowerShell,
        &[
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &fixture,
        ],
        FIREWALL_TIMEOUT,
    )
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        super::super::decode_powershell_text(&output.stderr)
    );
    serde_json::from_str(&super::super::decode_powershell_text(&output.stdout)).unwrap()
}

#[test]
fn firewall_fixed_script_roundtrips_multiple_rules_and_skips_verified_rules() {
    let created = simulate("", true);
    assert_eq!(created["reply"]["state"], "ready");
    assert_eq!(created["reply"]["rules"].as_array().unwrap().len(), 2);
    assert_eq!(created["reply"]["rules"][0]["local_port"], 7778);
    assert_eq!(
        created["reply"]["rules"][1]["local_address"],
        "192.168.31.150"
    );
    assert_eq!(created["created"], 2);
    let existing = simulate(
        "foreach($spec in $fixtureSpecs){$script:Items += New-TestRule $spec 'LanGame Server Manager'}",
        false,
    );
    assert_eq!(existing["reply"]["state"], "ready");
    assert_eq!(existing["created"], 0);
    assert_eq!(existing["removed"], 0);
}

#[test]
fn firewall_fixed_script_checks_foreign_ownership_for_whole_batch_before_mutation() {
    let foreign = simulate(
        "$script:Items += New-TestRule $fixtureSpecs[1] 'Operator owned'",
        true,
    );
    assert_eq!(foreign["reply"]["state"], "rule_conflict");
    assert_eq!(foreign["created"], 0);
    assert_eq!(foreign["removed"], 0);
    assert_eq!(foreign["remaining"], 1);
}

#[test]
fn firewall_fixed_script_retains_prior_rules_on_create_failure_and_requires_readback() {
    let failed = simulate(
        "$rule=New-TestRule $fixtureSpecs[0] 'LanGame Server Manager';$rule.Enabled='False';$script:Items += $rule;$script:FailCreate=$true",
        true,
    );
    assert_eq!(failed["reply"]["state"], "apply_failed");
    assert_eq!(failed["removed"], 0);
    assert_eq!(failed["remaining"], 1);
    assert!(!failed.to_string().contains("private details"));
    let missing = simulate("$script:ForgetCreate=$true", true);
    assert_eq!(missing["reply"]["state"], "verification_failed");
}

#[test]
fn firewall_fixed_script_repairs_owned_stale_scope_and_probe_never_mutates() {
    let setup = "$rule=New-TestRule $fixtureSpecs[0] 'LanGame Server Manager';$rule.Address.LocalAddress='Any';$script:Items += $rule";
    let probe = simulate(setup, false);
    assert_eq!(probe["reply"]["state"], "repair");
    assert_eq!(probe["created"], 0);
    assert_eq!(probe["removed"], 0);
    let repaired = simulate(setup, true);
    assert_eq!(repaired["reply"]["state"], "ready");
    assert_eq!(repaired["created"], 2);
    assert_eq!(repaired["removed"], 1);
    assert_eq!(repaired["reply"]["rules"][0]["status"], "replaced");
}

#[test]
fn firewall_readonly_probe_rejects_restricted_remote_scope() {
    let prefix = "foreach($spec in $fixtureSpecs){$script:Items += New-TestRule $spec 'LanGame Server Manager'}";
    let restricted = simulate(
        &format!("{prefix};$script:Items[0].Address.RemoteAddress='192.168.31.1'"),
        false,
    );
    assert_eq!(restricted["reply"]["state"], "repair");
    assert_eq!(restricted["created"], 0);
}

#[test]
fn firewall_failed_old_removal_rolls_back_only_new_rule_and_retry_succeeds() {
    let retried = simulate_with_retry(
        "$rule=New-TestRule $fixtureSpecs[0] 'LanGame Server Manager';$rule.Enabled='False';$script:Items += $rule;$script:FailRemove=1",
        true,
        true,
    );
    assert_eq!(retried["first_reply"]["state"], "apply_failed");
    assert_eq!(
        retried["first_remaining"], 1,
        "prior rule must remain without a duplicate"
    );
    assert_eq!(
        retried["first_removed"], 2,
        "both of this batch's uncommitted additions were rolled back"
    );
    assert_eq!(retried["reply"]["state"], "ready");
    assert_eq!(retried["remaining"], 2);
    let uncertain = simulate_with_retry(
        "$rule=New-TestRule $fixtureSpecs[0] 'LanGame Server Manager';$rule.Enabled='False';$script:Items += $rule;$script:RemoveThenFail=1",
        true,
        true,
    );
    assert_eq!(uncertain["first_reply"]["state"], "apply_failed");
    assert_eq!(
        uncertain["first_remaining"], 1,
        "validated replacement remains when the old rule actually disappeared"
    );
    assert_eq!(uncertain["first_removed"], 2);
    assert_eq!(uncertain["reply"]["state"], "ready");
    assert_eq!(uncertain["remaining"], 2);
}

#[test]
fn firewall_incomplete_new_validation_rolls_back_exact_operation_identity() {
    let failed = simulate(
        "$rule=New-TestRule $fixtureSpecs[0] 'LanGame Server Manager';$rule.Enabled='False';$script:Items += $rule;$script:CorruptCreate=$true",
        true,
    );
    assert_eq!(failed["reply"]["state"], "verification_failed");
    assert_eq!(failed["remaining"], 1);
    assert_eq!(failed["removed"], 2);
    let not_created = simulate(
        "$rule=New-TestRule $fixtureSpecs[0] 'LanGame Server Manager';$rule.Enabled='False';$script:Items += $rule;$script:ForgetCreate=$true",
        true,
    );
    assert_eq!(not_created["reply"]["state"], "verification_failed");
    assert_eq!(
        not_created["remaining"], 1,
        "old rule is not retired until new identity is read back"
    );
    assert_eq!(not_created["removed"], 0);
}

#[test]
fn firewall_rollback_failure_or_changed_identity_is_explicit_and_preserves_rules() {
    for fault in [
        "$script:FailCleanup=$true",
        "$script:ChangeCreatedOnRemove=$true",
    ] {
        let failed = simulate(
            &format!(
                "$rule=New-TestRule $fixtureSpecs[0] 'LanGame Server Manager';$rule.Enabled='False';$script:Items += $rule;$script:FailRemove=1;{fault}"
            ),
            true,
        );
        assert_eq!(failed["reply"]["state"], "cleanup_failed");
        assert_eq!(
            failed["remaining"], 3,
            "uncertain or changed rules must not be deleted"
        );
        assert_eq!(failed["removed"], 0);
    }
    let specs = specs();
    assert_eq!(
        reason(
            apply_with(
                &specs,
                |_| Ok(reply(&specs, "cleanup_failed")),
                || panic!("do not retry cleanup"),
                |_| panic!("do not elevate cleanup")
            )
            .unwrap_err()
        ),
        "cleanup_failed"
    );
}

#[path = "firewall_snapshot_tests.rs"]
mod snapshot_tests;
