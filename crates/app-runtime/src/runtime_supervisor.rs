use std::collections::HashMap;
use std::time::{Duration, Instant};

use app_core::{
    InstanceSummary, ProcessIdentity, RuntimePerformanceApplication, RuntimePerformancePolicy,
    RuntimePerformancePolicyPreview, RuntimeProcessPerformanceSnapshot,
};

use crate::{
    RuntimeChild, RuntimeProcessError, WindowsHiddenDesktop, apply_runtime_performance_policy,
    inspect_process_identity, kill_process_by_pid, process_matches_identity,
};
#[cfg(windows)]
use crate::{find_windows_preferred_descendant_pid, request_windows_console_ctrl_c};

#[path = "stdin_dispatch.rs"]
mod stdin_dispatch;
use stdin_dispatch::StdinDispatchRegistry;
pub use stdin_dispatch::{
    RuntimeCommandDispatchLease, RuntimeCommandDispatchTarget, RuntimeCommandSubmissionTracker,
};

#[derive(Debug)]
pub struct ManagedProcess {
    pub run_id: i64,
    pub process_key: String,
    pub display_name: String,
    pub pid: u32,
    pub process_identity: ProcessIdentity,
    pub root_process_identity: ProcessIdentity,
    pub log_path: String,
    pub is_primary: bool,
    pub uses_script_entrypoint: bool,
    pub performance_policy: RuntimePerformancePolicy,
    pub last_performance_refresh: Option<Instant>,
    pub last_performance_target_count: Option<usize>,
    pub last_performance_application: Option<RuntimePerformanceApplication>,
    pub child: Option<RuntimeChild>,
    pub hidden_desktop: Option<WindowsHiddenDesktop>,
}

#[derive(Debug)]
pub struct ManagedInstance {
    pub summary: InstanceSummary,
    pub session_id: Option<String>,
    // Drop cancels outstanding input before releasing any child process handles.
    stdin_dispatches: StdinDispatchRegistry,
    pub processes: Vec<ManagedProcess>,
}

#[derive(Debug, Clone)]
pub struct TrackedInstance {
    pub summary: InstanceSummary,
    pub session_id: Option<String>,
    pub run_id: i64,
    pub pid: Option<u32>,
    pub log_path: Option<String>,
    pub process_count: usize,
}

#[derive(Debug, Clone)]
pub(super) struct RuntimePerformanceResolution {
    pub(super) policy: RuntimePerformancePolicy,
    pub(super) preview: RuntimePerformancePolicyPreview,
}

#[derive(Debug, Clone)]
pub struct ExitedManagedProcess {
    pub summary: InstanceSummary,
    pub session_id: Option<String>,
    pub run_id: i64,
    pub process_key: String,
    pub display_name: String,
    pub pid: u32,
    pub log_path: String,
    pub is_primary: bool,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct StoppedManagedProcess {
    pub run_id: i64,
    pub process_key: String,
    pub display_name: String,
    pub pid: u32,
    pub log_path: String,
    pub is_primary: bool,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct RuntimePerformanceRefresh {
    pub instance_id: String,
    pub instance_name: String,
    pub process_key: String,
    pub display_name: String,
    pub application: RuntimePerformanceApplication,
    pub target_count_changed: bool,
}

#[derive(Debug, Default)]
pub struct RuntimeSupervisor {
    instances: HashMap<String, ManagedInstance>,
}

impl RuntimeSupervisor {
    pub fn is_tracked(&self, instance_id: &str) -> bool {
        self.instances.contains_key(instance_id)
    }

    pub fn insert_running(
        &mut self,
        summary: InstanceSummary,
        session_id: Option<String>,
        mut processes: Vec<ManagedProcess>,
    ) {
        sort_managed_processes(&mut processes);
        self.instances.insert(
            summary.id.clone(),
            ManagedInstance {
                summary,
                session_id,
                stdin_dispatches: StdinDispatchRegistry::default(),
                processes,
            },
        );
    }

    pub fn take_running_for_stop(&mut self, instance_id: &str) -> Option<ManagedInstance> {
        let instance = self.instances.remove(instance_id)?;
        instance.stdin_dispatches.cancel_all();
        Some(instance)
    }

    pub fn restore_running_after_failed_stop(&mut self, mut instance: ManagedInstance) -> bool {
        let instance_id = instance.summary.id.clone();
        if self.instances.contains_key(&instance_id) {
            return false;
        }
        instance
            .stdin_dispatches
            .restore_returned(&mut instance.processes);
        self.instances.insert(instance_id, instance);
        true
    }

    pub fn tracked_instances(&self) -> Vec<TrackedInstance> {
        self.instances
            .values()
            .filter_map(|instance| {
                let primary = select_primary_managed_process(&instance.processes)?;
                Some(TrackedInstance {
                    summary: instance.summary.clone(),
                    session_id: instance.session_id.clone(),
                    run_id: primary.run_id,
                    pid: Some(primary.pid),
                    log_path: Some(primary.log_path.clone()),
                    process_count: instance.processes.len(),
                })
            })
            .collect()
    }

    pub fn matches_running_process(
        &mut self,
        instance_id: &str,
        run_id: i64,
        process_key: &str,
        pid: u32,
    ) -> Result<bool, RuntimeProcessError> {
        let Some(instance) = self.instances.get_mut(instance_id) else {
            return Ok(false);
        };
        let Some(process) = instance.processes.iter_mut().find(|process| {
            process.run_id == run_id
                && process.pid == pid
                && process.process_key.eq_ignore_ascii_case(process_key)
        }) else {
            return Ok(false);
        };
        let Some(child) = process.child.as_mut() else {
            return Ok(false);
        };
        if child.id() != pid {
            return process_matches_identity(pid, &process.process_identity);
        }
        child
            .try_wait()
            .map(|status| status.is_none())
            .map_err(|source| RuntimeProcessError::WaitTrackedProcess { pid, source })
    }

    /// `None` means no handle owns this exact run and PID, not that it exited.
    pub fn owned_process_is_running(
        &mut self,
        instance_id: &str,
        run_id: i64,
        process_key: &str,
        pid: u32,
    ) -> Result<Option<bool>, RuntimeProcessError> {
        let child = self.instances.get_mut(instance_id).and_then(|instance| {
            instance.processes.iter_mut().find_map(|process| {
                if process.run_id != run_id
                    || process.pid != pid
                    || !process.process_key.eq_ignore_ascii_case(process_key)
                {
                    return None;
                }
                process.child.as_mut().filter(|child| child.id() == pid)
            })
        });
        let Some(child) = child else {
            return Ok(None);
        };
        child
            .try_wait()
            .map(|status| Some(status.is_none()))
            .map_err(|source| RuntimeProcessError::WaitTrackedProcess { pid, source })
    }

    /// Inspect the complete owned trees without reaping or terminating them.
    /// `None` means ownership cannot prove that every descendant has exited.
    pub fn instance_process_tree_is_running(
        &mut self,
        instance_id: &str,
    ) -> Result<Option<bool>, RuntimeProcessError> {
        let Some(instance) = self.instances.get_mut(instance_id) else {
            return Ok(None);
        };
        let mut complete = !instance.processes.is_empty();
        let mut running = false;
        for process in &mut instance.processes {
            let Some(child) = process.child.as_mut() else {
                complete = false;
                continue;
            };
            match child.owned_process_tree_is_running().map_err(|source| {
                RuntimeProcessError::WaitTrackedProcess {
                    pid: process.pid,
                    source,
                }
            })? {
                Some(is_running) => running |= is_running,
                None => complete = false,
            }
        }
        Ok(complete.then_some(running))
    }

    pub fn reap_exited(&mut self) -> Result<Vec<ExitedManagedProcess>, RuntimeProcessError> {
        self.reap_exited_matching(|_| true)
    }

    /// Reconcile only instances whose lifecycle mutation lock the caller owns.
    pub fn reap_exited_for(
        &mut self,
        instance_ids: &std::collections::HashSet<String>,
    ) -> Result<Vec<ExitedManagedProcess>, RuntimeProcessError> {
        self.reap_exited_matching(|id| instance_ids.contains(id))
    }

    fn reap_exited_matching(
        &mut self,
        eligible: impl Fn(&str) -> bool,
    ) -> Result<Vec<ExitedManagedProcess>, RuntimeProcessError> {
        let mut emptied_instances = Vec::new();
        let mut exited_processes = Vec::new();
        let mut completed_instances = Vec::new();

        for (instance_id, instance) in self.instances.iter_mut() {
            if !eligible(instance_id) {
                continue;
            }
            let mut exited_indexes = Vec::new();

            for (index, process) in instance.processes.iter_mut().enumerate() {
                if process.uses_script_entrypoint
                    && process.pid == managed_process_root_pid(process)
                {
                    #[cfg(windows)]
                    if let Some(descendant_pid) = find_windows_preferred_descendant_pid(
                        managed_process_root_pid(process),
                        &process.root_process_identity,
                        None,
                    )? && let Some(identity) = inspect_process_identity(descendant_pid)?
                    {
                        process.pid = descendant_pid;
                        process.process_identity = identity;
                    }
                }

                let mut release_child = false;
                let mut stop_after_loop = false;
                let exit_code = if let Some(root_pid) = process.child.as_ref().map(RuntimeChild::id)
                {
                    if process.pid != root_pid {
                        if !process_matches_identity(process.pid, &process.process_identity)? {
                            stop_after_loop = true;
                            None
                        } else if let Some(child) = process.child.as_mut() {
                            if child
                                .try_wait_code()
                                .map_err(|source| RuntimeProcessError::WaitTrackedProcess {
                                    pid: process.pid,
                                    source,
                                })?
                                .is_some()
                            {
                                release_child = !child.has_owned_process_tree();
                            }
                            None
                        } else {
                            None
                        }
                    } else if let Some(child) = process.child.as_mut() {
                        child.try_wait_code().map_err(|source| {
                            RuntimeProcessError::WaitTrackedProcess {
                                pid: process.pid,
                                source,
                            }
                        })?
                    } else {
                        None
                    }
                } else if process_matches_identity(process.pid, &process.process_identity)? {
                    None
                } else {
                    Some(None)
                };

                if (stop_after_loop || exit_code.is_some())
                    && let Some(child) = process.child.as_mut()
                    && child.has_owned_process_tree()
                    && child.owned_process_tree_is_running().map_err(|source| {
                        RuntimeProcessError::WaitTrackedProcess {
                            pid: process.pid,
                            source,
                        }
                    })? != Some(false)
                {
                    // A launcher/workload exiting is not permission to kill
                    // its descendants. Keep the owner for a later observation,
                    // including after a normal stop timed out.
                    continue;
                }

                let exit_code = if stop_after_loop {
                    instance.stdin_dispatches.remove_process(process);
                    Some(stop_tracked_process(
                        process.pid,
                        &process.process_identity,
                        &process.root_process_identity,
                        &mut process.child,
                    )?)
                } else {
                    exit_code
                };

                if release_child {
                    instance.stdin_dispatches.remove_process(process);
                    process.child = None;
                }

                if let Some(exit_code) = exit_code {
                    instance.stdin_dispatches.remove_process(process);
                    if let Some(child) = process.child.as_mut() {
                        child.finish_process_tree().map_err(|source| {
                            RuntimeProcessError::WaitTrackedProcess {
                                pid: process.pid,
                                source,
                            }
                        })?;
                    }
                    exited_indexes.push((index, exit_code));
                }
            }

            completed_instances.push((instance_id.clone(), exited_indexes));
        }

        // Keep every exit owned until all fallible inspection/cleanup succeeds.
        // A later instance's cleanup failure must not discard earlier exits.
        for (instance_id, exited_indexes) in completed_instances {
            let Some(instance) = self.instances.get_mut(&instance_id) else {
                continue;
            };
            for (index, exit_code) in exited_indexes.into_iter().rev() {
                let process = instance.processes.remove(index);
                exited_processes.push(ExitedManagedProcess {
                    summary: instance.summary.clone(),
                    session_id: instance.session_id.clone(),
                    run_id: process.run_id,
                    process_key: process.process_key,
                    display_name: process.display_name,
                    pid: process.pid,
                    log_path: process.log_path,
                    is_primary: process.is_primary,
                    exit_code,
                });
            }

            if instance.processes.is_empty() {
                emptied_instances.push(instance_id.clone());
            }
        }

        for instance_id in emptied_instances {
            self.instances.remove(&instance_id);
        }

        Ok(exited_processes)
    }

    pub fn refresh_performance_policies(
        &mut self,
        min_interval: Duration,
    ) -> Vec<RuntimePerformanceRefresh> {
        let now = Instant::now();
        let mut refreshed = Vec::new();

        for instance in self.instances.values_mut() {
            for process in &mut instance.processes {
                let should_refresh = process
                    .last_performance_refresh
                    .map(|last| now.saturating_duration_since(last) >= min_interval)
                    .unwrap_or(true);
                if !should_refresh {
                    continue;
                }

                let application = apply_runtime_performance_policy(
                    process.pid,
                    &process.process_identity,
                    &process.performance_policy,
                );
                let target_count_changed = process.last_performance_target_count
                    != Some(application.targeted_process_count);
                process.last_performance_refresh = Some(now);
                process.last_performance_target_count = Some(application.targeted_process_count);
                process.last_performance_application = Some(application.clone());
                refreshed.push(RuntimePerformanceRefresh {
                    instance_id: instance.summary.id.clone(),
                    instance_name: instance.summary.name.clone(),
                    process_key: process.process_key.clone(),
                    display_name: process.display_name.clone(),
                    application,
                    target_count_changed,
                });
            }
        }

        refreshed
    }

    pub fn performance_snapshots(
        &self,
        instance_id: &str,
    ) -> Vec<RuntimeProcessPerformanceSnapshot> {
        self.instances
            .get(instance_id)
            .map(|instance| {
                instance
                    .processes
                    .iter()
                    .map(|process| RuntimeProcessPerformanceSnapshot {
                        process_key: process.process_key.clone(),
                        display_name: process.display_name.clone(),
                        pid: process.pid,
                        policy: process.performance_policy.clone(),
                        application: process.last_performance_application.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn dispatch_command(
        &mut self,
        instance_id: &str,
        process_key: Option<&str>,
        command: &str,
    ) -> Result<RuntimeCommandDispatchTarget, RuntimeProcessError> {
        let mut lease = self.begin_command_dispatch(instance_id, process_key)?;
        let write_result = lease.write_stdin_line(command);
        let target = lease.target().clone();
        self.finish_command_dispatch(lease);
        write_result?;
        Ok(target)
    }

    pub fn begin_command_dispatch(
        &mut self,
        instance_id: &str,
        process_key: Option<&str>,
    ) -> Result<RuntimeCommandDispatchLease, RuntimeProcessError> {
        let instance = self.instances.get_mut(instance_id).ok_or_else(|| {
            RuntimeProcessError::TrackedInstanceNotFound {
                instance_id: instance_id.to_string(),
            }
        })?;

        let target_index = select_requested_managed_process_index(&instance.processes, process_key)
            .ok_or_else(|| RuntimeProcessError::TrackedProcessNotFound {
                instance_id: instance_id.to_string(),
                process_key: process_key.unwrap_or("primary").to_string(),
            })?;

        instance
            .stdin_dispatches
            .restore_returned(&mut instance.processes);
        let process = instance.processes.get_mut(target_index).ok_or_else(|| {
            RuntimeProcessError::TrackedProcessNotFound {
                instance_id: instance_id.to_string(),
                process_key: process_key.unwrap_or("primary").to_string(),
            }
        })?;

        instance.stdin_dispatches.begin(instance_id, process)
    }

    /// Reclaim returned stdin only from the current instance owner. Stop callers
    /// can wait for an earlier accepted writer without replaying its command.
    pub fn has_pending_command_dispatches(
        &mut self,
        instance_id: &str,
        expected_run_id: i64,
    ) -> Result<bool, RuntimeProcessError> {
        let Some(instance) = self.instances.get_mut(instance_id) else {
            return Ok(false);
        };
        if select_primary_managed_process(&instance.processes).map(|process| process.run_id)
            != Some(expected_run_id)
        {
            return Err(RuntimeProcessError::TrackedInstanceRunChanged {
                instance_id: instance_id.to_owned(),
                expected_run_id,
            });
        }
        instance
            .stdin_dispatches
            .restore_returned(&mut instance.processes);
        Ok(instance.stdin_dispatches.has_pending())
    }

    pub fn finish_command_dispatch(&mut self, lease: RuntimeCommandDispatchLease) {
        let instance_id = lease.return_stdin();
        if let Some(instance) = self.instances.get_mut(&instance_id) {
            instance
                .stdin_dispatches
                .restore_returned(&mut instance.processes);
        }
    }

    pub fn request_console_interrupt(
        &mut self,
        instance_id: &str,
        process_key: Option<&str>,
    ) -> Result<RuntimeCommandDispatchTarget, RuntimeProcessError> {
        self.request_native_control(instance_id, process_key, |process| {
            request_console_interrupt_for_process(
                process.pid,
                &process.process_identity,
                &process.root_process_identity,
                process.child.as_mut(),
            )
        })
    }

    pub fn request_window_close(
        &mut self,
        instance_id: &str,
        process_key: Option<&str>,
    ) -> Result<RuntimeCommandDispatchTarget, RuntimeProcessError> {
        self.request_native_control(instance_id, process_key, |process| {
            #[cfg(windows)]
            {
                let (pid, identity) = console_interrupt_target(
                    process.pid,
                    &process.process_identity,
                    &process.root_process_identity,
                    process.child.as_mut(),
                )?;
                crate::windows_window_close::request_windows_window_close(
                    pid,
                    identity,
                    process.hidden_desktop.as_ref(),
                )
            }
            #[cfg(not(windows))]
            Err(RuntimeProcessError::WindowCloseProcess {
                pid: process.pid,
                operation: "platform support check",
                source: std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "window close is only implemented for Windows managed processes",
                ),
            })
        })
    }

    pub fn request_unreal_console_command(
        &mut self,
        instance_id: &str,
        process_key: Option<&str>,
        command: &str,
    ) -> Result<RuntimeCommandDispatchTarget, RuntimeProcessError> {
        self.request_native_control(instance_id, process_key, |process| {
            #[cfg(windows)]
            {
                let child = process
                    .child
                    .as_ref()
                    .ok_or(RuntimeProcessError::ProcessIdentityUnavailable { pid: process.pid })?;
                crate::windows_unreal_console::request(
                    child,
                    &process.root_process_identity,
                    process.hidden_desktop.as_ref(),
                    command,
                )
            }
            #[cfg(not(windows))]
            {
                let _ = command;
                Err(RuntimeProcessError::WriteTrackedProcessStdin {
                    pid: process.pid,
                    source: std::io::Error::new(
                        std::io::ErrorKind::Unsupported,
                        "Unreal GUI console requires Windows",
                    ),
                })
            }
        })
    }

    fn request_native_control(
        &mut self,
        instance_id: &str,
        process_key: Option<&str>,
        request: impl FnOnce(&mut ManagedProcess) -> Result<(), RuntimeProcessError>,
    ) -> Result<RuntimeCommandDispatchTarget, RuntimeProcessError> {
        let instance = self.instances.get_mut(instance_id).ok_or_else(|| {
            RuntimeProcessError::TrackedInstanceNotFound {
                instance_id: instance_id.to_string(),
            }
        })?;

        let target_index = select_requested_managed_process_index(&instance.processes, process_key)
            .ok_or_else(|| RuntimeProcessError::TrackedProcessNotFound {
                instance_id: instance_id.to_string(),
                process_key: process_key.unwrap_or("primary").to_string(),
            })?;

        let process = instance.processes.get_mut(target_index).ok_or_else(|| {
            RuntimeProcessError::TrackedProcessNotFound {
                instance_id: instance_id.to_string(),
                process_key: process_key.unwrap_or("primary").to_string(),
            }
        })?;

        request(process)?;

        Ok(RuntimeCommandDispatchTarget {
            process_key: process.process_key.clone(),
            display_name: process.display_name.clone(),
            pid: process.pid,
        })
    }
}

fn sort_managed_processes(processes: &mut [ManagedProcess]) {
    processes.sort_by(|left, right| {
        right
            .is_primary
            .cmp(&left.is_primary)
            .then_with(|| left.run_id.cmp(&right.run_id))
            .then_with(|| left.process_key.cmp(&right.process_key))
    });
}

#[cfg(all(windows, test))]
#[path = "stdin_dispatch_cancellation_tests.rs"]
mod stdin_dispatch_cancellation_tests;

fn managed_process_root_pid(process: &ManagedProcess) -> u32 {
    process
        .child
        .as_ref()
        .map(RuntimeChild::id)
        .unwrap_or(process.pid)
}

pub(super) fn stop_tracked_process(
    tracked_pid: u32,
    tracked_identity: &ProcessIdentity,
    root_identity: &ProcessIdentity,
    child_slot: &mut Option<RuntimeChild>,
) -> Result<Option<i32>, RuntimeProcessError> {
    if let Some(child) = child_slot.as_mut() {
        let root_pid = child.id();
        #[cfg(windows)]
        if child.has_owned_process_tree() {
            let root_exit = child.try_wait_code().map_err(|source| {
                RuntimeProcessError::WaitTrackedProcess {
                    pid: tracked_pid,
                    source,
                }
            })?;
            let mut exit_code = if tracked_pid == root_pid {
                root_exit.flatten()
            } else {
                None
            };
            let workload_running = if tracked_pid == root_pid {
                root_exit.is_none()
            } else {
                process_matches_identity(tracked_pid, tracked_identity)?
            };
            if workload_running
                && request_console_interrupt_for_process(
                    tracked_pid,
                    tracked_identity,
                    root_identity,
                    Some(child),
                )
                .is_ok()
                && let Some(graceful_exit) = wait_for_graceful_tracked_exit(
                    tracked_pid,
                    tracked_identity,
                    root_pid,
                    child,
                    Duration::from_secs(20),
                )?
            {
                exit_code = graceful_exit;
            }
            // The launcher or tracked workload exiting does not establish that
            // its siblings and grandchildren have exited. Keep ownership on error.
            child.finish_process_tree().map_err(|source| {
                RuntimeProcessError::WaitTrackedProcess {
                    pid: tracked_pid,
                    source,
                }
            })?;
            *child_slot = None;
            return Ok(exit_code);
        }
        if let Some(exit_code) =
            child
                .try_wait_code()
                .map_err(|source| RuntimeProcessError::WaitTrackedProcess {
                    pid: tracked_pid,
                    source,
                })?
        {
            if tracked_pid != root_pid && process_matches_identity(tracked_pid, tracked_identity)? {
                kill_process_by_pid(tracked_pid, tracked_identity)?;
                *child_slot = None;
                return Ok(None);
            }
            *child_slot = None;
            return Ok(exit_code);
        }

        #[cfg(windows)]
        {
            if request_windows_console_ctrl_c(root_pid, root_identity).is_ok()
                && let Some(exit_code) = wait_for_graceful_tracked_exit(
                    tracked_pid,
                    tracked_identity,
                    root_pid,
                    child,
                    Duration::from_secs(20),
                )?
            {
                *child_slot = None;
                return Ok(exit_code);
            }

            if let Err(error) = kill_process_by_pid(root_pid, root_identity) {
                if child
                    .try_wait_code()
                    .map_err(|source| RuntimeProcessError::WaitTrackedProcess {
                        pid: tracked_pid,
                        source,
                    })?
                    .is_some()
                {
                    *child_slot = None;
                    return Ok(None);
                }
                return Err(error);
            }
            let _ =
                child
                    .wait_code()
                    .map_err(|source| RuntimeProcessError::WaitTrackedProcess {
                        pid: tracked_pid,
                        source,
                    })?;
            *child_slot = None;
            return Ok(None);
        }

        #[cfg(not(windows))]
        {
            child
                .kill()
                .map_err(|source| RuntimeProcessError::KillTrackedProcess {
                    pid: tracked_pid,
                    source,
                })?;
            let _ =
                child
                    .wait_code()
                    .map_err(|source| RuntimeProcessError::WaitTrackedProcess {
                        pid: tracked_pid,
                        source,
                    })?;
            *child_slot = None;
            return Ok(None);
        }
    }

    if process_matches_identity(tracked_pid, tracked_identity)? {
        kill_process_by_pid(tracked_pid, tracked_identity)?;
    }

    Ok(None)
}

pub(super) fn request_console_interrupt_for_process(
    tracked_pid: u32,
    tracked_identity: &ProcessIdentity,
    root_identity: &ProcessIdentity,
    child: Option<&mut RuntimeChild>,
) -> Result<(), RuntimeProcessError> {
    #[cfg(windows)]
    {
        let mut child = child;
        if let Some(RuntimeChild::Windows(native)) = child.as_deref_mut()
            && let Some(elevated) = &mut native.elevated
        {
            return elevated
                .request_console_interrupt(native.process_handle)
                .map_err(|source| RuntimeProcessError::ConsoleInterruptProcess {
                    pid: tracked_pid,
                    operation: "elevated owned console control",
                    source,
                });
        }
        let (pid, identity) =
            console_interrupt_target(tracked_pid, tracked_identity, root_identity, child)?;
        request_windows_console_ctrl_c(pid, identity)
    }

    #[cfg(not(windows))]
    {
        let _ = (tracked_identity, root_identity, child);
        Err(RuntimeProcessError::ConsoleInterruptProcess {
            pid: tracked_pid,
            operation: "platform support check",
            source: std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "console interrupt is only implemented for Windows managed consoles",
            ),
        })
    }
}

#[cfg(windows)]
pub(super) fn console_interrupt_target<'a>(
    tracked_pid: u32,
    tracked_identity: &'a ProcessIdentity,
    root_identity: &'a ProcessIdentity,
    child: Option<&mut RuntimeChild>,
) -> Result<(u32, &'a ProcessIdentity), RuntimeProcessError> {
    if let Some(child) = child
        && child
            .try_wait()
            .map_err(|source| RuntimeProcessError::WaitTrackedProcess {
                pid: tracked_pid,
                source,
            })?
            .is_none()
    {
        return Ok((child.id(), root_identity));
    }
    Ok((tracked_pid, tracked_identity))
}

#[cfg(windows)]
pub(super) fn wait_for_graceful_tracked_exit(
    tracked_pid: u32,
    tracked_identity: &ProcessIdentity,
    root_pid: u32,
    child: &mut RuntimeChild,
    timeout: Duration,
) -> Result<Option<Option<i32>>, RuntimeProcessError> {
    let started_at = Instant::now();
    loop {
        if let Some(exit_code) =
            child
                .try_wait_code()
                .map_err(|source| RuntimeProcessError::WaitTrackedProcess {
                    pid: tracked_pid,
                    source,
                })?
        {
            if tracked_pid != root_pid && process_matches_identity(tracked_pid, tracked_identity)? {
                if !child.has_owned_process_tree() {
                    kill_process_by_pid(tracked_pid, tracked_identity)?;
                    return Ok(Some(None));
                }
            } else {
                return Ok(Some(exit_code));
            }
        }

        // The owned child handle remains valid throughout exit. Reopening its PID
        // here can fail while Windows is tearing the process down.
        if tracked_pid != root_pid && !process_matches_identity(tracked_pid, tracked_identity)? {
            return Ok(Some(None));
        }

        if started_at.elapsed() >= timeout {
            return Ok(None);
        }

        std::thread::sleep(Duration::from_millis(250));
    }
}

fn select_primary_managed_process(processes: &[ManagedProcess]) -> Option<&ManagedProcess> {
    processes
        .iter()
        .find(|process| process.is_primary)
        .or_else(|| {
            processes
                .iter()
                .find(|process| process.process_key.eq_ignore_ascii_case("master"))
        })
        .or_else(|| {
            processes
                .iter()
                .find(|process| process.process_key.eq_ignore_ascii_case("main"))
        })
        .or_else(|| processes.first())
}

fn select_primary_managed_process_index(processes: &[ManagedProcess]) -> Option<usize> {
    processes
        .iter()
        .position(|process| process.is_primary)
        .or_else(|| {
            processes
                .iter()
                .position(|process| process.process_key.eq_ignore_ascii_case("master"))
        })
        .or_else(|| {
            processes
                .iter()
                .position(|process| process.process_key.eq_ignore_ascii_case("main"))
        })
        .or_else(|| (!processes.is_empty()).then_some(0))
}

fn find_managed_process_index(processes: &[ManagedProcess], process_key: &str) -> Option<usize> {
    processes
        .iter()
        .position(|process| process.process_key.eq_ignore_ascii_case(process_key))
}

fn select_requested_managed_process_index(
    processes: &[ManagedProcess],
    process_key: Option<&str>,
) -> Option<usize> {
    match process_key.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => find_managed_process_index(processes, value),
        None => select_primary_managed_process_index(processes),
    }
}
