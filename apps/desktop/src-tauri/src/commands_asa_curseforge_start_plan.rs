use app_core::{InstanceDetails, ProcessLaunchPlan};
use std::collections::BTreeSet;
use std::path::PathBuf;

pub(super) struct Requirements {
    pub(super) project_ids: Vec<String>,
    pub(super) native_log: PathBuf,
}

fn flag_values<'a>(args: &'a [String], names: &[&str]) -> Vec<&'a str> {
    let mut values = Vec::new();
    let mut arguments = args.iter().peekable();
    while let Some(argument) = arguments.next() {
        let (key, value) = argument.split_once('=').map_or_else(
            || {
                (
                    argument.as_str(),
                    arguments
                        .peek()
                        .map(|value| value.as_str())
                        .filter(|value| !value.starts_with('-')),
                )
            },
            |(key, value)| (key, Some(value)),
        );
        if names.iter().any(|name| key.eq_ignore_ascii_case(name))
            && let Some(value) = value
        {
            values.push(value.trim());
        }
    }
    values
}

pub(in crate::commands) fn requires_validation(module_id: &str, args: &[String]) -> bool {
    module_id == "arksurvivalascended"
        && flag_values(args, &["-mods", "-passivemods"])
            .iter()
            .any(|value| !value.is_empty())
}

pub(super) fn collect(
    instance: &InstanceDetails,
    plan: &ProcessLaunchPlan,
) -> Result<Option<Requirements>, String> {
    let module_id = &instance.summary.module_id;
    if !requires_validation(module_id, &plan.launch_plan.args) {
        return Ok(None);
    }
    let mut project_ids = BTreeSet::new();
    for flag in ["-mods", "-passivemods"] {
        if flag_values(&plan.launch_plan.args, &[flag]).len() > 1 {
            return Err("ASA has duplicate native Mod flags; keep one -mods and one -passivemods list before starting.".into());
        }
    }
    for value in flag_values(&plan.launch_plan.args, &["-mods", "-passivemods"]) {
        if value.len() > 128 * 1024 {
            return Err("ASA CurseForge Mod list exceeds the startup limit.".into());
        }
        for token in value
            .split(',')
            .map(str::trim)
            .filter(|token| !token.is_empty())
        {
            // Wildcard documents the -dev suffix for an author's unpublished
            // revision. Observe its native validation without changing that
            // intentional revision into a published-file update.
            let id = token.strip_suffix("-dev").unwrap_or(token);
            let numeric = id.parse::<i64>().ok().filter(|id| *id > 0);
            if !id.bytes().all(|byte| byte.is_ascii_digit()) || numeric.is_none() {
                return Err("ASA native Mod flags require positive CurseForge project IDs; correct -mods/-passivemods before starting.".into());
            }
            project_ids.insert(numeric.ok_or("Invalid ASA Mod project ID")?.to_string());
            if project_ids.len() > 8192 {
                return Err("ASA CurseForge Mod list exceeds the startup limit.".into());
            }
        }
    }
    if project_ids.is_empty() {
        return Err("ASA Mod flags contain no verifiable CurseForge project IDs.".into());
    }
    let map = app_core::ark_maps::processes(instance)?
        .into_iter()
        .find(|map| map.process_key == plan.process_key)
        .ok_or("ASA Mod startup has an unknown map process")?;
    let paths = flag_values(&plan.launch_plan.args, &["-abslog"]);
    let [path] = paths.as_slice() else {
        return Err("ASA Mod startup requires exactly one owned native -abslog path.".into());
    };
    let normalize = |path: &str| path.replace('\\', "/").to_ascii_lowercase();
    if normalize(path) != normalize(&map.native_log_path) {
        return Err("ASA Mod startup log does not belong to this instance/map; remove conflicting -abslog flags.".into());
    }
    Ok(Some(Requirements {
        project_ids: project_ids.into_iter().collect(),
        native_log: PathBuf::from(map.native_log_path),
    }))
}

#[cfg(test)]
#[path = "commands_asa_curseforge_start_plan_tests.rs"]
mod tests;
