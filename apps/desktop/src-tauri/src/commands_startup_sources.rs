use app_core::startup_readiness::StartupLogSource;
use app_core::{InstanceDetails, ProcessLaunchPlan};
use std::path::{Component, Path, PathBuf};

pub(super) fn log_path(
    source: &StartupLogSource,
    instance: &InstanceDetails,
    plan: &ProcessLaunchPlan,
) -> Result<(PathBuf, bool), String> {
    let native = match source {
        StartupLogSource::Managed => return Ok((PathBuf::from(&plan.log_path), true)),
        StartupLogSource::LaunchArgument {
            name,
            fallback_to_managed,
        } => match argument_value(&plan.launch_plan.args, name)? {
            None if *fallback_to_managed => return Ok((PathBuf::from(&plan.log_path), true)),
            Some(value) if value == "-" && *fallback_to_managed => {
                return Ok((PathBuf::from(&plan.log_path), true));
            }
            Some(value) if !value.is_empty() => {
                let path = PathBuf::from(value.trim_matches('"'));
                if path.is_absolute() {
                    path
                } else {
                    Path::new(&plan.launch_plan.working_directory).join(path)
                }
            }
            _ => return Err(format!("Startup log argument `{name}` is missing")),
        },
        StartupLogSource::CorekeeperNative => {
            app_core::runtime_logs::corekeeper_native_log_path(Path::new(&plan.log_path))
                .ok_or("Cannot resolve this launch's Core Keeper native log")?
        }
        StartupLogSource::Instance { relative_path } => {
            let root = Path::new(&instance.config_file_path)
                .parent()
                .and_then(Path::parent)
                .ok_or("Instance configuration has no root")?;
            confined_path(root, relative_path)?
        }
        StartupLogSource::Install { relative_path } => {
            confined_path(Path::new(&plan.launch_plan.install_root), relative_path)?
        }
    };
    if !native.is_absolute() {
        return Err("Startup logs require an absolute resolved path".into());
    }
    Ok((native, false))
}

fn confined_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    if relative.is_empty()
        || !Path::new(relative)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err("Startup log path must stay within its root".into());
    }
    let path = root.join(relative);
    if path.exists() {
        let root = root.canonicalize().map_err(|error| error.to_string())?;
        if !path
            .canonicalize()
            .map_err(|error| error.to_string())?
            .starts_with(root)
        {
            return Err("Startup log path escaped its root".into());
        }
    }
    Ok(path)
}

/// UE URL parameters, `-key=value`, and separate switch/value arguments cover
/// the actual launchers. Duplicate definitions are ambiguous and are rejected.
pub(super) fn argument_value(args: &[String], name: &str) -> Result<Option<String>, String> {
    let mut values = Vec::new();
    for (index, argument) in args.iter().enumerate() {
        if argument.eq_ignore_ascii_case(name) {
            let value = args
                .get(index + 1)
                .ok_or_else(|| format!("Missing value for `{name}`"))?;
            values.push(value.clone());
        } else if let Some((key, value)) = argument.split_once('=')
            && key.eq_ignore_ascii_case(name)
        {
            values.push(value.into());
        }
        for parameter in argument.split('?').skip(1) {
            if let Some((key, value)) = parameter.split_once('=')
                && key.eq_ignore_ascii_case(name)
            {
                values.push(value.into());
            }
        }
    }
    if values.len() > 1 {
        return Err(format!("Startup evidence has duplicate `{name}` arguments"));
    }
    Ok(values.pop())
}

pub(super) fn query_port(
    instance: &InstanceDetails,
    plan: &ProcessLaunchPlan,
    name: &str,
    argument: Option<&str>,
    multiple_processes: bool,
) -> Result<u16, String> {
    if let Some(argument) = argument
        && let Some(value) = argument_value(&plan.launch_plan.args, argument)?
    {
        return value
            .parse::<u16>()
            .ok()
            .filter(|port| *port > 0)
            .ok_or_else(|| format!("Invalid startup query port for `{}`", plan.process_key));
    }
    if multiple_processes {
        return Err(format!(
            "Startup query port must be resolved for each process: {}",
            plan.process_key
        ));
    }
    instance
        .ports
        .iter()
        .find(|port| port.name == name && port.protocol == "udp" && port.port > 0)
        .map(|port| port.port)
        .ok_or_else(|| format!("Startup query port `{name}` is missing"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn per_map_query_arguments_are_not_replaced_by_primary_ports() {
        let args = ["TheIsland?Port=7779?QueryPort=27017?RCONPort=27022".into()];
        assert_eq!(
            argument_value(&args, "QueryPort").unwrap().as_deref(),
            Some("27017")
        );
        assert_eq!(
            argument_value(
                &["+server.queryport".into(), "28017".into()],
                "+server.queryport"
            )
            .unwrap()
            .as_deref(),
            Some("28017")
        );
        assert!(argument_value(&["-abslog=a".into(), "-abslog=b".into()], "-abslog").is_err());
        assert!(argument_value(&["-abslog".into()], "-abslog").is_err());
    }
}
