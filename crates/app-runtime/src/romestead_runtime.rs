use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use app_core::AppSettings;
use serde_json::Value;

/// The framework-dependent apphost must use the configured tools location,
/// which does not move when an instance receives a private runtime copy.
pub(super) fn apply_portable_runtime(
    settings: &AppSettings,
    module_id: &str,
    install_root: &Path,
    environment: &mut BTreeMap<String, String>,
) {
    if module_id != "romestead" {
        return;
    }
    let Some(required) = required_runtime(install_root) else {
        return;
    };
    let Some(tools_root) = Path::new(&settings.steamcmd_root).parent() else {
        return;
    };
    let runtime_root = tools_root.join("dotnet");
    if !runtime_root.is_absolute()
        || !runtime_root.join("dotnet.exe").is_file()
        || !contains_runtime(
            &runtime_root.join("host").join("fxr"),
            RuntimeRequirement {
                version: [8, 0, 0],
                exact_patch: false,
            },
            &["hostfxr.dll"],
        )
        || !contains_runtime(
            &runtime_root.join("shared").join("Microsoft.NETCore.App"),
            required,
            &[
                "coreclr.dll",
                "hostpolicy.dll",
                "System.Private.CoreLib.dll",
            ],
        )
    {
        // Missing/incompatible portable installations must not hide a usable
        // globally installed runtime or the apphost's original diagnostics.
        return;
    }
    environment.retain(|key, _| {
        !key.eq_ignore_ascii_case("DOTNET_ROOT") && !key.eq_ignore_ascii_case("DOTNET_ROOT_X64")
    });
    let value = runtime_root.to_string_lossy().into_owned();
    environment.insert(String::from("DOTNET_ROOT_X64"), value.clone());
    environment.insert(String::from("DOTNET_ROOT"), value);
}

#[derive(Clone, Copy)]
struct RuntimeRequirement {
    version: [u32; 3],
    exact_patch: bool,
}

fn required_runtime(install_root: &Path) -> Option<RuntimeRequirement> {
    let file = File::open(install_root.join("Server.runtimeconfig.json")).ok()?;
    let mut bytes = Vec::new();
    file.take(65_537).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 65_536 {
        return None;
    }
    let config: Value = serde_json::from_slice(&bytes).ok()?;
    let options = config.get("runtimeOptions")?;
    let frameworks = options.get("frameworks").and_then(Value::as_array);
    let framework = match frameworks {
        Some(frameworks) if frameworks.len() == 1 => &frameworks[0],
        Some(_) => return None,
        None => options.get("framework")?,
    };
    if framework.get("name")?.as_str()? != "Microsoft.NETCore.App" {
        return None;
    }
    let version = stable_version(framework.get("version")?.as_str()?)?;
    // The inspected Romestead apphost requires .NET 8. Unknown future runtime
    // requirements stay with the system resolver instead of forcing a rollout.
    if version[0] != 8 {
        return None;
    }
    Some(RuntimeRequirement {
        version,
        exact_patch: options.get("rollForward").and_then(Value::as_str) == Some("Disable")
            || options.get("applyPatches").and_then(Value::as_bool) == Some(false),
    })
}

fn stable_version(value: &str) -> Option<[u32; 3]> {
    let mut parts = value.split('.');
    let version = [
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ];
    parts.next().is_none().then_some(version)
}

fn contains_runtime(root: &Path, required: RuntimeRequirement, files: &[&str]) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    entries.take(128).filter_map(Result::ok).any(|entry| {
        let Some(version) = entry.file_name().to_str().and_then(stable_version) else {
            return false;
        };
        version[..2] == required.version[..2]
            && if required.exact_patch {
                version[2] == required.version[2]
            } else {
                version[2] >= required.version[2]
            }
            && files.iter().all(|file| entry.path().join(file).is_file())
    })
}

#[cfg(test)]
#[path = "romestead_runtime_tests.rs"]
mod tests;
