use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use app_core::InstanceDetails;
use app_platform_win::{WindowsFirewallRuleSpec, build_instance_firewall_rule_specs};

/// Only the explicitly acknowledged SCUM fixture may run elevated. Its unique
/// instance rules are checked absent before launch and removed after shutdown.
pub(in crate::commands::tests) fn elevated_fixture_allowed(
    module_id: &str,
    elevated: bool,
    acknowledgement: Option<&str>,
) -> Result<bool, &'static str> {
    if !elevated {
        return Ok(false);
    }
    if module_id == "scum" && acknowledgement == Some("true") {
        return Ok(true);
    }
    Err("elevated native fixture requires explicit SCUM acknowledgement")
}

pub(in crate::commands::tests) struct NativeFirewall {
    specs: Vec<WindowsFirewallRuleSpec>,
    finished: bool,
}

impl NativeFirewall {
    pub(in crate::commands::tests) fn prepare(
        instance: &InstanceDetails,
        root: &Path,
    ) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|error| error.to_string())?;
        let config = Path::new(&instance.config_file_path)
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if instance.summary.module_id != "scum" || !config.starts_with(&root) {
            return Err("elevated firewall fixture must own a SCUM instance".into());
        }
        let specs = build_instance_firewall_rule_specs(
            &instance.summary.id,
            &instance.summary.name,
            &instance.ports,
            None,
        );
        if specs.is_empty() || specs.len() > 8 {
            return Err("elevated fixture requires bounded declared firewall rules".into());
        }
        run(&script(&specs, Action::CheckAbsent)?)?;
        Ok(Self {
            specs,
            finished: false,
        })
    }

    pub(in crate::commands::tests) fn prepare_dst_loopback(
        instance: &InstanceDetails,
        run_root: &Path,
    ) -> Result<Self, String> {
        let specs = dst_loopback_rule_specs(instance, run_root)?;
        run(&script(&specs, Action::CheckAbsent)?)?;
        Ok(Self {
            specs,
            finished: false,
        })
    }

    pub(in crate::commands::tests) fn finish(&mut self) -> Result<(), String> {
        run(&script(&self.specs, Action::RemoveOwned)?)?;
        self.finished = true;
        Ok(())
    }
}

fn dst_loopback_rule_specs(
    instance: &InstanceDetails,
    run_root: &Path,
) -> Result<Vec<WindowsFirewallRuleSpec>, String> {
    let id = instance.summary.id.as_str();
    let valid_id = id.rsplit_once('-').is_some_and(|(slug, suffix)| {
        !slug.is_empty()
            && slug.len() <= 32
            && slug
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            && slug.split('-').all(|segment| !segment.is_empty())
            && suffix.len() == 16
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    if !valid_id
        || instance.summary.module_id != "dontstarve"
        || instance.summary.bind_ip != "127.0.0.1"
        || instance.summary.autostart
        || instance.ports.is_empty()
        || instance.ports.len() > 7
    {
        return Err("DST firewall acceptance requires one bounded loopback instance".into());
    }
    let run_root = ordinary_path(run_root)?;
    let run_id = run_root
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("ai-"))
        .filter(|id| {
            id.len() == 32
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .and_then(|id| uuid::Uuid::parse_str(id).ok());
    if !run_root.is_dir() || run_id.is_none_or(|id| id.get_version() != Some(uuid::Version::Random))
    {
        return Err("DST firewall acceptance requires its unique ai-UUID run directory".into());
    }
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if run_root.starts_with(&workspace) || workspace.starts_with(&run_root) {
        return Err("DST firewall acceptance must remain outside the repository".into());
    }
    if let Ok(persistent) = Path::new(app_core::DEFAULT_LANGAME_SERVER_FILES_ROOT).canonicalize()
        && (run_root.starts_with(&persistent) || persistent.starts_with(&run_root))
    {
        return Err("DST firewall acceptance cannot own persistent server files".into());
    }
    let instance_root = run_root.join("i").join(id);
    let expected_config = ordinary_path(&instance_root.join("config/instance.json"))?;
    let expected_saves = ordinary_path(&instance_root.join("config/clusters/main"))?;
    if ordinary_path(Path::new(&instance.config_file_path))? != expected_config
        || ordinary_path(Path::new(&instance.saves_path))? != expected_saves
        || !expected_config.is_file()
        || !expected_saves.is_dir()
    {
        return Err("DST firewall acceptance must own its exact config and saves paths".into());
    }
    let settings: serde_json::Value = serde_json::from_str(&instance.settings_json)
        .map_err(|error| format!("DST firewall acceptance settings are invalid: {error}"))?;
    if settings["bind_ip"] != "127.0.0.1"
        || settings["offline_cluster"] != true
        || settings["lan_only_cluster"] != true
        || settings["cluster_token"] != ""
        || settings["enable_caves"] != false
    {
        return Err(
            "DST firewall acceptance requires an offline single-world loopback configuration"
                .into(),
        );
    }
    let allowed_names = [
        "master",
        "caves",
        "shard_master",
        "steam_query",
        "steam_auth",
        "caves_steam_query",
        "caves_steam_auth",
    ];
    let mut names = HashSet::new();
    let mut numbers = HashSet::new();
    if instance.ports.iter().any(|port| {
        !allowed_names.contains(&port.name.as_str())
            || !port.protocol.eq_ignore_ascii_case("udp")
            || port.port == 0
            || !names.insert(port.name.as_str())
            || !numbers.insert(port.port)
    }) {
        return Err("DST firewall acceptance requires distinct declared nonzero UDP ports".into());
    }
    let specs = build_instance_firewall_rule_specs(
        id,
        &instance.summary.name,
        &instance.ports,
        Some("127.0.0.1"),
    );
    if specs.len() != instance.ports.len()
        || specs
            .iter()
            .any(|spec| spec.local_address != "127.0.0.1" || spec.protocol != "UDP")
    {
        return Err("DST firewall acceptance produced an unexpected rule scope".into());
    }
    Ok(specs)
}

fn ordinary_path(path: &Path) -> Result<PathBuf, String> {
    use std::os::windows::fs::MetadataExt;
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err("Native firewall ownership requires an absolute normalized path".into());
    }
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0 {
            return Err("Native firewall ownership cannot cross reparse points".into());
        }
    }
    path.canonicalize().map_err(|error| error.to_string())
}

impl Drop for NativeFirewall {
    fn drop(&mut self) {
        if !self.finished
            && let Err(error) = self.finish()
        {
            eprintln!("NATIVE_LIFECYCLE cleanup=firewall_failed detail={error}");
        }
    }
}

#[derive(Clone, Copy)]
enum Action {
    CheckAbsent,
    RemoveOwned,
}

fn script(specs: &[WindowsFirewallRuleSpec], action: Action) -> Result<String, String> {
    let json = serde_json::to_string(specs).map_err(|error| error.to_string())?;
    let remove = matches!(action, Action::RemoveOwned);
    Ok(format!(
        r#"
$ErrorActionPreference = 'Stop'
$rules = ConvertFrom-Json @'
{json}
'@
$removeOwned = ${remove}
$owned = @()
foreach ($spec in $rules) {{
  $rulesFound = @(Get-NetFirewallRule -ErrorAction Stop | Where-Object {{ $_.DisplayName -ieq $spec.rule_name }})
  if (!$removeOwned) {{
    if ($rulesFound.Count -ne 0) {{ throw 'Native fixture firewall name already exists' }}
    continue
  }}
  foreach ($rule in $rulesFound) {{
    $ports = @($rule | Get-NetFirewallPortFilter -ErrorAction Stop)
    $addresses = @($rule | Get-NetFirewallAddressFilter -ErrorAction Stop)
    $protocol = if ($spec.protocol -eq 'UDP') {{ '17' }} else {{ '6' }}
    if ($rule.DisplayName -cne $spec.rule_name -or $rule.Group -cne 'LanGame Server Manager' -or [string]$rule.Direction -ne 'Inbound' -or [string]$rule.Action -ne 'Allow' -or
        $ports.Count -ne 1 -or [string]$ports[0].LocalPort -ne [string]$spec.local_port -or
        ([string]$ports[0].Protocol -ne $spec.protocol -and [string]$ports[0].Protocol -ne $protocol) -or
        $addresses.Count -ne 1 -or [string]$addresses[0].LocalAddress -ne $spec.local_address) {{
      throw 'Native fixture firewall rule changed; refusing cleanup'
    }}
    $owned += $rule
  }}
}}
# Validate the entire bounded set before removing any rule. A changed rule must
# leave the other owned rules available for explicit investigation and retry.
foreach ($rule in $owned) {{
  $rule | Remove-NetFirewallRule -ErrorAction Stop
}}
if ($removeOwned) {{
  foreach ($spec in $rules) {{
    if (@(Get-NetFirewallRule -ErrorAction Stop | Where-Object {{ $_.DisplayName -ieq $spec.rule_name }}).Count -ne 0) {{
    throw 'Native fixture firewall cleanup did not remove its exact rule'
    }}
  }}
}}
"#
    ))
}

fn run(script: &str) -> Result<(), String> {
    let powershell = PathBuf::from(std::env::var_os("SystemRoot").ok_or("missing SystemRoot")?)
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let output = app_runtime::capture_windows_utility(
        &powershell,
        &[
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ],
        Duration::from_secs(30),
    )
    .map_err(|error| format!("native firewall utility failed: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "native firewall utility failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "commands_native_firewall_tests.rs"]
mod tests;
