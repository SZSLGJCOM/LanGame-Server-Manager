use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs::{self, File};
#[cfg(not(windows))]
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, UdpSocket};
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, FromRawHandle};
#[cfg(windows)]
use std::os::windows::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ExitStatus};
#[cfg(any(not(windows), test))]
use std::process::{Command, Stdio};
#[cfg(windows)]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
#[cfg(windows)]
use std::time::Duration;

use app_core::{
    AppSettings, InstanceDetails, LaunchPlan, LaunchValidationIssue, ModuleDetails, PortBinding,
    ProcessHostSurface, ProcessIdentity, ProcessWindowPolicy, RuntimePerformanceApplication,
    RuntimePerformancePolicy, RuntimePerformancePolicyPreview, RuntimePriorityClass,
};
use serde_json::Value;
use thiserror::Error;

#[cfg(windows)]
mod elevated_launcher;
pub mod native_player_console;
#[cfg(windows)]
pub use elevated_launcher::run_elevated_launcher_if_requested;
mod runtime_launch;
pub use runtime_launch::{
    spawn_launch_plan, spawn_launch_plan_in_resource_group, spawn_launch_plan_with_log_writer,
};
mod resource_admission;
pub use resource_admission::{
    RuntimeResourceAdmission, RuntimeResourceAdmissionSnapshot, RuntimeResourceGroup,
};
mod spawned_process_lifecycle;
mod startup_console;
#[cfg(all(test, windows))]
mod windows_resource_limits_tests;
pub use spawned_process_lifecycle::{
    stabilize_spawned_process, stop_spawned_process, stop_spawned_process_gracefully,
    stop_spawned_unreal_process_gracefully,
};
pub use startup_console::StartupConsoleWriter;
mod stdin_write;
pub use stdin_write::RuntimeStdinCancellation;
#[cfg(windows)]
use stdin_write::WindowsPipeWriter;

#[path = "launch_templates.rs"]
mod launch_templates;
use launch_templates::*;
#[path = "performance_policy.rs"]
mod performance_policy;
#[cfg(windows)]
mod pseudo_console;
#[cfg(any(windows, test))]
mod pseudo_console_transcript;
use performance_policy::resolve_runtime_performance_resolution_for_instance;
mod port_remap;
mod process_environment;
#[cfg(windows)]
mod process_exit_target;
pub use port_remap::{remap_taken_port_bindings, remap_taken_port_bindings_for_module};
#[cfg(windows)]
pub use process_exit_target::ProcessExitTarget;
#[cfg(any(windows, test))]
mod romestead_runtime;
#[cfg(windows)]
mod windows_console_control;
#[cfg(windows)]
mod windows_exit_watchdog;
#[cfg(windows)]
mod windows_unreal_console;
#[cfg(windows)]
mod windows_window_close;
#[cfg(windows)]
use windows_console_control::request_windows_console_ctrl_c;
#[cfg(windows)]
pub use windows_exit_watchdog::{ExitWatchdogOutcome, spawn_exit_watchdog};
#[cfg(windows)]
mod windows_process_identity;
#[cfg(windows)]
mod windows_process_job;
#[cfg(windows)]
mod windows_process_output;
#[cfg(windows)]
mod windows_process_spawn;
#[cfg(windows)]
mod windows_utility;
#[cfg(test)]
use performance_policy::{
    available_logical_cpu_count, balanced_instance_affinity_mask, half_affinity_mask,
};
pub use performance_policy::{
    resolve_runtime_performance_policy, resolve_runtime_performance_policy_for_instance,
    resolve_runtime_performance_policy_with_preview_for_instance,
};
use process_environment::{SpawnCommand, validate_environment};
#[cfg(windows)]
use windows_process_identity::{
    inspect_windows_process_identity, query_windows_process_identity_from_handle,
};
#[cfg(windows)]
use windows_process_spawn::{spawn_hidden_desktop_process, spawn_standard_process};
#[cfg(windows)]
pub use windows_utility::{capture_windows_utility, windows_system_directory};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;
#[cfg(windows)]
const DETACHED_PROCESS: u32 = 0x00000008;
#[cfg(windows)]
const CREATE_NEW_CONSOLE: u32 = 0x00000010;
#[cfg(windows)]
const CREATE_UNICODE_ENVIRONMENT: u32 = 0x00000400;
#[cfg(windows)]
const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x00080000;
#[cfg(windows)]
const CTRL_C_EVENT: u32 = 0;
#[cfg(windows)]
const STARTF_USESHOWWINDOW: u32 = 0x00000001;
#[cfg(windows)]
const STARTF_USESTDHANDLES: u32 = 0x00000100;
#[cfg(windows)]
const SW_HIDE: u16 = 0;
#[cfg(windows)]
const WAIT_OBJECT_0: u32 = 0x00000000;
#[cfg(windows)]
const WAIT_TIMEOUT: u32 = 0x00000102;
#[cfg(windows)]
const PROCESS_EXIT_SETTLE_TIMEOUT_MS: u32 = 1500;
#[cfg(windows)]
const INFINITE: u32 = 0xFFFFFFFF;
#[cfg(windows)]
const HANDLE_FLAG_INHERIT: u32 = 0x00000001;
#[cfg(windows)]
const DUPLICATE_SAME_ACCESS: u32 = 0x00000002;
#[cfg(windows)]
const PROC_THREAD_ATTRIBUTE_HANDLE_LIST: usize = 0x00020002;
#[cfg(windows)]
const PROCESS_TERMINATE: u32 = 0x0001;
#[cfg(windows)]
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
#[cfg(windows)]
const PROCESS_SET_INFORMATION: u32 = 0x0200;
#[cfg(windows)]
const SYNCHRONIZE: u32 = 0x00100000;
#[cfg(windows)]
const STILL_ACTIVE: u32 = 259;
#[cfg(windows)]
const ERROR_INVALID_PARAMETER: i32 = 87;
#[cfg(windows)]
const ERROR_ACCESS_DENIED: i32 = 5;
#[cfg(windows)]
const ERROR_INSUFFICIENT_BUFFER: i32 = 122;
#[cfg(windows)]
const SEE_MASK_NOCLOSEPROCESS: u32 = 0x00000040;
#[cfg(windows)]
const MAX_PATH_WIDE: usize = 260;
#[cfg(windows)]
const MAX_PROCESS_IMAGE_PATH_WIDE: usize = 32_768;
#[cfg(windows)]
const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
#[cfg(windows)]
const IDLE_PRIORITY_CLASS: u32 = 0x0000_0040;
#[cfg(windows)]
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
#[cfg(windows)]
const NORMAL_PRIORITY_CLASS: u32 = 0x0000_0020;
#[cfg(windows)]
const ABOVE_NORMAL_PRIORITY_CLASS: u32 = 0x0000_8000;
#[cfg(windows)]
const HIGH_PRIORITY_CLASS: u32 = 0x0000_0080;
#[cfg(windows)]
const DESKTOP_READOBJECTS: u32 = 0x0001;
#[cfg(windows)]
const DESKTOP_CREATEWINDOW: u32 = 0x0002;
#[cfg(windows)]
const DESKTOP_CREATEMENU: u32 = 0x0004;
#[cfg(windows)]
const DESKTOP_HOOKCONTROL: u32 = 0x0008;
#[cfg(windows)]
const DESKTOP_ENUMERATE: u32 = 0x0040;
#[cfg(windows)]
const DESKTOP_WRITEOBJECTS: u32 = 0x0080;
#[cfg(windows)]
const HIDDEN_DESKTOP_ACCESS: u32 = DESKTOP_READOBJECTS
    | DESKTOP_CREATEWINDOW
    | DESKTOP_CREATEMENU
    | DESKTOP_HOOKCONTROL
    | DESKTOP_ENUMERATE
    | DESKTOP_WRITEOBJECTS;
#[cfg(windows)]
static HIDDEN_DESKTOP_COUNTER: AtomicU64 = AtomicU64::new(1);

#[path = "runtime_supervisor.rs"]
mod runtime_supervisor;
pub use runtime_supervisor::{
    ExitedManagedProcess, ManagedInstance, ManagedProcess, RuntimeCommandDispatchLease,
    RuntimeCommandDispatchTarget, RuntimeCommandSubmissionTracker, RuntimePerformanceRefresh,
    RuntimeSupervisor, StoppedManagedProcess, TrackedInstance,
};
use runtime_supervisor::{RuntimePerformanceResolution, stop_tracked_process};

#[derive(Debug, Error)]
pub enum LaunchPlanError {
    #[error("module `{module_id}` does not declare an [install] section")]
    MissingInstallSpec { module_id: String },
    #[error("module `{module_id}` does not declare a [process] section")]
    MissingProcessSpec { module_id: String },
    #[error("instance settings_json is not valid JSON: {0}")]
    InvalidSettingsJson(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum RuntimeProcessError {
    #[error("launch executable does not exist: {path}")]
    MissingExecutable { path: String },
    #[error("failed to create log directory {path}: {source}")]
    CreateLogDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to open log file {path}: {source}")]
    OpenLogFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to spawn process {path}: {source}")]
    SpawnProcess {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to stop tracked process {pid}: {source}")]
    KillTrackedProcess {
        pid: u32,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to wait for tracked process {pid}: {source}")]
    WaitTrackedProcess {
        pid: u32,
        #[source]
        source: std::io::Error,
    },
    #[error("launch failed ({launch_error}); cleanup also failed: {source}")]
    FailedLaunchCleanup {
        launch_error: Box<RuntimeProcessError>,
        #[source]
        source: std::io::Error,
    },
    #[error("tracked instance `{instance_id}` is not running")]
    TrackedInstanceNotFound { instance_id: String },
    #[error("tracked instance `{instance_id}` no longer owns run {expected_run_id}")]
    TrackedInstanceRunChanged {
        instance_id: String,
        expected_run_id: i64,
    },
    #[error("tracked process `{process_key}` is not available for instance `{instance_id}`")]
    TrackedProcessNotFound {
        instance_id: String,
        process_key: String,
    },
    #[error("tracked process {pid} does not expose stdin")]
    MissingTrackedProcessStdin { pid: u32 },
    #[error("failed to write to tracked process {pid}: {source}")]
    WriteTrackedProcessStdin {
        pid: u32,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "failed to request console interrupt for tracked process {pid} during {operation}: {source}"
    )]
    ConsoleInterruptProcess {
        pid: u32,
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "failed to request window close for tracked process {pid} during {operation}: {source}"
    )]
    WindowCloseProcess {
        pid: u32,
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to stop external process {pid}: {source}")]
    KillByPid {
        pid: u32,
        #[source]
        source: std::io::Error,
    },
    #[error("external stop command failed for pid {pid}")]
    KillByPidFailed { pid: u32 },
    #[error("failed to inspect external process {pid}: {source}")]
    InspectProcess {
        pid: u32,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to interpret process inspection output for pid {pid}: {message}")]
    InspectProcessOutput { pid: u32, message: String },
    #[error("process {pid} exited before its identity could be recorded")]
    ProcessIdentityUnavailable { pid: u32 },
    #[error("elevated launcher {pid} did not create a workload for {path} within 30 seconds")]
    WorkloadStartupTimedOut { pid: u32, path: String },
    #[error("refused to control pid {pid}: its process identity does not match the recorded owner")]
    ProcessIdentityMismatch { pid: u32 },
}

pub struct SpawnedProcess {
    pub child: Option<RuntimeChild>,
    pub pid: u32,
    pub process_identity: ProcessIdentity,
    pub root_process_identity: ProcessIdentity,
    pub log_path: String,
    pub uses_script_entrypoint: bool,
    #[cfg(windows)]
    requires_workload_handoff: bool,
    pub hidden_desktop: Option<WindowsHiddenDesktop>,
}

#[derive(Debug)]
pub struct WindowsHiddenDesktop {
    #[cfg(windows)]
    handle: usize,
    pub name: String,
}

#[cfg(windows)]
unsafe impl Send for WindowsHiddenDesktop {}

#[cfg(windows)]
impl Drop for WindowsHiddenDesktop {
    fn drop(&mut self) {
        if self.handle != 0 {
            unsafe {
                let _ = CloseDesktop(self.handle as *mut std::ffi::c_void);
            }
        }
    }
}

#[derive(Debug)]
pub enum RuntimeChild {
    Standard(Child),
    #[cfg(windows)]
    Windows(WindowsSpawnedChild),
}

#[derive(Debug)]
enum RuntimeStdin {
    #[cfg(not(windows))]
    Standard(ChildStdin),
    #[cfg(windows)]
    Standard(WindowsPipeWriter<ChildStdin>),
    #[cfg(windows)]
    Windows(WindowsPipeWriter<File>),
    #[cfg(windows)]
    PseudoConsole(pseudo_console::PseudoConsoleInput),
}

impl RuntimeStdin {
    #[cfg(test)]
    fn write_stdin_line(&mut self, command: &str) -> Result<(), std::io::Error> {
        self.write_stdin_line_with_cancellation(command, &RuntimeStdinCancellation::default())
    }

    fn write_stdin_line_with_cancellation(
        &mut self,
        command: &str,
        cancellation: &RuntimeStdinCancellation,
    ) -> Result<(), std::io::Error> {
        #[cfg(windows)]
        return match self {
            RuntimeStdin::Standard(stdin) => stdin.write_line(command, cancellation),
            RuntimeStdin::Windows(stdin) => stdin.write_line(command, cancellation),
            RuntimeStdin::PseudoConsole(stdin) => {
                stdin.write_line_with_cancellation(command, cancellation)
            }
        };
        #[cfg(not(windows))]
        {
            if cancellation.is_cancelled() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "The stdin write was cancelled.",
                ));
            }
            let writer: &mut dyn Write = match self {
                RuntimeStdin::Standard(stdin) => stdin,
            };
            writer
                .write_all(command.as_bytes())
                .and_then(|_| writer.write_all(b"\n"))
                .and_then(|_| writer.flush())
        }
    }
}

impl RuntimeChild {
    #[cfg(not(windows))]
    pub(crate) fn owned_process_tree_is_running(&mut self) -> std::io::Result<Option<bool>> {
        // A root process handle cannot establish whether its descendants exited.
        Ok(None)
    }

    #[cfg(not(windows))]
    pub(crate) fn has_owned_process_tree(&self) -> bool {
        false
    }

    #[cfg(not(windows))]
    pub fn finish_process_tree(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    fn id(&self) -> u32 {
        match self {
            RuntimeChild::Standard(child) => child.id(),
            #[cfg(windows)]
            RuntimeChild::Windows(child) => child.process_id,
        }
    }

    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, std::io::Error> {
        match self {
            RuntimeChild::Standard(child) => child.try_wait(),
            #[cfg(windows)]
            RuntimeChild::Windows(child) => child.try_wait(),
        }
    }

    fn try_wait_code(&mut self) -> Result<Option<Option<i32>>, std::io::Error> {
        self.try_wait()
            .map(|status| status.map(|status| status.code()))
    }

    fn wait_code(&mut self) -> Result<Option<i32>, std::io::Error> {
        match self {
            RuntimeChild::Standard(child) => child.wait().map(|status| status.code()),
            #[cfg(windows)]
            RuntimeChild::Windows(child) => child.wait_code(),
        }
    }

    fn terminate_owned(&mut self) -> Result<(), std::io::Error> {
        match self {
            RuntimeChild::Standard(child) => child.kill(),
            #[cfg(windows)]
            RuntimeChild::Windows(child) => {
                if let Some(elevated) = &mut child.elevated {
                    return elevated.finish(child.process_handle);
                }
                if let Some(job) = &child.job {
                    return job.terminate_and_wait();
                }
                if unsafe { TerminateProcess(child.process_handle as *mut std::ffi::c_void, 1) }
                    == 0
                {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            }
        }
    }

    #[cfg(not(windows))]
    fn kill(&mut self) -> Result<(), std::io::Error> {
        match self {
            RuntimeChild::Standard(child) => child.kill(),
        }
    }

    fn take_stdin(&mut self) -> Option<RuntimeStdin> {
        match self {
            #[cfg(not(windows))]
            RuntimeChild::Standard(child) => child.stdin.take().map(RuntimeStdin::Standard),
            #[cfg(windows)]
            RuntimeChild::Standard(child) => child
                .stdin
                .take()
                .map(WindowsPipeWriter::new)
                .map(RuntimeStdin::Standard),
            #[cfg(windows)]
            RuntimeChild::Windows(child) => child.stdin.take(),
        }
    }

    fn restore_stdin(&mut self, stdin: RuntimeStdin) {
        match (self, stdin) {
            (RuntimeChild::Standard(child), RuntimeStdin::Standard(stdin)) => {
                if child.stdin.is_none() {
                    #[cfg(not(windows))]
                    {
                        child.stdin = Some(stdin);
                    }
                    #[cfg(windows)]
                    {
                        child.stdin = stdin.into_healthy_pipe();
                    }
                }
            }
            #[cfg(windows)]
            (
                RuntimeChild::Windows(child),
                stdin @ (RuntimeStdin::Windows(_) | RuntimeStdin::PseudoConsole(_)),
            ) if child.stdin.is_none() => {
                child.stdin = Some(stdin);
            }
            #[cfg(windows)]
            _ => {}
        }
    }
}

#[cfg(windows)]
#[derive(Debug)]
pub struct WindowsSpawnedChild {
    process_handle: usize,
    process_id: u32,
    stdin: Option<RuntimeStdin>,
    terminal: Option<pseudo_console::ManagedPseudoConsole>,
    job: Option<windows_process_job::OwnedProcessJob>,
    elevated: Option<elevated_launcher::ElevatedGuard>,
    output: Option<windows_process_output::ManagedProcessOutput>,
}

#[cfg(windows)]
unsafe impl Send for WindowsSpawnedChild {}

#[cfg(windows)]
impl WindowsSpawnedChild {
    fn try_wait(&self) -> Result<Option<ExitStatus>, std::io::Error> {
        match unsafe { WaitForSingleObject(self.process_handle as *mut std::ffi::c_void, 0) } {
            WAIT_TIMEOUT => Ok(None),
            WAIT_OBJECT_0 => self
                .exit_code()
                .map(|code| Some(ExitStatus::from_raw(code as u32))),
            _ => Err(std::io::Error::last_os_error()),
        }
    }

    fn wait_code(&self) -> Result<Option<i32>, std::io::Error> {
        match unsafe { WaitForSingleObject(self.process_handle as *mut std::ffi::c_void, INFINITE) }
        {
            WAIT_OBJECT_0 => self.exit_code().map(Some),
            _ => Err(std::io::Error::last_os_error()),
        }
    }

    fn exit_code(&self) -> Result<i32, std::io::Error> {
        let mut exit_code = 0u32;
        if unsafe {
            GetExitCodeProcess(self.process_handle as *mut std::ffi::c_void, &mut exit_code)
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(exit_code as i32)
    }
}

#[cfg(windows)]
impl Drop for WindowsSpawnedChild {
    fn drop(&mut self) {
        // End the private launch tree before closing terminal endpoints.
        self.job.take();
        self.elevated.take();
        self.stdin.take();
        self.terminal.take();
        self.output.take();
        if self.process_handle != 0 {
            unsafe {
                let _ = CloseHandle(self.process_handle as *mut std::ffi::c_void);
            }
        }
    }
}

pub fn build_launch_plan(
    settings: &AppSettings,
    module: &ModuleDetails,
    instance: &InstanceDetails,
) -> Result<LaunchPlan, LaunchPlanError> {
    build_launch_plan_with_override(settings, module, instance, None)
}

pub fn build_launch_plan_with_override(
    settings: &AppSettings,
    module: &ModuleDetails,
    instance: &InstanceDetails,
    install_root_override: Option<&str>,
) -> Result<LaunchPlan, LaunchPlanError> {
    let install = module
        .install
        .as_ref()
        .ok_or_else(|| LaunchPlanError::MissingInstallSpec {
            module_id: module.summary.id.clone(),
        })?;
    let process = module
        .process
        .as_ref()
        .ok_or_else(|| LaunchPlanError::MissingProcessSpec {
            module_id: module.summary.id.clone(),
        })?;

    let install_root = install_root_override
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&settings.games_root).join(&install.shared_game_dir));

    let config_file_path = PathBuf::from(&instance.config_file_path);
    let config_dir = config_file_path
        .parent()
        .unwrap_or(config_file_path.as_path())
        .to_path_buf();
    let instance_root = config_dir
        .parent()
        .unwrap_or(config_dir.as_path())
        .to_path_buf();
    let data_dir = instance_root.join("data");
    let logs_dir = instance_root.join("logs");
    let saves_dir = PathBuf::from(&instance.saves_path);

    let settings_value: Value = serde_json::from_str(&instance.settings_json)?;
    let performance_resolution = resolve_runtime_performance_resolution_for_instance(
        &module.runtime.performance,
        &settings_value,
        &instance.summary.id,
    );
    let context = TemplateContext {
        instance,
        settings: &settings_value,
        install_root: &install_root,
        config_dir: &config_dir,
        data_dir: &data_dir,
        logs_dir: &logs_dir,
        saves_dir: &saves_dir,
    };

    let configured_executable = resolve_template(&process.executable, &context);
    let executable_path = resolve_process_executable(&install_root, &configured_executable);
    #[cfg(windows)]
    let executable_path = compatible_java_executable_path(executable_path);
    #[cfg(windows)]
    let executable_path = compatible_native_path(executable_path);

    let args = process
        .args_template
        .iter()
        .flat_map(|segment| expand_resolved_argument_segments(segment, &context))
        .collect::<Vec<_>>();
    #[cfg(windows)]
    let args = {
        let mut args = args;
        for index in 1..args.len() {
            if args[index - 1].eq_ignore_ascii_case("-jar")
                && args[index].starts_with(r"\\?\")
                && args[index].contains('/')
            {
                // Verbatim paths need native separators in both preflight and Java's actual argument.
                args[index] = args[index].replace('/', "\\");
            }
        }
        args
    };
    let environment = process
        .environment_template
        .iter()
        .map(|(key, value)| (key.clone(), resolve_template(value, &context)))
        .collect::<BTreeMap<_, _>>();
    #[cfg(any(windows, test))]
    let environment = {
        let mut environment = environment;
        romestead_runtime::apply_portable_runtime(
            settings,
            &module.summary.id,
            &install_root,
            &mut environment,
        );
        environment
    };

    let default_working_directory = executable_path
        .parent()
        .unwrap_or(install_root.as_path())
        .to_path_buf();
    let working_directory = process
        .working_directory_template
        .as_deref()
        .map(|template| resolve_template(template, &context))
        .filter(|resolved| {
            let trimmed = resolved.trim();
            !trimmed.is_empty() && !trimmed.contains("{{") && !trimmed.contains("}}")
        })
        .map(rendered_path)
        .unwrap_or_else(|| default_working_directory.clone());
    #[cfg(windows)]
    let working_directory = compatible_native_path(working_directory);

    let mut validation_issues = collect_launch_validation_issues(
        &instance.summary.bind_ip,
        &instance.ports,
        &install_root,
        &config_dir,
        &working_directory,
        &executable_path,
        &args,
    );
    #[cfg(windows)]
    if working_directory.is_dir()
        && native_working_directory_is_incompatible(
            &module.summary.id,
            &executable_path,
            &args,
            &working_directory,
        )
    {
        validation_issues.push(LaunchValidationIssue {
            code: String::from("launch_working_directory_incompatible"),
            context: BTreeMap::new(),
            severity: String::from("error"),
            message: String::from(
                "This native entrypoint cannot use the selected Windows working directory. Use a short local path without special directory names.",
            ),
            path: Some(working_directory.to_string_lossy().into_owned()),
        });
    }
    if can_prepare_launch_executable(&module.summary.id, &context, &executable_path) {
        for issue in &mut validation_issues {
            if issue.code == "launch_executable_missing" {
                issue.code = String::from("launch_preparation_required");
                issue.severity = String::from("info");
                issue.message = String::from(
                    "The installed server files are available. The instance launch program will be prepared before startup.",
                );
            }
        }
    }
    validation_issues.extend(collect_module_launch_setting_issues(
        &module.summary.id,
        &settings_value,
        &instance.ports,
    ));
    if let Err(error) = validate_environment(&environment, module.runtime.requires_admin) {
        validation_issues.push(LaunchValidationIssue {
            code: String::from("launch_environment_invalid"),
            context: BTreeMap::new(),
            severity: String::from("error"),
            message: error.to_string(),
            path: None,
        });
    }
    let resource_validation =
        settings_value
            .pointer("/runtime_performance/resource_limits")
            .map(|value| {
                serde_json::from_value::<app_core::RuntimeResourceLimits>(value.clone())
                    .map_err(|error| error.to_string())
                    .and_then(|limits| {
                        limits.validate().and_then(|()| {
                if limits.enabled() && (module.runtime.requires_admin || !cfg!(windows)) {
                    Err("Instance resource limits require a non-elevated Windows launch".into())
                } else { Ok(()) }
            })
                    })
            });
    if let Some(Err(message)) = resource_validation {
        validation_issues.push(LaunchValidationIssue {
            code: String::from("runtime_resource_limits_invalid"),
            context: BTreeMap::new(),
            severity: String::from("error"),
            message,
            path: None,
        });
    }
    let ready_to_launch = !validation_issues
        .iter()
        .any(|issue| issue.severity.eq_ignore_ascii_case("error"));
    let uses_script_entrypoint = is_script_entrypoint(&executable_path, &args);
    let requires_admin = module.runtime.requires_admin;

    Ok(LaunchPlan {
        instance_id: instance.summary.id.clone(),
        instance_name: instance.summary.name.clone(),
        module_id: instance.summary.module_id.clone(),
        install_root: install_root.to_string_lossy().into_owned(),
        install_state: module.summary.install_state.clone(),
        uses_private_runtime: false,
        working_directory: working_directory.to_string_lossy().into_owned(),
        executable_path: executable_path.to_string_lossy().into_owned(),
        executable_exists: executable_path.exists(),
        ready_to_launch,
        validation_issues,
        command_line: build_command_line(&executable_path, &args),
        args,
        environment,
        window_policy: process.window_policy.clone(),
        uses_script_entrypoint,
        requires_admin,
        host_surface: process.host_surface.clone(),
        host_notes: process.host_notes.clone(),
        performance_policy: performance_resolution.policy,
        performance_preview: performance_resolution.preview,
    })
}

fn collect_module_launch_setting_issues(
    module_id: &str,
    settings: &Value,
    ports: &[PortBinding],
) -> Vec<LaunchValidationIssue> {
    if matches!(module_id, "arksurvivalascended" | "arksurvivalevolved") {
        return collect_ark_cluster_launch_issues(settings);
    }
    if module_id == "dontstarve" {
        return collect_dontstarve_launch_setting_issues(settings, ports);
    }
    let (field, managed_options): (&str, &[&str]) = match module_id {
        "rust" => (
            "custom_launch_flags",
            &[
                "-batchmode",
                "-nographics",
                "-logfile",
                "+server.ip",
                "+server.identity",
                "+world.configfile",
                "+server.port",
                "+server.queryport",
                "+rcon.port",
                "+rcon.password",
                "+rcon.web",
            ],
        ),
        "nightingale" => (
            "extra_launch_args",
            &[
                "-port",
                "-multihome",
                "-enablecheats",
                "-statusport",
                "-ini:engine:[/script/engine.gamesession]:maxplayers",
                "-ini:engine:[httpserver.listeners]:+listeneroverrides",
                "-ini:engine:[jsonlogger]:benable",
                "-ini:engine:[jsonlogger]:bstdout",
                "-noconsole",
            ],
        ),
        "runescapedragonwilds" => ("extra_launch_args", &["-port", "-queryport"]),
        "satisfactory" => (
            "custom_launch_flags",
            &[
                "-userdir",
                "-port",
                "-reliableport",
                "-externalreliableport",
                "-disablepacketrouting",
                "-disableseasonalevents",
                "-ini:engine:[systemsettings]:fg.dedicatedserver.allowinsecurelocalaccess",
            ],
        ),
        "theforest" => (
            "extra_launch_args",
            &[
                "-serverip",
                "-serversteamport",
                "-servergameport",
                "-serverqueryport",
                "-servername",
                "-serverplayers",
                "-serverpassword",
                "-serverpassword_admin",
                "-serversteamaccount",
                "-enablevac",
                "-serverautosaveinterval",
                "-difficulty",
                "-inittype",
                "-slot",
                "-showlogs",
                "-veganmode",
                "-vegetarianmode",
                "-resetholesmode",
                "-treeregrowmode",
                "-nobuildingdestruction",
                "-allowenemiescreative",
                "-allowcheats",
                "-configfilepath",
                "-savefolderpath",
                "-realisticplayerdamage",
                "-batchmode",
                "-nographics",
                "-nosteamclient",
                "-dedicated",
            ],
        ),
        _ => return Vec::new(),
    };
    let Some(raw) = settings.get(field).and_then(Value::as_str) else {
        return Vec::new();
    };
    if !custom_launch_flags_contain_managed_option(raw, managed_options) {
        return Vec::new();
    }
    vec![LaunchValidationIssue {
        code: String::from("managed_launch_option_conflict"),
        context: BTreeMap::from([(String::from("field"), String::from(field))]),
        severity: String::from("error"),
        message: format!(
            "{field} repeats a launch option already owned by a typed setting or managed port. Remove the duplicate raw option before launch."
        ),
        path: None,
    }]
}

fn collect_launch_validation_issues(
    bind_ip: &str,
    ports: &[PortBinding],
    install_root: &Path,
    config_dir: &Path,
    working_directory: &Path,
    executable_path: &Path,
    args: &[String],
) -> Vec<LaunchValidationIssue> {
    let mut issues = Vec::new();

    if !install_root.exists() {
        issues.push(LaunchValidationIssue {
            code: String::from("install_root_missing"),
            context: BTreeMap::new(),
            severity: String::from("error"),
            message: format!(
                "Install root does not exist yet: {}. Install or repair the game files first.",
                install_root.display()
            ),
            path: Some(install_root.to_string_lossy().into_owned()),
        });
    }

    if !config_dir.exists() {
        issues.push(LaunchValidationIssue {
            code: String::from("config_dir_missing"),
            context: BTreeMap::new(),
            severity: String::from("error"),
            message: format!(
                "Instance config directory is missing: {}. Save or recreate the instance config before launch.",
                config_dir.display()
            ),
            path: Some(config_dir.to_string_lossy().into_owned()),
        });
    }

    if !working_directory.exists() {
        issues.push(LaunchValidationIssue {
            code: String::from("working_directory_missing"),
            context: BTreeMap::new(),
            severity: String::from("error"),
            message: format!(
                "Working directory does not exist: {}.",
                working_directory.display()
            ),
            path: Some(working_directory.to_string_lossy().into_owned()),
        });
    }

    if !executable_path.exists() {
        let message = format!(
            "Launch executable is missing: {}. Install or repair the game files first.",
            executable_path.display()
        );

        issues.push(LaunchValidationIssue {
            code: String::from("launch_executable_missing"),
            context: BTreeMap::new(),
            severity: String::from("error"),
            message,
            path: Some(executable_path.to_string_lossy().into_owned()),
        });
    }

    issues.extend(collect_port_validation_issues(bind_ip, ports));
    issues.extend(collect_required_argument_file_issues(
        working_directory,
        args,
    ));
    let unresolved_args = args
        .iter()
        .filter(|segment| contains_unresolved_template_token(segment))
        .count();
    if unresolved_args > 0 {
        issues.push(LaunchValidationIssue {
            code: String::from("unresolved_launch_args"),
            context: BTreeMap::from([(String::from("count"), unresolved_args.to_string())]),
            severity: String::from("error"),
            message: format!(
                "{unresolved_args} launch argument segment(s) still contain unresolved template tokens."
            ),
            path: None,
        });
    }

    issues
}

fn collect_required_argument_file_issues(
    working_directory: &Path,
    args: &[String],
) -> Vec<LaunchValidationIssue> {
    let mut issues = Vec::new();

    for index in 0..args.len() {
        if !args[index].eq_ignore_ascii_case("-jar") {
            continue;
        }

        let Some(raw_path) = args.get(index + 1) else {
            issues.push(LaunchValidationIssue {
                code: String::from("launch_required_file_missing"),
                severity: String::from("error"),
                message: String::from("Java launch argument -jar is missing the server jar path."),
                context: BTreeMap::from([(String::from("argument"), String::from("-jar"))]),
                path: None,
            });
            continue;
        };

        if contains_unresolved_template_token(raw_path) {
            continue;
        }

        let jar_path = PathBuf::from(raw_path);
        let resolved_path = if jar_path.is_absolute() {
            jar_path
        } else {
            working_directory.join(jar_path)
        };

        if !resolved_path.exists() {
            issues.push(LaunchValidationIssue {
                code: String::from("launch_required_file_missing"),
                severity: String::from("error"),
                message: format!(
                    "Required Java server jar is missing: {}.",
                    resolved_path.display()
                ),
                context: BTreeMap::new(),
                path: Some(resolved_path.to_string_lossy().into_owned()),
            });
        }
    }

    issues
}

fn collect_port_validation_issues(
    bind_ip: &str,
    ports: &[PortBinding],
) -> Vec<LaunchValidationIssue> {
    if ports.is_empty() {
        return Vec::new();
    }

    let probe_ip = match resolve_port_probe_ip(bind_ip) {
        Ok(ip) => ip,
        Err(message) => {
            return vec![LaunchValidationIssue {
                code: String::from("bind_ip_invalid"),
                context: BTreeMap::from([(String::from("bind_ip"), String::from(bind_ip))]),
                severity: String::from("error"),
                message,
                path: None,
            }];
        }
    };

    ports
        .iter()
        .filter_map(|port| validate_port_binding(probe_ip, port))
        .collect()
}

fn resolve_port_probe_ip(bind_ip: &str) -> Result<IpAddr, String> {
    let normalized = match bind_ip.trim() {
        "" | "0.0.0.0" => return Ok(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
        "[::]" | "::" => return Ok(IpAddr::V6(Ipv6Addr::UNSPECIFIED)),
        value => value,
    };

    normalized.parse::<IpAddr>().map_err(|error| {
        format!(
            "Bind IP '{normalized}' is not a valid local address for launch validation: {error}."
        )
    })
}

fn validate_port_binding(probe_ip: IpAddr, port: &PortBinding) -> Option<LaunchValidationIssue> {
    if port.port == 0 {
        return None;
    }

    let protocol = port.protocol.trim().to_ascii_lowercase();
    let address = SocketAddr::new(probe_ip, port.port);
    let bind_error = match protocol.as_str() {
        "tcp" => TcpListener::bind(address).err(),
        "udp" => UdpSocket::bind(address).err(),
        _ => None,
    }?;

    Some(LaunchValidationIssue {
        code: String::from("port_binding_unavailable"),
        context: BTreeMap::from([
            (String::from("port_name"), port.name.clone()),
            (String::from("protocol"), protocol.to_ascii_uppercase()),
            (String::from("address"), address.to_string()),
        ]),
        severity: String::from("error"),
        message: format!(
            "Port binding '{}' ({}/{}) is not available: {}. Stop the conflicting process or change the bind address / port before launch.",
            port.name,
            protocol.to_ascii_uppercase(),
            address,
            bind_error
        ),
        path: None,
    })
}

fn contains_unresolved_template_token(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.contains("{{") && trimmed.contains("}}")
}

pub fn stop_managed_instance(
    instance: &mut ManagedInstance,
) -> Result<Vec<StoppedManagedProcess>, RuntimeProcessError> {
    let mut stopped = Vec::with_capacity(instance.processes.len());

    for process in &mut instance.processes {
        let exit_code = stop_tracked_process(
            process.pid,
            &process.process_identity,
            &process.root_process_identity,
            &mut process.child,
        )?;

        stopped.push(StoppedManagedProcess {
            run_id: process.run_id,
            process_key: process.process_key.clone(),
            display_name: process.display_name.clone(),
            pid: process.pid,
            log_path: process.log_path.clone(),
            is_primary: process.is_primary,
            exit_code,
        });
    }

    sort_stopped_processes(&mut stopped);
    Ok(stopped)
}

fn sort_stopped_processes(processes: &mut [StoppedManagedProcess]) {
    processes.sort_by(|left, right| {
        right
            .is_primary
            .cmp(&left.is_primary)
            .then_with(|| left.run_id.cmp(&right.run_id))
            .then_with(|| left.process_key.cmp(&right.process_key))
    });
}

pub fn kill_process_by_pid(
    pid: u32,
    expected_identity: &ProcessIdentity,
) -> Result<(), RuntimeProcessError> {
    #[cfg(windows)]
    {
        kill_windows_process_tree(pid, expected_identity)
    }

    #[cfg(not(windows))]
    {
        match inspect_process_identity(pid)? {
            None => return Ok(()),
            Some(actual) if !process_identities_match(expected_identity, &actual) => {
                return Err(RuntimeProcessError::ProcessIdentityMismatch { pid });
            }
            Some(_) => {}
        }
        let status = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .map_err(|source| RuntimeProcessError::KillByPid { pid, source })?;
        if status.success() {
            Ok(())
        } else {
            Err(RuntimeProcessError::KillByPidFailed { pid })
        }
    }
}

pub fn process_matches_identity(
    pid: u32,
    expected_identity: &ProcessIdentity,
) -> Result<bool, RuntimeProcessError> {
    inspect_process_identity(pid).map(|actual| {
        actual
            .as_ref()
            .is_some_and(|actual| process_identities_match(expected_identity, actual))
    })
}

pub fn process_identities_match(expected: &ProcessIdentity, actual: &ProcessIdentity) -> bool {
    expected.creation_time == actual.creation_time
        && normalized_process_image_path(&expected.image_path)
            == normalized_process_image_path(&actual.image_path)
}

pub fn inspect_process_identity(pid: u32) -> Result<Option<ProcessIdentity>, RuntimeProcessError> {
    #[cfg(windows)]
    {
        inspect_windows_process_identity(pid)
    }

    #[cfg(target_os = "linux")]
    {
        inspect_linux_process_identity(pid)
    }

    #[cfg(all(not(windows), not(target_os = "linux")))]
    {
        let _ = pid;
        Err(RuntimeProcessError::InspectProcessOutput {
            pid,
            message: String::from("stable process identity inspection is unsupported on this OS"),
        })
    }
}

fn normalized_process_image_path(path: &str) -> String {
    #[cfg(windows)]
    {
        let normalized = path.replace('/', "\\");
        let normalized = normalized
            .strip_prefix(r"\\?\UNC\")
            .map(|path| format!(r"\\{path}"))
            .or_else(|| normalized.strip_prefix(r"\\?\").map(String::from))
            .unwrap_or(normalized);
        normalized.to_lowercase()
    }

    #[cfg(not(windows))]
    {
        path.to_owned()
    }
}

#[cfg(target_os = "linux")]
fn inspect_linux_process_identity(
    pid: u32,
) -> Result<Option<ProcessIdentity>, RuntimeProcessError> {
    let process_root = PathBuf::from(format!("/proc/{pid}"));
    let stat = match fs::read_to_string(process_root.join("stat")) {
        Ok(stat) => stat,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(RuntimeProcessError::InspectProcess { pid, source }),
    };
    let fields = stat
        .rsplit_once(") ")
        .map(|(_, fields)| fields)
        .ok_or_else(|| RuntimeProcessError::InspectProcessOutput {
            pid,
            message: String::from("/proc stat record does not contain a command terminator"),
        })?;
    let creation_time = fields
        .split_whitespace()
        .nth(19)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| RuntimeProcessError::InspectProcessOutput {
            pid,
            message: String::from("/proc stat record does not contain a valid start time"),
        })?;
    let image_path = fs::read_link(process_root.join("exe"))
        .map_err(|source| RuntimeProcessError::InspectProcess { pid, source })?
        .to_string_lossy()
        .into_owned();
    Ok(Some(ProcessIdentity {
        creation_time,
        image_path,
    }))
}

pub fn process_is_running(pid: u32) -> Result<bool, RuntimeProcessError> {
    #[cfg(windows)]
    {
        windows_process_is_running(pid)
    }

    #[cfg(not(windows))]
    {
        let status = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map_err(|source| RuntimeProcessError::InspectProcess { pid, source })?;
        Ok(status.success())
    }
}

pub fn apply_runtime_performance_policy(
    pid: u32,
    expected_identity: &ProcessIdentity,
    policy: &RuntimePerformancePolicy,
) -> RuntimePerformanceApplication {
    #[cfg(windows)]
    {
        apply_windows_runtime_performance_policy(pid, expected_identity, policy)
    }

    #[cfg(not(windows))]
    {
        let _ = expected_identity;
        RuntimePerformanceApplication {
            pid,
            priority_class: policy.priority_class.clone(),
            cpu_affinity_mask: policy.cpu_affinity_mask,
            apply_to_child_processes: policy.apply_to_child_processes,
            targeted_process_count: 0,
            priority_applied_count: 0,
            affinity_applied_count: 0,
            warnings: vec![String::from(
                "Runtime performance policy is only applied on Windows.",
            )],
        }
    }
}

#[cfg(windows)]
fn apply_windows_runtime_performance_policy(
    pid: u32,
    expected_identity: &ProcessIdentity,
    policy: &RuntimePerformancePolicy,
) -> RuntimePerformanceApplication {
    let targets = if policy.apply_to_child_processes {
        collect_windows_process_tree(pid, expected_identity)
    } else {
        verified_windows_process_root(pid, expected_identity).map(|root| vec![root])
    };
    let (targets, mut warnings) = match targets {
        Ok(targets) => (targets, Vec::new()),
        Err(error) => (
            Vec::new(),
            vec![format!(
                "Failed to verify the runtime process target for pid {pid}: {error}"
            )],
        ),
    };

    let mut priority_applied_count = 0_usize;
    let mut affinity_applied_count = 0_usize;

    for target in &targets {
        let Some(identity) = target.identity.as_ref() else {
            warnings.push(format!(
                "Skipped runtime performance policy for pid {} because its identity was unavailable",
                target.process_id
            ));
            continue;
        };
        match apply_windows_process_performance(target.process_id, identity, policy) {
            Ok(applied) => {
                if applied.priority {
                    priority_applied_count += 1;
                }
                if applied.affinity {
                    affinity_applied_count += 1;
                }
            }
            Err(errors) => {
                for error in errors {
                    warnings.push(format!(
                        "Failed to apply runtime performance policy to pid {}: {}",
                        target.process_id, error
                    ));
                }
            }
        }
    }

    RuntimePerformanceApplication {
        pid,
        priority_class: policy.priority_class.clone(),
        cpu_affinity_mask: policy.cpu_affinity_mask,
        apply_to_child_processes: policy.apply_to_child_processes,
        targeted_process_count: targets.len(),
        priority_applied_count,
        affinity_applied_count,
        warnings,
    }
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
struct WindowsPerformanceApplied {
    priority: bool,
    affinity: bool,
}

#[cfg(windows)]
fn apply_windows_process_performance(
    pid: u32,
    expected_identity: &ProcessIdentity,
    policy: &RuntimePerformancePolicy,
) -> Result<WindowsPerformanceApplied, Vec<String>> {
    let verified_handle =
        match open_verified_windows_process(pid, expected_identity, PROCESS_SET_INFORMATION) {
            Ok(Some(handle)) => handle,
            Ok(None) => return Err(vec![String::from("process is no longer running")]),
            Err(error) => return Err(vec![error.to_string()]),
        };
    let handle = verified_handle.raw;

    let priority_applied =
        unsafe { SetPriorityClass(handle, windows_priority_class(&policy.priority_class)) } != 0;
    let priority_error = (!priority_applied).then(|| std::io::Error::last_os_error().to_string());

    let (affinity_applied, affinity_error) = match policy.cpu_affinity_mask {
        Some(mask) => match cpu_affinity_mask_usize(mask) {
            Some(mask) => {
                let applied = unsafe { SetProcessAffinityMask(handle, mask) } != 0;
                (
                    applied,
                    (!applied).then(|| std::io::Error::last_os_error().to_string()),
                )
            }
            None => (
                false,
                Some(String::from(
                    "CPU affinity mask must be a non-zero value that fits the current process architecture",
                )),
            ),
        },
        None => (false, None),
    };

    let errors = [priority_error, affinity_error]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(WindowsPerformanceApplied {
        priority: priority_applied,
        affinity: affinity_applied,
    })
}

#[cfg(windows)]
fn windows_priority_class(priority: &RuntimePriorityClass) -> u32 {
    match priority {
        RuntimePriorityClass::Idle => IDLE_PRIORITY_CLASS,
        RuntimePriorityClass::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
        RuntimePriorityClass::Normal => NORMAL_PRIORITY_CLASS,
        RuntimePriorityClass::AboveNormal => ABOVE_NORMAL_PRIORITY_CLASS,
        RuntimePriorityClass::High => HIGH_PRIORITY_CLASS,
    }
}

#[cfg(windows)]
fn cpu_affinity_mask_usize(mask: u64) -> Option<usize> {
    if mask == 0 || mask > usize::MAX as u64 {
        None
    } else {
        Some(mask as usize)
    }
}

#[cfg(windows)]
fn kill_windows_process_tree(
    pid: u32,
    expected_identity: &ProcessIdentity,
) -> Result<(), RuntimeProcessError> {
    let Some(root_handle) =
        open_verified_windows_process(pid, expected_identity, PROCESS_TERMINATE)?
    else {
        return Ok(());
    };
    let mut processes = vec![WindowsProcessRecord {
        process_id: pid,
        parent_process_id: 0,
        name: String::new(),
        identity: Some(expected_identity.clone()),
    }];
    let mut first_error = match collect_windows_process_snapshot(pid) {
        Ok(snapshot) => {
            let verified = collect_verified_windows_process_descendants_from_snapshot(
                pid,
                expected_identity,
                &snapshot,
                inspect_windows_process_identity,
            );
            processes.extend(verified.processes);
            verified.inspection_errors.into_iter().next()
        }
        Err(error) => Some(error),
    };
    let by_id = processes
        .iter()
        .map(|record| (record.process_id, record.parent_process_id))
        .collect::<HashMap<_, _>>();

    processes.sort_by(|left, right| {
        windows_descendant_depth(right.process_id, pid, &by_id)
            .cmp(&windows_descendant_depth(left.process_id, pid, &by_id))
            .then_with(|| right.process_id.cmp(&left.process_id))
    });

    for process in processes {
        let stop_result = if process.process_id == pid {
            root_handle.terminate()
        } else if let Some(identity) = process.identity.as_ref() {
            terminate_windows_process(process.process_id, identity)
        } else {
            Ok(())
        };
        if let Err(error) = stop_result {
            if should_ignore_windows_process_termination_error(&process, pid, &error) {
                continue;
            }
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }

    if let Some(error) = first_error {
        // A console-triggered exit can race with TerminateProcess and briefly leave
        // the verified handle unsignaled after Windows reports access denied.
        return resolve_windows_process_tree_stop_result(error, || {
            root_handle.wait_for_exit(PROCESS_EXIT_SETTLE_TIMEOUT_MS)
        });
    }

    Ok(())
}

#[cfg(windows)]
fn resolve_windows_process_tree_stop_result(
    error: RuntimeProcessError,
    wait_for_root_exit: impl FnOnce() -> Result<bool, RuntimeProcessError>,
) -> Result<(), RuntimeProcessError> {
    if wait_for_root_exit()? {
        Ok(())
    } else {
        Err(error)
    }
}

#[cfg(windows)]
fn should_ignore_windows_process_termination_error(
    process: &WindowsProcessRecord,
    root_pid: u32,
    error: &RuntimeProcessError,
) -> bool {
    if process.process_id == root_pid || !is_windows_shell_support_process_name(&process.name) {
        return false;
    }

    matches!(
        error,
        RuntimeProcessError::KillByPid { source, .. }
            if source.raw_os_error() == Some(ERROR_ACCESS_DENIED)
    )
}

#[cfg(windows)]
fn terminate_windows_process(
    pid: u32,
    expected_identity: &ProcessIdentity,
) -> Result<(), RuntimeProcessError> {
    let Some(handle) = open_verified_windows_process(pid, expected_identity, PROCESS_TERMINATE)?
    else {
        return Ok(());
    };
    handle.terminate()
}

#[cfg(windows)]
fn windows_process_is_running(pid: u32) -> Result<bool, RuntimeProcessError> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        let source = std::io::Error::last_os_error();
        return match source.raw_os_error() {
            Some(ERROR_INVALID_PARAMETER) => Ok(false),
            Some(ERROR_ACCESS_DENIED) => Ok(true),
            _ => Err(RuntimeProcessError::InspectProcess { pid, source }),
        };
    }

    let wait = unsafe { WaitForSingleObject(handle, 0) };
    if wait == WAIT_TIMEOUT {
        close_handle(handle);
        return Ok(true);
    }
    if wait == WAIT_OBJECT_0 {
        let mut exit_code = 0_u32;
        if unsafe { GetExitCodeProcess(handle, &mut exit_code) } == 0 {
            let source = std::io::Error::last_os_error();
            close_handle(handle);
            return Err(RuntimeProcessError::InspectProcess { pid, source });
        }
        close_handle(handle);
        return Ok(exit_code == STILL_ACTIVE);
    }

    let source = std::io::Error::last_os_error();
    close_handle(handle);
    Err(RuntimeProcessError::InspectProcess { pid, source })
}

#[cfg(windows)]
struct WindowsProcessHandle {
    raw: *mut std::ffi::c_void,
    pid: u32,
}

#[cfg(windows)]
impl WindowsProcessHandle {
    fn open(pid: u32, additional_access: u32) -> Result<Option<Self>, RuntimeProcessError> {
        let raw = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE | additional_access,
                0,
                pid,
            )
        };
        if raw.is_null() {
            let source = std::io::Error::last_os_error();
            return match source.raw_os_error() {
                Some(ERROR_INVALID_PARAMETER) => Ok(None),
                _ => Err(RuntimeProcessError::InspectProcess { pid, source }),
            };
        }
        Ok(Some(Self { raw, pid }))
    }

    fn is_running(&self) -> Result<bool, RuntimeProcessError> {
        self.wait_for_exit(0).map(|exited| !exited)
    }

    fn wait_for_exit(&self, timeout_ms: u32) -> Result<bool, RuntimeProcessError> {
        match unsafe { WaitForSingleObject(self.raw, timeout_ms) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(RuntimeProcessError::InspectProcess {
                pid: self.pid,
                source: std::io::Error::last_os_error(),
            }),
        }
    }

    fn identity(&self) -> Result<ProcessIdentity, RuntimeProcessError> {
        query_windows_process_identity_from_handle(self.pid, self.raw)
    }

    fn terminate(&self) -> Result<(), RuntimeProcessError> {
        if unsafe { TerminateProcess(self.raw, 1) } == 0 {
            return Err(RuntimeProcessError::KillByPid {
                pid: self.pid,
                source: std::io::Error::last_os_error(),
            });
        }
        let _ = self.wait_for_exit(PROCESS_EXIT_SETTLE_TIMEOUT_MS);
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for WindowsProcessHandle {
    fn drop(&mut self) {
        close_handle(self.raw);
    }
}

#[cfg(windows)]
fn open_verified_windows_process(
    pid: u32,
    expected_identity: &ProcessIdentity,
    additional_access: u32,
) -> Result<Option<WindowsProcessHandle>, RuntimeProcessError> {
    let Some(handle) = WindowsProcessHandle::open(pid, additional_access)? else {
        return Ok(None);
    };
    if !handle.is_running()? {
        return Ok(None);
    }
    if !process_identities_match(expected_identity, &handle.identity()?) {
        return Err(RuntimeProcessError::ProcessIdentityMismatch { pid });
    }
    Ok(Some(handle))
}

#[cfg(windows)]
#[derive(Debug, Clone)]
struct WindowsProcessRecord {
    process_id: u32,
    parent_process_id: u32,
    name: String,
    identity: Option<ProcessIdentity>,
}

#[cfg(windows)]
#[repr(C)]
struct ProcessEntry32W {
    dw_size: u32,
    cnt_usage: u32,
    th32_process_id: u32,
    th32_default_heap_id: usize,
    th32_module_id: u32,
    cnt_threads: u32,
    th32_parent_process_id: u32,
    pc_pri_class_base: i32,
    dw_flags: u32,
    sz_exe_file: [u16; MAX_PATH_WIDE],
}

#[cfg(windows)]
fn find_windows_preferred_descendant_pid(
    root_pid: u32,
    expected_root_identity: &ProcessIdentity,
    expected_executable: Option<&str>,
) -> Result<Option<u32>, RuntimeProcessError> {
    let descendants = load_windows_process_descendants(root_pid, expected_root_identity)?;
    Ok(select_windows_workload_descendant(
        root_pid,
        &descendants,
        expected_executable,
    ))
}

#[cfg(windows)]
fn select_windows_workload_descendant(
    root_pid: u32,
    descendants: &[WindowsProcessRecord],
    expected_executable: Option<&str>,
) -> Option<u32> {
    let expected_executable = expected_executable.map(normalized_process_image_path);
    let by_id = descendants
        .iter()
        .map(|record| (record.process_id, record.parent_process_id))
        .collect::<HashMap<_, _>>();

    // An elevated launch can expose conhost before its executable exists.
    // Keep tracking the owning shell until a workload appears; console hosts
    // also must not displace a workload merely because they are leaf nodes.
    descendants
        .iter()
        .filter(|record| !is_windows_shell_support_process_name(&record.name))
        // An injected loader may add a deeper crash reporter. A native launch
        // already specifies the workload, so require its verified image path.
        .filter(|record| {
            expected_executable.as_ref().is_none_or(|expected| {
                record.identity.as_ref().is_some_and(|identity| {
                    normalized_process_image_path(&identity.image_path) == *expected
                })
            })
        })
        .max_by_key(|record| {
            (
                windows_descendant_depth(record.process_id, root_pid, &by_id),
                record.process_id,
            )
        })
        .map(|record| record.process_id)
}

#[cfg(windows)]
fn windows_descendant_depth(process_id: u32, root_pid: u32, by_id: &HashMap<u32, u32>) -> usize {
    let mut depth = 0_usize;
    let mut current = process_id;
    while let Some(parent) = by_id.get(&current).copied() {
        depth += 1;
        if parent == root_pid {
            break;
        }
        current = parent;
    }
    depth
}

#[cfg(windows)]
fn is_windows_shell_support_process_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "cmd.exe" | "conhost.exe"
    )
}

#[cfg(windows)]
fn load_windows_process_descendants(
    root_pid: u32,
    expected_root_identity: &ProcessIdentity,
) -> Result<Vec<WindowsProcessRecord>, RuntimeProcessError> {
    let Some(root_identity) = inspect_windows_process_identity(root_pid)? else {
        return Ok(Vec::new());
    };
    if !process_identities_match(expected_root_identity, &root_identity) {
        return Ok(Vec::new());
    }
    let snapshot = collect_windows_process_snapshot(root_pid)?;
    Ok(collect_verified_windows_process_descendants_from_snapshot(
        root_pid,
        &root_identity,
        &snapshot,
        inspect_windows_process_identity,
    )
    .processes)
}

#[cfg(windows)]
fn collect_windows_process_tree(
    root_pid: u32,
    expected_identity: &ProcessIdentity,
) -> Result<Vec<WindowsProcessRecord>, RuntimeProcessError> {
    let root = verified_windows_process_root(root_pid, expected_identity)?;
    let snapshot = collect_windows_process_snapshot(root_pid)?;
    let mut tree = vec![root];
    tree.extend(
        collect_verified_windows_process_descendants_from_snapshot(
            root_pid,
            expected_identity,
            &snapshot,
            inspect_windows_process_identity,
        )
        .processes,
    );
    Ok(tree)
}

#[cfg(windows)]
fn verified_windows_process_root(
    pid: u32,
    expected_identity: &ProcessIdentity,
) -> Result<WindowsProcessRecord, RuntimeProcessError> {
    let Some(actual_identity) = inspect_windows_process_identity(pid)? else {
        return Err(RuntimeProcessError::ProcessIdentityUnavailable { pid });
    };
    if !process_identities_match(expected_identity, &actual_identity) {
        return Err(RuntimeProcessError::ProcessIdentityMismatch { pid });
    }
    Ok(WindowsProcessRecord {
        process_id: pid,
        parent_process_id: 0,
        name: String::new(),
        identity: Some(actual_identity),
    })
}

#[cfg(windows)]
struct WindowsVerifiedProcessDescendants {
    processes: Vec<WindowsProcessRecord>,
    inspection_errors: Vec<RuntimeProcessError>,
}

#[cfg(windows)]
fn collect_verified_windows_process_descendants_from_snapshot<F>(
    root_pid: u32,
    root_identity: &ProcessIdentity,
    snapshot: &[WindowsProcessRecord],
    mut inspect_identity: F,
) -> WindowsVerifiedProcessDescendants
where
    F: FnMut(u32) -> Result<Option<ProcessIdentity>, RuntimeProcessError>,
{
    let mut children_by_parent = HashMap::<u32, Vec<&WindowsProcessRecord>>::new();
    for record in snapshot {
        children_by_parent
            .entry(record.parent_process_id)
            .or_default()
            .push(record);
    }

    let mut processes = Vec::new();
    let mut inspection_errors = Vec::new();
    let mut queue = VecDeque::from([(root_pid, root_identity.clone())]);
    let mut discovered = HashSet::from([root_pid]);

    while let Some((parent_pid, parent_identity)) = queue.pop_front() {
        if let Some(children) = children_by_parent.get(&parent_pid) {
            for child in children {
                if discovered.contains(&child.process_id) {
                    continue;
                }
                let child_identity = match inspect_identity(child.process_id) {
                    Ok(Some(identity)) => identity,
                    Ok(None) => continue,
                    Err(error) => {
                        inspection_errors.push(error);
                        continue;
                    }
                };
                if child_identity.creation_time < parent_identity.creation_time {
                    continue;
                }

                discovered.insert(child.process_id);
                let mut verified_child = (*child).clone();
                verified_child.identity = Some(child_identity.clone());
                processes.push(verified_child);
                queue.push_back((child.process_id, child_identity));
            }
        }
    }

    WindowsVerifiedProcessDescendants {
        processes,
        inspection_errors,
    }
}

#[cfg(windows)]
fn collect_windows_process_snapshot(
    root_pid: u32,
) -> Result<Vec<WindowsProcessRecord>, RuntimeProcessError> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot as isize == -1 {
        return Err(RuntimeProcessError::InspectProcess {
            pid: root_pid,
            source: std::io::Error::last_os_error(),
        });
    }

    let mut records = Vec::new();
    let mut process_entry = ProcessEntry32W {
        dw_size: std::mem::size_of::<ProcessEntry32W>() as u32,
        cnt_usage: 0,
        th32_process_id: 0,
        th32_default_heap_id: 0,
        th32_module_id: 0,
        cnt_threads: 0,
        th32_parent_process_id: 0,
        pc_pri_class_base: 0,
        dw_flags: 0,
        sz_exe_file: [0; MAX_PATH_WIDE],
    };

    let first_ok = unsafe { Process32FirstW(snapshot, &mut process_entry) };
    if first_ok != 0 {
        loop {
            records.push(WindowsProcessRecord {
                process_id: process_entry.th32_process_id,
                parent_process_id: process_entry.th32_parent_process_id,
                name: utf16_buffer_to_string(&process_entry.sz_exe_file),
                identity: None,
            });

            let next_ok = unsafe { Process32NextW(snapshot, &mut process_entry) };
            if next_ok == 0 {
                break;
            }
        }
    }

    close_handle(snapshot);
    Ok(records)
}

#[cfg(windows)]
fn utf16_buffer_to_string(buffer: &[u16]) -> String {
    let len = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..len])
}

#[cfg(not(windows))]
fn is_windows_batch_script(_path: &Path) -> bool {
    false
}

#[cfg(windows)]
fn spawn_run_as_process(
    spawn_command: &str,
    spawn_args: &[String],
    working_directory: &Path,
    stdout: File,
    stderr: File,
    run_in_background: bool,
) -> Result<(RuntimeChild, Option<WindowsHiddenDesktop>), std::io::Error> {
    elevated_launcher::spawn(
        spawn_command,
        spawn_args,
        working_directory,
        stdout,
        stderr,
        run_in_background,
    )
}

#[cfg(not(windows))]
fn spawn_standard_process(
    plan: &SpawnCommand<'_>,
    stdout: File,
    stderr: File,
    run_in_background: bool,
    uses_script_entrypoint: bool,
) -> Result<(RuntimeChild, Option<WindowsHiddenDesktop>), std::io::Error> {
    let mut command = Command::new(plan.executable);
    if run_in_background || uses_script_entrypoint {
        apply_background_window_policy(&mut command, uses_script_entrypoint);
    }
    command
        .args(plan.args)
        .envs(plan.environment)
        .current_dir(plan.working_directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map(|child| (RuntimeChild::Standard(child), None))
}

#[cfg(windows)]
fn create_hidden_desktop_for_spawn() -> Result<WindowsHiddenDesktop, std::io::Error> {
    let desktop_index = HIDDEN_DESKTOP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let desktop_name = format!("LanGameHidden-{}-{desktop_index}", std::process::id());
    let mut desktop_name_wide = wide_null(&desktop_name);
    let mut security = SecurityAttributes {
        n_length: std::mem::size_of::<SecurityAttributes>() as u32,
        lp_security_descriptor: std::ptr::null_mut(),
        b_inherit_handle: 1,
    };

    let desktop = unsafe {
        CreateDesktopW(
            desktop_name_wide.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            0,
            HIDDEN_DESKTOP_ACCESS,
            &mut security,
        )
    };
    if desktop.is_null() {
        return Err(std::io::Error::last_os_error());
    }

    Ok(WindowsHiddenDesktop {
        handle: desktop as usize,
        name: desktop_name,
    })
}

#[cfg(windows)]
fn hidden_desktop_spawn_target(desktop_name: &str) -> String {
    if desktop_name.contains('\\') {
        desktop_name.to_string()
    } else {
        format!("WinSta0\\{desktop_name}")
    }
}

fn build_spawn_command_line(spawn_command: &str, spawn_args: &[String]) -> String {
    let mut segments = Vec::with_capacity(spawn_args.len() + 1);
    segments.push(quote_command_segment(spawn_command));
    segments.extend(spawn_args.iter().map(|arg| quote_command_segment(arg)));
    segments.join(" ")
}

#[cfg(windows)]
fn background_creation_flags(
    uses_script_entrypoint: bool,
    host_surface: &ProcessHostSurface,
) -> u32 {
    match host_surface {
        ProcessHostSurface::ManagedTerminal | ProcessHostSurface::ManagedPseudoConsole => {
            CREATE_NEW_CONSOLE
        }
        ProcessHostSurface::ManagedNativeWindow if uses_script_entrypoint => CREATE_NO_WINDOW,
        ProcessHostSurface::ManagedNativeWindow | ProcessHostSurface::ExternalWindow => {
            CREATE_NO_WINDOW | DETACHED_PROCESS
        }
    }
}

#[cfg(windows)]
fn duplicate_inheritable_handle(
    handle: *mut std::ffi::c_void,
) -> Result<OwnedWindowsHandle, std::io::Error> {
    let mut duplicate = std::ptr::null_mut();
    let current_process = unsafe { GetCurrentProcess() };
    if unsafe {
        DuplicateHandle(
            current_process,
            handle,
            current_process,
            &mut duplicate,
            0,
            1,
            DUPLICATE_SAME_ACCESS,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(OwnedWindowsHandle::new(duplicate))
}

#[cfg(windows)]
fn hidden_desktop_inherited_handles(
    stdin: *mut std::ffi::c_void,
    stdout: *mut std::ffi::c_void,
    stderr: *mut std::ffi::c_void,
) -> [*mut std::ffi::c_void; 3] {
    [stdin, stdout, stderr]
}

#[cfg(windows)]
#[derive(Debug)]
struct OwnedWindowsHandle(*mut std::ffi::c_void);

#[cfg(windows)]
impl OwnedWindowsHandle {
    fn new(handle: *mut std::ffi::c_void) -> Self {
        Self(handle)
    }

    fn as_raw(&self) -> *mut std::ffi::c_void {
        self.0
    }

    fn into_raw(mut self) -> *mut std::ffi::c_void {
        let handle = self.0;
        self.0 = std::ptr::null_mut();
        handle
    }
}

#[cfg(windows)]
impl Drop for OwnedWindowsHandle {
    fn drop(&mut self) {
        close_handle(self.0);
    }
}

#[cfg(windows)]
fn close_handle(handle: *mut std::ffi::c_void) {
    if !handle.is_null() {
        unsafe {
            let _ = CloseHandle(handle);
        }
    }
}

#[cfg(windows)]
fn wide_null(value: &str) -> Vec<u16> {
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(not(windows))]
fn apply_background_window_policy(_command: &mut Command, _uses_script_entrypoint: bool) {}

#[cfg(windows)]
#[repr(C)]
struct SecurityAttributes {
    n_length: u32,
    lp_security_descriptor: *mut std::ffi::c_void,
    b_inherit_handle: i32,
}

#[cfg(windows)]
#[repr(C)]
struct StartupInfoW {
    cb: u32,
    lp_reserved: *mut u16,
    lp_desktop: *mut u16,
    lp_title: *mut u16,
    dw_x: u32,
    dw_y: u32,
    dw_x_size: u32,
    dw_y_size: u32,
    dw_x_count_chars: u32,
    dw_y_count_chars: u32,
    dw_fill_attribute: u32,
    dw_flags: u32,
    w_show_window: u16,
    cb_reserved2: u16,
    lp_reserved2: *mut u8,
    h_std_input: *mut std::ffi::c_void,
    h_std_output: *mut std::ffi::c_void,
    h_std_error: *mut std::ffi::c_void,
}

#[cfg(windows)]
#[repr(C)]
struct StartupInfoExW {
    startup_info: StartupInfoW,
    attribute_list: *mut std::ffi::c_void,
}

#[cfg(windows)]
#[derive(Default)]
#[repr(C)]
struct ProcessInformation {
    process_handle: *mut std::ffi::c_void,
    thread_handle: *mut std::ffi::c_void,
    process_id: u32,
    thread_id: u32,
}

#[cfg(windows)]
#[derive(Default)]
#[repr(C)]
struct FileTime {
    low: u32,
    high: u32,
}

#[cfg(windows)]
#[repr(C)]
struct ShellExecuteInfoW {
    cb_size: u32,
    f_mask: u32,
    hwnd: *mut std::ffi::c_void,
    lp_verb: *const u16,
    lp_file: *const u16,
    lp_parameters: *const u16,
    lp_directory: *const u16,
    n_show: i32,
    h_inst_app: *mut std::ffi::c_void,
    lp_id_list: *mut std::ffi::c_void,
    lp_class: *const u16,
    hkey_class: *mut std::ffi::c_void,
    dw_hot_key: u16,
    h_icon_or_monitor: *mut std::ffi::c_void,
    h_process: *mut std::ffi::c_void,
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn CreateDesktopW(
        desktop: *mut u16,
        device: *const u16,
        device_mode: *mut std::ffi::c_void,
        flags: u32,
        desired_access: u32,
        security_attributes: *mut SecurityAttributes,
    ) -> *mut std::ffi::c_void;
    fn CloseDesktop(desktop: *mut std::ffi::c_void) -> i32;
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreatePipe(
        read_pipe: *mut *mut std::ffi::c_void,
        write_pipe: *mut *mut std::ffi::c_void,
        pipe_attributes: *mut SecurityAttributes,
        size: u32,
    ) -> i32;
    fn SetHandleInformation(handle: *mut std::ffi::c_void, mask: u32, flags: u32) -> i32;
    fn InitializeProcThreadAttributeList(
        attribute_list: *mut std::ffi::c_void,
        attribute_count: u32,
        flags: u32,
        size: *mut usize,
    ) -> i32;
    fn UpdateProcThreadAttribute(
        attribute_list: *mut std::ffi::c_void,
        flags: u32,
        attribute: usize,
        value: *const std::ffi::c_void,
        size: usize,
        previous_value: *mut std::ffi::c_void,
        return_size: *mut usize,
    ) -> i32;
    fn DeleteProcThreadAttributeList(attribute_list: *mut std::ffi::c_void);
    fn DuplicateHandle(
        source_process_handle: *mut std::ffi::c_void,
        source_handle: *mut std::ffi::c_void,
        target_process_handle: *mut std::ffi::c_void,
        target_handle: *mut *mut std::ffi::c_void,
        desired_access: u32,
        inherit_handle: i32,
        options: u32,
    ) -> i32;
    fn GetCurrentProcess() -> *mut std::ffi::c_void;
    fn OpenProcess(
        desired_access: u32,
        inherit_handle: i32,
        process_id: u32,
    ) -> *mut std::ffi::c_void;
    fn SetPriorityClass(process_handle: *mut std::ffi::c_void, priority_class: u32) -> i32;
    fn SetProcessAffinityMask(process_handle: *mut std::ffi::c_void, process_mask: usize) -> i32;
    fn TerminateProcess(process_handle: *mut std::ffi::c_void, exit_code: u32) -> i32;
    fn GetProcessId(process_handle: *mut std::ffi::c_void) -> u32;
    fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> *mut std::ffi::c_void;
    fn Process32FirstW(snapshot: *mut std::ffi::c_void, entry: *mut ProcessEntry32W) -> i32;
    fn Process32NextW(snapshot: *mut std::ffi::c_void, entry: *mut ProcessEntry32W) -> i32;
    fn CreateProcessW(
        application_name: *const u16,
        command_line: *mut u16,
        process_attributes: *mut SecurityAttributes,
        thread_attributes: *mut SecurityAttributes,
        inherit_handles: i32,
        creation_flags: u32,
        environment: *mut std::ffi::c_void,
        current_directory: *mut u16,
        startup_info: *mut StartupInfoW,
        process_information: *mut ProcessInformation,
    ) -> i32;
    fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
    fn GetExitCodeProcess(handle: *mut std::ffi::c_void, exit_code: *mut u32) -> i32;
    fn GetProcessTimes(
        process_handle: *mut std::ffi::c_void,
        creation_time: *mut FileTime,
        exit_time: *mut FileTime,
        kernel_time: *mut FileTime,
        user_time: *mut FileTime,
    ) -> i32;
    fn QueryFullProcessImageNameW(
        process_handle: *mut std::ffi::c_void,
        flags: u32,
        executable_name: *mut u16,
        size: *mut u32,
    ) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    fn AttachConsole(process_id: u32) -> i32;
    fn FreeConsole() -> i32;
    fn GenerateConsoleCtrlEvent(ctrl_event: u32, process_group_id: u32) -> i32;
    fn SetConsoleCtrlHandler(handler_routine: *mut std::ffi::c_void, add: i32) -> i32;
}

#[cfg(windows)]
#[link(name = "shell32")]
unsafe extern "system" {
    fn ShellExecuteExW(execute_info: *mut ShellExecuteInfoW) -> i32;
}

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod launch_plan_tests;
#[cfg(test)]
mod launch_preparation_tests;
#[cfg(test)]
mod launch_verbatim_path_tests;
#[cfg(test)]
mod tests_save_policy;

#[cfg(test)]
mod runtime_supervisor_tests;

#[cfg(test)]
mod windows_process_tests;

#[cfg(all(windows, test))]
mod stdin_write_tests;

#[cfg(all(windows, test))]
mod owned_process_tree_tests;

#[cfg(all(test, windows))]
mod runtime_reliability_tests;

#[cfg(test)]
#[path = "native_launch_parameter_tests.rs"]
mod native_launch_parameter_tests;

#[cfg(test)]
#[path = "../../config_acceptance_test_support.rs"]
mod config_acceptance_test_support;

#[cfg(test)]
#[path = "runtime_acceptance_fixture_selection_tests.rs"]
mod runtime_acceptance_fixture_selection_tests;
