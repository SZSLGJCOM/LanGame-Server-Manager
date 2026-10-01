use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Map, Value};

use crate::atomic_file::{
    compare_and_swap_file_atomically, compare_and_swap_optional_file_atomically,
};

use super::windrose_plan::{
    clear_pending_plan, pending_plan_document, pending_plan_path, prepare_windrose_documents,
    replace_server,
};
use super::{WindroseWorldTargetError, canonicalize, ensure_within_root, read_bytes};

pub(super) const WINDROSE_WORLD_UPDATER_FILE: &str = "R5WorldDescriptionUpdater.exe";
const WINDROSE_WORLD_UPDATER_TIMEOUT: Duration = Duration::from_secs(120);
const WINDROSE_WORLD_UPDATER_TERMINATION_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg(any(windows, test))]
pub(super) const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(any(windows, test))]
pub(super) fn windrose_updater_creation_flags() -> u32 {
    CREATE_NO_WINDOW
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WindroseUpdaterCommand {
    pub(super) executable: PathBuf,
    pub(super) working_directory: PathBuf,
    pub(super) argument: PathBuf,
}

pub(super) struct WindroseUpdaterStatus {
    success: bool,
    code: Option<i32>,
}

pub(super) enum WindroseUpdaterRunResult {
    Completed(WindroseUpdaterStatus),
    LaunchFailed(io::Error),
    TimedOut(Duration),
    TerminationUnconfirmed(String),
}

impl WindroseUpdaterStatus {
    #[cfg(test)]
    pub(super) fn success() -> Self {
        Self {
            success: true,
            code: Some(0),
        }
    }

    #[cfg(test)]
    pub(super) fn failed(code: Option<i32>) -> Self {
        Self {
            success: false,
            code,
        }
    }
}

pub(super) async fn apply_pending_world_update(
    install_root: &Path,
    config_dir: &Path,
    settings: &Map<String, Value>,
) -> Result<(), WindroseWorldTargetError> {
    apply_pending_world_update_with_runner(
        install_root,
        config_dir,
        settings,
        run_windrose_updater_process,
    )
    .await
}

async fn run_windrose_updater_process(
    invocation: WindroseUpdaterCommand,
) -> WindroseUpdaterRunResult {
    let mut command = tokio::process::Command::new(&invocation.executable);
    command
        .current_dir(&invocation.working_directory)
        .arg(&invocation.argument)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        command.creation_flags(windrose_updater_creation_flags());
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(source) => return WindroseUpdaterRunResult::LaunchFailed(source),
    };
    match tokio::time::timeout(WINDROSE_WORLD_UPDATER_TIMEOUT, child.wait()).await {
        Ok(Ok(status)) => WindroseUpdaterRunResult::Completed(WindroseUpdaterStatus {
            success: status.success(),
            code: status.code(),
        }),
        Ok(Err(source)) => terminate_after_wait_error(child, source).await,
        Err(_) => terminate_after_timeout(child).await,
    }
}

async fn terminate_after_wait_error(
    mut child: tokio::process::Child,
    source: io::Error,
) -> WindroseUpdaterRunResult {
    match terminate_and_reap(&mut child).await {
        Ok(()) => WindroseUpdaterRunResult::LaunchFailed(source),
        Err(message) => WindroseUpdaterRunResult::TerminationUnconfirmed(message),
    }
}

async fn terminate_after_timeout(mut child: tokio::process::Child) -> WindroseUpdaterRunResult {
    match terminate_and_reap(&mut child).await {
        Ok(()) => WindroseUpdaterRunResult::TimedOut(WINDROSE_WORLD_UPDATER_TIMEOUT),
        Err(message) => WindroseUpdaterRunResult::TerminationUnconfirmed(message),
    }
}

async fn terminate_and_reap(child: &mut tokio::process::Child) -> Result<(), String> {
    child
        .start_kill()
        .map_err(|error| format!("failed to request termination: {error}"))?;
    match tokio::time::timeout(WINDROSE_WORLD_UPDATER_TERMINATION_TIMEOUT, child.wait()).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(error)) => Err(format!("failed to reap terminated updater: {error}")),
        Err(_) => Err(format!(
            "process did not exit within {:?} after termination",
            WINDROSE_WORLD_UPDATER_TERMINATION_TIMEOUT
        )),
    }
}

#[cfg(test)]
pub(super) async fn apply_pending_world_update_with<F, Fut>(
    install_root: &Path,
    config_dir: &Path,
    settings: &Map<String, Value>,
    run_updater: F,
) -> Result<(), WindroseWorldTargetError>
where
    F: FnOnce(WindroseUpdaterCommand) -> Fut,
    Fut: Future<Output = io::Result<WindroseUpdaterStatus>>,
{
    apply_pending_world_update_with_timeout(
        install_root,
        config_dir,
        settings,
        WINDROSE_WORLD_UPDATER_TIMEOUT,
        run_updater,
    )
    .await
}

#[cfg(test)]
pub(super) async fn apply_pending_world_update_with_timeout<F, Fut>(
    install_root: &Path,
    config_dir: &Path,
    settings: &Map<String, Value>,
    timeout: Duration,
    run_updater: F,
) -> Result<(), WindroseWorldTargetError>
where
    F: FnOnce(WindroseUpdaterCommand) -> Fut,
    Fut: Future<Output = io::Result<WindroseUpdaterStatus>>,
{
    apply_pending_world_update_with_runner(
        install_root,
        config_dir,
        settings,
        move |invocation| async move {
            match tokio::time::timeout(timeout, run_updater(invocation)).await {
                Ok(Ok(status)) => WindroseUpdaterRunResult::Completed(status),
                Ok(Err(source)) => WindroseUpdaterRunResult::LaunchFailed(source),
                Err(_) => WindroseUpdaterRunResult::TimedOut(timeout),
            }
        },
    )
    .await
}

pub(super) async fn apply_pending_world_update_with_runner<F, Fut>(
    install_root: &Path,
    config_dir: &Path,
    settings: &Map<String, Value>,
    run_updater: F,
) -> Result<(), WindroseWorldTargetError>
where
    F: FnOnce(WindroseUpdaterCommand) -> Fut,
    Fut: Future<Output = WindroseUpdaterRunResult>,
{
    let plan_path = pending_plan_path(config_dir);
    if !plan_path.is_file() {
        return Ok(());
    }
    let persisted_plan =
        super::windrose_document::parse_json(&plan_path, &read_bytes(&plan_path)?)?;
    let prepared = prepare_windrose_documents(install_root, config_dir, settings)?;
    let world =
        prepared
            .world
            .as_ref()
            .ok_or_else(|| WindroseWorldTargetError::InvalidPendingPlan {
                path: plan_path.clone(),
                message: String::from("selected world is absent"),
            })?;
    let expected_plan = pending_plan_document(install_root, config_dir, &prepared, settings)?;
    if persisted_plan != expected_plan {
        return Err(WindroseWorldTargetError::InvalidPendingPlan {
            path: plan_path,
            message: String::from("plan no longer matches the selected world and settings"),
        });
    }
    let canonical_install = canonicalize(install_root)?;
    let updater = install_root.join(WINDROSE_WORLD_UPDATER_FILE);
    if !updater.is_file() {
        return Err(WindroseWorldTargetError::MissingUpdater { path: updater });
    }
    let canonical_updater = canonicalize(&updater)?;
    ensure_within_root(&canonical_install, &canonical_updater)?;
    let relative_world = world
        .path
        .strip_prefix(&canonical_install)
        .map(Path::to_path_buf)
        .map_err(|_| WindroseWorldTargetError::OutsideInstallRoot {
            path: world.path.clone(),
            root: canonical_install.clone(),
        })?;

    replace_world(world)?;
    if let Err(error) = replace_server(
        &prepared.server_path,
        prepared.server_original.as_deref(),
        &prepared.server_replacement,
    ) {
        rollback_world(world)?;
        return Err(error);
    }

    let status = run_updater(WindroseUpdaterCommand {
        executable: canonical_updater,
        working_directory: canonical_install,
        argument: relative_world,
    })
    .await;
    match status {
        WindroseUpdaterRunResult::Completed(status) if status.success => {
            clear_pending_plan(config_dir)
        }
        WindroseUpdaterRunResult::Completed(status) => {
            rollback_documents(&prepared)?;
            Err(WindroseWorldTargetError::UpdaterFailed { code: status.code })
        }
        WindroseUpdaterRunResult::LaunchFailed(source) => {
            rollback_documents(&prepared)?;
            Err(WindroseWorldTargetError::UpdaterLaunch {
                path: updater,
                source,
            })
        }
        WindroseUpdaterRunResult::TimedOut(timeout) => {
            rollback_documents(&prepared)?;
            Err(WindroseWorldTargetError::UpdaterTimeout { timeout })
        }
        WindroseUpdaterRunResult::TerminationUnconfirmed(message) => {
            Err(WindroseWorldTargetError::UpdaterTermination {
                path: updater,
                message,
            })
        }
    }
}

fn replace_world(
    world: &super::windrose_plan::PreparedWindroseWorld,
) -> Result<(), WindroseWorldTargetError> {
    if !compare_and_swap_file_atomically(&world.path, &world.original, &world.replacement).map_err(
        |source| WindroseWorldTargetError::Replacement {
            path: world.path.clone(),
            source,
        },
    )? {
        return Err(WindroseWorldTargetError::ConcurrentModification {
            path: world.path.clone(),
        });
    }
    Ok(())
}

fn rollback_world(
    world: &super::windrose_plan::PreparedWindroseWorld,
) -> Result<(), WindroseWorldTargetError> {
    let rolled_back =
        compare_and_swap_file_atomically(&world.path, &world.replacement, &world.original)
            .unwrap_or(false);
    if rolled_back {
        Ok(())
    } else {
        Err(WindroseWorldTargetError::RollbackFailed {
            path: world.path.clone(),
        })
    }
}

fn rollback_documents(
    prepared: &super::windrose_plan::PreparedWindroseDocuments,
) -> Result<(), WindroseWorldTargetError> {
    let server_rolled_back = match prepared.server_original.as_deref() {
        Some(original_server) if original_server == prepared.server_replacement => true,
        original_server => compare_and_swap_optional_file_atomically(
            &prepared.server_path,
            Some(&prepared.server_replacement),
            original_server,
        )
        .unwrap_or(false),
    };
    let world_rolled_back = prepared
        .world
        .as_ref()
        .is_some_and(|world| rollback_world(world).is_ok());
    if !server_rolled_back {
        return Err(WindroseWorldTargetError::RollbackFailed {
            path: prepared.server_path.clone(),
        });
    }
    if !world_rolled_back {
        return Err(WindroseWorldTargetError::RollbackFailed {
            path: prepared
                .world
                .as_ref()
                .map(|world| world.path.clone())
                .unwrap_or_else(|| prepared.server_path.clone()),
        });
    }
    Ok(())
}
