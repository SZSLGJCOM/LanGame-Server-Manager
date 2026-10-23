use super::*;

/// Bind the native writer only after a concrete run log has been assigned.
/// Ordinary launch previews keep the module's fixed default logfile.
pub fn bind_corekeeper_run_log(plan: &mut LaunchPlan, console_log: &Path) -> Result<(), String> {
    if plan.module_id != "corekeeper" {
        return Err("Core Keeper run log binding requires its own launch plan".into());
    }
    let native = app_core::runtime_logs::corekeeper_native_log_path(console_log)
        .ok_or("Core Keeper requires a valid current-run console path")?;
    match fs::symlink_metadata(&native) {
        Ok(_) => {
            return Err(format!(
                "The current native run log already exists; start again to use a new run. The existing log was preserved: {}",
                native.display()
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "Cannot verify the native run log {}: {error}",
                native.display()
            ));
        }
    }
    let mut options = plan
        .args
        .iter()
        .enumerate()
        .filter_map(|(index, arg)| arg.eq_ignore_ascii_case("-logfile").then_some(index));
    let index = options
        .next()
        .ok_or("Core Keeper launch plan has no native logfile option")?;
    if options.next().is_some()
        || plan
            .args
            .get(index + 1)
            .is_none_or(|path| path.is_empty() || path.starts_with('-'))
    {
        return Err("Core Keeper launch plan has an ambiguous native logfile option".into());
    }
    plan.args[index + 1] = native.to_string_lossy().into_owned();
    plan.command_line = build_command_line(Path::new(&plan.executable_path), &plan.args);
    Ok(())
}
