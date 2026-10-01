use super::*;

pub fn spawn_launch_plan(
    plan: &LaunchPlan,
    log_path: impl AsRef<Path>,
) -> Result<SpawnedProcess, RuntimeProcessError> {
    spawn(plan, log_path.as_ref(), None, None)
}

/// Use an application-owned sink without allowing the child to retain its file
/// handle. This is required when the application rotates output segments.
pub fn spawn_launch_plan_with_log_writer(
    plan: &LaunchPlan,
    log_path: impl AsRef<Path>,
    writer: Box<dyn std::io::Write + Send>,
) -> Result<SpawnedProcess, RuntimeProcessError> {
    spawn(plan, log_path.as_ref(), Some(writer), None)
}

/// A single shared group must be supplied for all worlds of an instance.
pub fn spawn_launch_plan_in_resource_group(
    plan: &LaunchPlan,
    log_path: impl AsRef<Path>,
    writer: Option<Box<dyn std::io::Write + Send>>,
    resource_group: &RuntimeResourceGroup,
) -> Result<SpawnedProcess, RuntimeProcessError> {
    spawn(plan, log_path.as_ref(), writer, Some(resource_group))
}

fn spawn(
    plan: &LaunchPlan,
    log_path: &Path,
    writer: Option<Box<dyn std::io::Write + Send>>,
    resource_group: Option<&RuntimeResourceGroup>,
) -> Result<SpawnedProcess, RuntimeProcessError> {
    let limits = &plan.performance_policy.resource_limits;
    let resources_valid = limits.validate().and_then(|()| {
        if limits.enabled() && (plan.requires_admin || !cfg!(windows)) {
            return Err("Instance resource limits require a non-elevated Windows launch".into());
        }
        match resource_group {
            Some(group) if group.instance_id() != plan.instance_id || group.limits() != limits => {
                Err("Launch plan does not match its instance resource reservation".into())
            }
            None if limits.enabled() => {
                Err("Resource-limited launches require an instance resource reservation".into())
            }
            _ => Ok(()),
        }
    });
    resources_valid.map_err(|message| RuntimeProcessError::SpawnProcess {
        path: plan.executable_path.clone(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, message),
    })?;
    if writer.is_some() && (plan.requires_admin || !cfg!(windows)) {
        return Err(RuntimeProcessError::SpawnProcess {
            path: plan.executable_path.clone(),
            source: std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Managed output sinks require a non-elevated Windows launch.",
            ),
        });
    }
    validate_environment(&plan.environment, plan.requires_admin).map_err(|source| {
        RuntimeProcessError::SpawnProcess {
            path: plan.executable_path.clone(),
            source,
        }
    })?;
    if !plan.executable_exists {
        return Err(RuntimeProcessError::MissingExecutable {
            path: plan.executable_path.clone(),
        });
    }

    let log_path = log_path.to_path_buf();
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent).map_err(|source| RuntimeProcessError::CreateLogDirectory {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let executable_path = PathBuf::from(&plan.executable_path);
    let working_directory = PathBuf::from(&plan.working_directory);
    let uses_script_entrypoint = is_script_entrypoint(&executable_path, &plan.args)
        || plan.uses_script_entrypoint
        || plan.requires_admin;
    let (spawn_command, spawn_args) =
        build_spawn_command(&executable_path, &working_directory, &plan.args).map_err(
            |source| RuntimeProcessError::SpawnProcess {
                path: plan.executable_path.clone(),
                source,
            },
        )?;
    let run_in_background = matches!(plan.window_policy, ProcessWindowPolicy::Background);
    let command = SpawnCommand {
        executable: &spawn_command,
        args: &spawn_args,
        working_directory: &working_directory,
        environment: &plan.environment,
    };

    let open_file = || File::options().create(true).append(true).open(&log_path);
    #[cfg(windows)]
    let mut output = None;
    #[cfg(windows)]
    let spawn_result = (|| -> std::io::Result<_> {
        if matches!(plan.host_surface, ProcessHostSurface::ManagedPseudoConsole) {
            if plan.requires_admin {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "Managed pseudo consoles do not support elevated launches.",
                ));
            }
            let sink = match writer {
                Some(writer) => writer,
                None => Box::new(open_file()?),
            };
            return pseudo_console::spawn(&command, sink, run_in_background, resource_group);
        }
        let stdout = match writer {
            Some(writer) => {
                let (owner, pipe) = windows_process_output::ManagedProcessOutput::new(writer)?;
                output = Some(owner);
                pipe
            }
            None => open_file()?,
        };
        let stderr = stdout.try_clone()?;
        if plan.requires_admin {
            spawn_run_as_process(
                &spawn_command,
                &spawn_args,
                &working_directory,
                stdout,
                stderr,
                run_in_background,
            )
        } else if run_in_background {
            spawn_hidden_desktop_process(
                &command,
                stdout,
                stderr,
                background_creation_flags(uses_script_entrypoint, &plan.host_surface),
                resource_group,
            )
        } else {
            spawn_standard_process(
                &command,
                stdout,
                stderr,
                run_in_background,
                uses_script_entrypoint,
                resource_group,
            )
        }
    })();

    #[cfg(not(windows))]
    let spawn_result = (|| -> std::io::Result<_> {
        if matches!(plan.host_surface, ProcessHostSurface::ManagedPseudoConsole) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Managed pseudo consoles require Windows.",
            ));
        }
        let stdout = open_file()?;
        let stderr = stdout.try_clone()?;
        spawn_standard_process(
            &command,
            stdout,
            stderr,
            run_in_background,
            uses_script_entrypoint,
        )
    })();
    let (mut child, hidden_desktop) =
        spawn_result.map_err(|source| RuntimeProcessError::SpawnProcess {
            path: plan.executable_path.clone(),
            source,
        })?;
    #[cfg(windows)]
    if let RuntimeChild::Windows(child) = &mut child {
        child.output = output;
    }
    let pid = child.id();
    let root_process_identity = match inspect_process_identity(pid) {
        Ok(Some(identity)) => identity,
        Ok(None) => {
            return Err(cleanup_failed_launch(
                &mut child,
                RuntimeProcessError::ProcessIdentityUnavailable { pid },
            ));
        }
        Err(error) => {
            return Err(cleanup_failed_launch(&mut child, error));
        }
    };

    Ok(SpawnedProcess {
        child: Some(child),
        pid,
        process_identity: root_process_identity.clone(),
        root_process_identity,
        log_path: log_path.to_string_lossy().into_owned(),
        uses_script_entrypoint,
        #[cfg(windows)]
        requires_workload_handoff: plan.requires_admin
            && !is_script_entrypoint(&executable_path, &plan.args),
        hidden_desktop,
    })
}

pub(super) fn cleanup_failed_launch(
    child: &mut RuntimeChild,
    launch_error: RuntimeProcessError,
) -> RuntimeProcessError {
    let cleanup = (|| {
        // Failed termination must not fall through to an infinite process wait.
        // Dropping the retained native owner still requests its Job/pipe cleanup.
        child.terminate_owned()?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
        loop {
            if child.try_wait()?.is_some() {
                return Ok(());
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "failed launch process did not finish after cleanup",
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(20).min(remaining));
        }
    })();
    match cleanup {
        Ok(()) => launch_error,
        Err(source) => RuntimeProcessError::FailedLaunchCleanup {
            launch_error: Box::new(launch_error),
            source,
        },
    }
}

#[cfg(all(test, windows))]
#[path = "runtime_console_acceptance_tests.rs"]
mod console_acceptance_tests;
