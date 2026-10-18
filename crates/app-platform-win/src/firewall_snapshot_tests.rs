//! Contract checks for bulk CIM identities and multi-rule recovery, without host writes.
use super::*;

fn twelve_specs() -> Vec<WindowsFirewallRuleSpec> {
    (0..12)
        .map(|index| WindowsFirewallRuleSpec {
            rule_name: format!("LanGame cluster map{index} UDP {}", 7778 + index),
            protocol: "UDP".into(),
            local_port: 7778 + index,
            local_address: "Any".into(),
        })
        .collect()
}

fn assert_filter_reads(result: &Value, expected: u64) {
    let counts = result["filter_reads"].as_object().unwrap();
    assert_eq!(counts.len(), 7);
    for count in counts.values() {
        assert_eq!(count.as_u64(), Some(expected));
    }
}

#[test]
fn firewall_twelve_existing_rules_use_one_complete_bulk_snapshot() {
    let result = simulate_rules(
        &twelve_specs(),
        "foreach($spec in $fixtureSpecs){$script:Items += New-TestRule $spec 'LanGame Server Manager'};$script:ReverseFilters=$true",
        false,
        false,
    );
    assert_eq!(result["reply"]["state"], "ready");
    assert_eq!(result["reply"]["rules"].as_array().unwrap().len(), 12);
    assert_eq!(result["rule_reads"], 1);
    assert_eq!(result["created"], 0);
    assert_eq!(result["removed"], 0);
    assert_filter_reads(&result, 1);
}

#[test]
fn firewall_ordinary_token_uses_associated_reads_only_after_explicit_bulk_permission_refusal() {
    for exception in [false, true] {
        let result = simulate_rules(
            &twelve_specs(),
            &format!(
                "foreach($spec in $fixtureSpecs){{$script:Items += New-TestRule $spec 'LanGame Server Manager'}};$script:DeniedBulkTypes=@('Port','Address','Interface','InterfaceType');$script:DeniedBulkException=${exception}"
            ),
            false,
            false,
        );
        assert_eq!(result["reply"]["state"], "ready");
        assert_eq!(result["reply"]["rules"].as_array().unwrap().len(), 12);
        assert_eq!(result["rule_reads"], 1);
        assert_eq!(result["created"], 0);
        assert_eq!(result["removed"], 0);
        assert_eq!(result["bulk_reads"].as_object().unwrap().len(), 7);
        for count in result["bulk_reads"].as_object().unwrap().values() {
            assert_eq!(count, 1, "each class tries bulk exactly once");
        }
        let associated = result["associated_reads"].as_object().unwrap();
        assert_eq!(associated.len(), 4);
        for kind in ["Port", "Address", "Interface", "InterfaceType"] {
            assert_eq!(associated[kind], 12, "one single-rule read per target");
        }
    }
}

#[test]
fn firewall_unknown_bulk_errors_never_fall_back_to_association_or_mutation() {
    let result = simulate(
        "foreach($spec in $fixtureSpecs){$script:Items += New-TestRule $spec 'LanGame Server Manager'};$script:FailedBulkType='Port'",
        true,
    );
    assert_eq!(result["reply"]["state"], "apply_failed");
    assert_eq!(result["bulk_reads"]["Port"], 1);
    assert!(result["associated_reads"].as_object().unwrap().is_empty());
    assert_eq!(result["created"], 0);
    assert_eq!(result["removed"], 0);
    assert!(!result["reply"].to_string().contains("private details"));
}

#[test]
fn firewall_associated_failure_and_incomplete_identity_never_confirm_ready() {
    for (fault, expected) in [
        ("$script:DeniedAssociatedType='Port'", "permission_denied"),
        ("$script:FailedAssociatedType='Port'", "apply_failed"),
        ("$script:OmitPortFilter=$true", "repair"),
        ("$script:DuplicatePortFilter=$true", "verification_failed"),
        ("$script:WrongPortFilterId=$true", "repair"),
        ("$script:Items[0].Port.RemotePort='443'", "repair"),
    ] {
        let result = simulate(
            &format!(
                "foreach($spec in $fixtureSpecs){{$script:Items += New-TestRule $spec 'LanGame Server Manager'}};$script:DeniedBulkTypes=@('Port');{fault}"
            ),
            false,
        );
        assert_eq!(result["reply"]["state"], expected);
        assert_eq!(result["bulk_reads"]["Port"], 1);
        assert!(result["associated_reads"]["Port"].as_u64().unwrap() > 0);
        assert_eq!(result["created"], 0);
        assert_eq!(result["removed"], 0);
        assert!(!result["reply"].to_string().contains("private details"));
    }
}

#[test]
fn firewall_twelve_new_rules_need_two_complete_bulk_snapshots_independent_of_batch_size() {
    let result = simulate_rules(&twelve_specs(), "", true, false);
    assert_eq!(result["reply"]["state"], "ready");
    assert_eq!(result["created"], 12);
    assert_eq!(result["remaining"], 12);
    assert_eq!(result["rule_reads"], 5);
    assert_filter_reads(&result, 2);
}

#[test]
fn firewall_bulk_lookup_rejects_missing_duplicate_and_unrelated_filter_ids() {
    let existing = "foreach($spec in $fixtureSpecs){$script:Items += New-TestRule $spec 'LanGame Server Manager'};";
    for (fault, expected) in [
        ("$script:OmitPortFilter=$true", "repair"),
        ("$script:DuplicatePortFilter=$true", "verification_failed"),
        ("$script:WrongPortFilterId=$true", "repair"),
        (
            "$script:Items[0].InstanceID='not-the-rule-name'",
            "verification_failed",
        ),
    ] {
        let result = simulate(&format!("{existing}{fault}"), false);
        assert_eq!(result["reply"]["state"], expected);
        assert_eq!(result["created"], 0);
        assert_eq!(result["removed"], 0);
    }
}

#[test]
fn firewall_refreshes_batch_ownership_before_creation_and_preserves_old_rules_on_middle_failure() {
    let conflict = simulate("$script:ConflictBeforeCreation=$true", true);
    assert_eq!(conflict["reply"]["state"], "rule_conflict");
    assert_eq!(conflict["created"], 0);
    assert_eq!(conflict["removed"], 0);
    for fault in ["$script:FailCreateAt=2", "$script:CorruptCreateAt=2"] {
        let result = simulate(
            &format!(
                "foreach($spec in $fixtureSpecs){{$rule=New-TestRule $spec 'LanGame Server Manager';$rule.Enabled='False';$script:Items += $rule}};{fault}"
            ),
            true,
        );
        assert_ne!(result["reply"]["state"], "ready");
        assert_eq!(
            result["remaining"], 2,
            "every old rule remains until all new rules validate"
        );
        assert_eq!(
            result["removed"], result["created"],
            "only this batch's additions roll back"
        );
    }
}

#[test]
fn firewall_partial_old_retirement_preserves_committed_replacement_and_retry_recovers() {
    let result = simulate_with_retry(
        "foreach($spec in $fixtureSpecs){$rule=New-TestRule $spec 'LanGame Server Manager';$rule.Enabled='False';$script:Items += $rule};$script:FailOldRemoveAt=2",
        true,
        true,
    );
    assert_eq!(result["first_reply"]["state"], "apply_failed");
    assert_eq!(
        result["first_remaining"], 2,
        "first replacement and second old rule remain, without duplicates"
    );
    assert_eq!(
        result["first_removed"], 2,
        "first old rule and second uncommitted addition only"
    );
    assert_eq!(result["reply"]["state"], "ready");
    assert_eq!(result["remaining"], 2);
}
