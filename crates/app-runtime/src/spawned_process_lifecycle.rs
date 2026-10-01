use std::time::{Duration, Instant};

use crate::runtime_supervisor::{request_console_interrupt_for_process, stop_tracked_process};
use crate::{RuntimeChild, RuntimeProcessError, SpawnedProcess, process_matches_identity};
#[cfg(windows)]
use crate::{
    find_windows_preferred_descendant_pid, inspect_process_identity, is_windows_batch_script,
};
#[cfg(windows)]
use std::path::Path;

pub fn stabilize_spawned_process(
    executable_path: &str,
    spawned: &mut SpawnedProcess,
    grace_period: Duration,
) -> Result<Option<i32>, RuntimeProcessError> {
    let root_pid = spawned
        .child
        .as_ref()
        .map(RuntimeChild::id)
        .unwrap_or(spawned.pid);
    let started_at = Instant::now();

    loop {
        if spawned.uses_script_entrypoint {
            #[cfg(windows)]
            if let Some(descendant_pid) = find_windows_preferred_descendant_pid(
                root_pid,
                &spawned.root_process_identity,
                spawned.requires_workload_handoff.then_some(executable_path),
            )? && let Some(identity) = inspect_process_identity(descendant_pid)?
            {
                spawned.pid = descendant_pid;
                spawned.process_identity = identity;
            }
        }

        if let Some(child) = spawned.child.as_mut()
            && let Some(exit_code) =
                child
                    .try_wait_code()
                    .map_err(|source| RuntimeProcessError::WaitTrackedProcess {
                        pid: spawned.pid,
                        source,
                    })?
        {
            if spawned.pid != root_pid
                && process_matches_identity(spawned.pid, &spawned.process_identity)?
            {
                if !child.has_owned_process_tree() {
                    spawned.child = None;
                }
                return Ok(None);
            }
            child.finish_process_tree().map_err(|source| {
                RuntimeProcessError::WaitTrackedProcess {
                    pid: spawned.pid,
                    source,
                }
            })?;
            return Ok(exit_code);
        }

        if started_at.elapsed() >= grace_period {
            // Elevated launches own a helper handle first. Do not persist that
            // helper as the server if process creation exceeds the grace period.
            #[cfg(windows)]
            if spawned.requires_workload_handoff && spawned.pid == root_pid {
                if started_at.elapsed() >= grace_period.max(Duration::from_secs(30)) {
                    return Err(RuntimeProcessError::WorkloadStartupTimedOut {
                        pid: root_pid,
                        path: executable_path.to_owned(),
                    });
                }
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }

            #[cfg(windows)]
            if is_windows_batch_script(Path::new(executable_path))
                && spawned.pid != root_pid
                && !process_matches_identity(spawned.pid, &spawned.process_identity)?
            {
                return Ok(Some(1));
            }

            return Ok(None);
        }

        std::thread::sleep(Duration::from_millis(200));
    }
}

pub fn stop_spawned_process(
    spawned: &mut SpawnedProcess,
) -> Result<Option<i32>, RuntimeProcessError> {
    stop_tracked_process(
        spawned.pid,
        &spawned.process_identity,
        &spawned.root_process_identity,
        &mut spawned.child,
    )
}

/// Request one native console interrupt and release the launch only after its
/// original process handle and complete owned tree have both exited. Errors
/// retain the child, Job, desktop and output reader for observation or recovery.
pub fn stop_spawned_process_gracefully(
    spawned: &mut SpawnedProcess,
    timeout: Duration,
) -> Result<Option<i32>, RuntimeProcessError> {
    stop_spawned_with_control(spawned, timeout, false)
}

/// Unreal's native GUI console executes quit on the game thread. Keep the
/// same complete-tree and exit-code ownership rules as console-interrupt stop.
pub fn stop_spawned_unreal_process_gracefully(
    spawned: &mut SpawnedProcess,
    timeout: Duration,
) -> Result<Option<i32>, RuntimeProcessError> {
    stop_spawned_with_control(spawned, timeout, true)
}

fn stop_spawned_with_control(
    spawned: &mut SpawnedProcess,
    timeout: Duration,
    unreal_quit: bool,
) -> Result<Option<i32>, RuntimeProcessError> {
    let started = Instant::now();
    let wait_error = |source| RuntimeProcessError::WaitTrackedProcess {
        pid: spawned.pid,
        source,
    };
    let timeout_error = || {
        wait_error(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "graceful shutdown did not confirm complete owned-tree exit; ownership retained",
        ))
    };
    let child = spawned.child.as_mut().ok_or_else(|| {
        wait_error(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "graceful shutdown requires the original launch owner",
        ))
    })?;
    if !child.has_owned_process_tree() {
        return Err(wait_error(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "graceful shutdown requires ownership of the complete process tree",
        )));
    }

    let mut interrupt_requested = false;
    let exit_code = loop {
        let root_exit = child.try_wait_code().map_err(wait_error)?;
        let tree_running = child
            .owned_process_tree_is_running()
            .map_err(wait_error)?
            .ok_or_else(|| {
                wait_error(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "complete owned-tree state is unknown; ownership retained",
                ))
            })?;
        if let Some(code) = root_exit
            && !tree_running
        {
            break code;
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(timeout_error());
        }
        // A root that exited naturally must not receive another signal. Keep
        // its original handle while waiting for any remaining descendants.
        if root_exit.is_none() && !interrupt_requested {
            if unreal_quit {
                #[cfg(windows)]
                crate::windows_unreal_console::request(
                    child,
                    &spawned.root_process_identity,
                    spawned.hidden_desktop.as_ref(),
                    "quit",
                )?;
                #[cfg(not(windows))]
                return Err(wait_error(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "Unreal GUI console requires Windows",
                )));
            } else {
                request_console_interrupt_for_process(
                    spawned.pid,
                    &spawned.process_identity,
                    &spawned.root_process_identity,
                    Some(child),
                )?;
            }
            interrupt_requested = true;
            continue;
        }
        std::thread::sleep(remaining.min(Duration::from_millis(20)));
    };

    // Do not use finish_process_tree: it is a forceful cleanup API. The empty
    // owner can now close its resources and join/drain the managed output reader.
    drop(spawned.child.take());
    drop(spawned.hidden_desktop.take());
    Ok(exit_code)
}

#[cfg(all(windows, test))]
#[path = "spawned_process_lifecycle_tests.rs"]
mod tests;
