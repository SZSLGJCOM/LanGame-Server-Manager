//! Instance-scoped firewall setup. The ordinary service never gains an elevated token.
//! Each independent read uses one rule query and seven complete filter views,
//! indexed by exact CIM identity. A refused bulk read uses bounded associated
//! rule queries with the same ordinary token. No snapshot survives a mutation
//! phase or substitutes for the original service's final readback.
use std::collections::HashSet;
use std::net::IpAddr;

use serde::Deserialize;
use serde_json::{Value, json};

use super::system_utility::{self, FIREWALL_TIMEOUT, SystemUtility};
use super::{WindowsFirewallRuleApplyResult, WindowsFirewallRuleSpec};

const MAX_RULES: usize = 128;
const MAX_PACKET_BYTES: usize = 65_536;
// Reserve command-line room for the fixed system executable and flags. The
// IPC frame limit alone does not bound CreateProcessW's UTF-16 command line.
const MAX_SCRIPT_ARGUMENT_UNITS: usize = 30_000;
const SCRIPT: &str = include_str!("firewall.ps1");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    rule_name: String,
    protocol: String,
    local_port: u16,
    local_address: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    state: String,
    #[serde(default)]
    rules: Vec<WindowsFirewallRuleApplyResult>,
}

fn validate(specs: &[WindowsFirewallRuleSpec]) -> Result<(), &'static str> {
    if specs.is_empty() || specs.len() > MAX_RULES {
        return Err("invalid_rules");
    }
    let mut names = HashSet::new();
    for spec in specs {
        if !spec.rule_name.starts_with("LanGame ")
            || spec.rule_name.len() > 160
            || !spec.rule_name.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'-' | b'_' | b'.')
            })
            || !names.insert(spec.rule_name.to_ascii_lowercase())
            || !matches!(spec.protocol.as_str(), "UDP" | "TCP")
            || spec.local_port == 0
            || (spec.local_address != "Any" && spec.local_address.parse::<IpAddr>().is_err())
        {
            return Err("invalid_rules");
        }
    }
    if serde_json::to_vec(specs)
        .map_err(|_| "invalid_rules")?
        .len()
        > MAX_PACKET_BYTES
    {
        return Err("invalid_rules");
    }
    Ok(())
}

fn parse_rules(value: &Value) -> Result<Vec<WindowsFirewallRuleSpec>, &'static str> {
    let rules: Vec<Rule> = serde_json::from_value(value.clone()).map_err(|_| "invalid_rules")?;
    let specs = rules
        .into_iter()
        .map(|rule| WindowsFirewallRuleSpec {
            rule_name: rule.rule_name,
            protocol: rule.protocol,
            local_port: rule.local_port,
            local_address: rule.local_address,
        })
        .collect::<Vec<_>>();
    validate(&specs)?;
    Ok(specs)
}

pub(super) fn script(specs: &[WindowsFirewallRuleSpec], apply: bool) -> Result<String, String> {
    validate(specs).map_err(str::to_owned)?;
    let rules_json = serde_json::to_string(specs).map_err(|_| "invalid_rules")?;
    // Every string above is restricted to non-script data. The helper chooses
    // this embedded script; IPC cannot choose executable, script or file paths.
    let script = format!("$apply = ${apply}\n$rulesJson = @'\n{rules_json}\n'@\n{SCRIPT}");
    check_script_argument(&script).map_err(str::to_owned)?;
    Ok(script)
}

fn check_script_argument(script: &str) -> Result<(), &'static str> {
    // Quoting adds two surrounding quotes and doubles backslashes preceding
    // quotes or the end. Counting every backslash twice is a conservative bound.
    let escaped_units = script.encode_utf16().count()
        + script
            .bytes()
            .filter(|byte| matches!(*byte, b'"' | b'\\'))
            .count()
        + 2;
    if escaped_units > MAX_SCRIPT_ARGUMENT_UNITS {
        Err("invalid_rules")
    } else {
        Ok(())
    }
}

fn execute(specs: &[WindowsFirewallRuleSpec], apply: bool) -> Result<Reply, String> {
    let script = script(specs, apply)?;
    let output = system_utility::capture(
        SystemUtility::PowerShell,
        &[
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &script,
        ],
        FIREWALL_TIMEOUT,
    )
    .map_err(|error| {
        if error.kind() == std::io::ErrorKind::TimedOut {
            "timeout"
        } else {
            "apply_failed"
        }
        .to_owned()
    })?;
    if !output.status.success() {
        return Err("apply_failed".into());
    }
    // Do not forward PowerShell's error text: it can include scripts and paths.
    serde_json::from_str(&super::decode_powershell_text(&output.stdout))
        .map_err(|_| "verification_failed".into())
}

fn ready(
    reply: Reply,
    specs: &[WindowsFirewallRuleSpec],
) -> Result<Vec<WindowsFirewallRuleApplyResult>, String> {
    if reply.state != "ready" {
        return Err(match reply.state.as_str() {
            "rule_conflict" | "permission_denied" | "apply_failed" | "timeout"
            | "cleanup_failed" => reply.state,
            _ => "verification_failed".into(),
        });
    }
    if reply.rules.len() != specs.len()
        || reply.rules.iter().zip(specs).any(|(result, spec)| {
            result.rule_name != spec.rule_name
                || result.protocol != spec.protocol
                || result.local_port != spec.local_port
                || result.local_address != spec.local_address
                || !matches!(result.status.as_str(), "existing" | "created" | "replaced")
                || result.message.is_some()
        })
    {
        return Err("verification_failed".into());
    }
    Ok(reply.rules)
}

pub(super) fn apply(
    specs: &[WindowsFirewallRuleSpec],
) -> Result<Vec<WindowsFirewallRuleApplyResult>, String> {
    apply_with(
        specs,
        |apply| execute(specs, apply),
        || {
            super::WindowsPlatform::current_process_is_elevated()
                .map_err(|_| "permission_denied".into())
        },
        app_runtime::request_elevated_firewall,
    )
}

fn apply_with(
    specs: &[WindowsFirewallRuleSpec],
    mut execute: impl FnMut(bool) -> Result<Reply, String>,
    is_elevated: impl FnOnce() -> Result<bool, String>,
    elevate: impl FnOnce(&Value) -> std::io::Result<Value>,
) -> Result<Vec<WindowsFirewallRuleApplyResult>, String> {
    if specs.is_empty() {
        return Ok(Vec::new());
    }
    validate(specs).map_err(diagnostic)?;
    let probe = execute(false).map_err(diagnostic)?;
    match probe.state.as_str() {
        "ready" => return ready(probe, specs).map_err(diagnostic),
        "repair" => {}
        code => return Err(diagnostic(code)),
    }
    let reply = if is_elevated().map_err(diagnostic)? {
        execute(true).map_err(diagnostic)?
    } else {
        let rules = serde_json::to_value(specs).map_err(|_| diagnostic("invalid_rules"))?;
        let reply = elevate(&rules).map_err(|error| {
            let code = match error.raw_os_error() {
                Some(1223) => "elevation_cancelled",
                Some(5) => "permission_denied",
                _ if error.kind() == std::io::ErrorKind::TimedOut => "timeout",
                _ => match error.to_string().as_str() {
                    "invalid_rules" => "invalid_rules",
                    "rule_conflict" => "rule_conflict",
                    "permission_denied" => "permission_denied",
                    "verification_failed" => "verification_failed",
                    "cleanup_failed" => "cleanup_failed",
                    "timeout" => "timeout",
                    _ => "apply_failed",
                },
            };
            diagnostic(code)
        })?;
        serde_json::from_value(reply).map_err(|_| diagnostic("verification_failed"))?
    };
    ready(reply, specs).map_err(diagnostic)?;
    // A successful helper receipt alone is insufficient. Verify again from the
    // original service after the elevated process has exited.
    ready(execute(false).map_err(diagnostic)?, specs).map_err(diagnostic)
}

/// Called before desktop/service startup. The only elevated operation registered
/// here is the embedded, validated firewall reconciliation above.
pub fn run_elevated_firewall_if_requested() -> Option<i32> {
    app_runtime::run_elevated_firewall_if_requested(|value| {
        let specs = parse_rules(value).map_err(str::to_owned)?;
        let reply = execute(&specs, true)?;
        let results = ready(reply, &specs)?;
        Ok(json!({"state": "ready", "rules": results}))
    })
}

fn diagnostic(reason: impl AsRef<str>) -> String {
    let reason = match reason.as_ref() {
        "invalid_rules" => "invalid_rules",
        "rule_conflict" => "rule_conflict",
        "permission_denied" => "permission_denied",
        "elevation_cancelled" => "elevation_cancelled",
        "verification_failed" => "verification_failed",
        "cleanup_failed" => "cleanup_failed",
        "timeout" => "timeout",
        _ => "apply_failed",
    };
    let detail = match reason {
        "elevation_cancelled" => "The Windows administrator prompt was cancelled.",
        "permission_denied" => "Windows denied access to the instance firewall rules.",
        "rule_conflict" => {
            "A rule-name conflict prevents safe identification of one LanGame-owned rule; conflicting rules were not changed."
        }
        "invalid_rules" => {
            "The requested instance firewall rules are invalid or exceed their limit."
        }
        "verification_failed" => {
            "The configured rules did not pass readback verification; some rules may have changed."
        }
        "cleanup_failed" => {
            "A rule created by this attempt could not be safely rolled back. Inspect the instance firewall rules before retrying; some rules may have changed."
        }
        "timeout" => {
            "The firewall operation or its cleanup timed out; some rules may have changed."
        }
        _ => "The firewall operation was not confirmed; some rules may have changed.",
    };
    json!({"code":"windows_firewall_setup_failed", "reason":reason,
        "message":format!("{detail} External connections may be blocked. Approve the Windows administrator prompt or inspect this instance's inbound rules.")}).to_string()
}

#[cfg(test)]
#[path = "firewall_tests.rs"]
mod tests;
