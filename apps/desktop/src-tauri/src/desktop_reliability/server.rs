use std::collections::BTreeMap;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use app_core::{
    InstanceStatus, InstanceSummary, LaunchPlan, ProcessHostSurface, ProcessWindowPolicy,
};
use app_runtime::{ManagedProcess, RuntimeSupervisor};
use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};

use super::config::{Config, verify_server_root};

pub(super) const INSTANCE_ID: &str = "desktop-reliability";
pub(super) const COMMAND: &str = "LGSM_RELIABILITY_COMMAND";
pub(super) const SERVER_ROLE: &str = "--desktop-reliability-server";

struct ProcessHandle(usize);

impl ProcessHandle {
    fn open(pid: u32) -> Result<Self, String> {
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(Self(handle as usize))
    }

    fn alive(&self) -> Result<bool, String> {
        match unsafe { WaitForSingleObject(self.0 as _, 0) } {
            WAIT_TIMEOUT => Ok(true),
            WAIT_OBJECT_0 => Ok(false),
            _ => Err(std::io::Error::last_os_error().to_string()),
        }
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0 as _);
        }
    }
}

pub(super) struct Server {
    pub supervisor: RuntimeSupervisor,
    pub pid: u32,
    pub log_path: PathBuf,
    root: PathBuf,
    handle: ProcessHandle,
}

impl Server {
    pub fn spawn(config: &Config) -> Result<Self, String> {
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let root = config.root.to_string_lossy().into_owned();
        let plan = LaunchPlan {
            instance_id: INSTANCE_ID.into(),
            instance_name: "Desktop reliability fixture".into(),
            module_id: "demo".into(),
            install_root: root.clone(),
            install_state: app_core::InstallState::Installed,
            uses_private_runtime: false,
            working_directory: root.clone(),
            executable_path: executable.to_string_lossy().into_owned(),
            executable_exists: true,
            ready_to_launch: true,
            validation_issues: Vec::new(),
            args: vec![SERVER_ROLE.into(), root, config.nonce.clone()],
            environment: BTreeMap::new(),
            command_line: String::new(),
            window_policy: ProcessWindowPolicy::Background,
            uses_script_entrypoint: false,
            requires_admin: false,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
            performance_policy: Default::default(),
            performance_preview: Default::default(),
        };
        let log_path = config.root.join("logs/managed-console/run-1.log");
        let writer = app_storage::managed_console_log::ManagedConsoleLog::open(&log_path)
            .map_err(|error| error.to_string())?;
        let spawned =
            app_runtime::spawn_launch_plan_with_log_writer(&plan, &log_path, Box::new(writer))
                .map_err(|error| error.to_string())?;
        let handle = ProcessHandle::open(spawned.pid)?;
        let pid = spawned.pid;
        let mut supervisor = RuntimeSupervisor::default();
        supervisor.insert_running(
            InstanceSummary {
                id: INSTANCE_ID.into(),
                name: plan.instance_name,
                module_id: plan.module_id,
                status: InstanceStatus::Running,
                active_process_count: 1,
                bind_ip: "127.0.0.1".into(),
                port_count: 0,
                autostart: false,
            },
            Some("desktop-reliability-session".into()),
            vec![ManagedProcess {
                run_id: 1,
                process_key: "main".into(),
                display_name: "Fixture server".into(),
                pid,
                process_identity: spawned.process_identity,
                root_process_identity: spawned.root_process_identity,
                log_path: spawned.log_path,
                is_primary: true,
                uses_script_entrypoint: false,
                performance_policy: Default::default(),
                last_performance_refresh: None,
                last_performance_target_count: None,
                last_performance_application: None,
                child: spawned.child,
                hidden_desktop: spawned.hidden_desktop,
            }],
        );
        Ok(Self {
            supervisor,
            pid,
            log_path,
            root: config.root.clone(),
            handle,
        })
    }

    pub fn alive(&self) -> Result<bool, String> {
        self.handle.alive()
    }

    pub fn command(&mut self, expected: u32) -> Result<(), String> {
        self.supervisor
            .dispatch_command(INSTANCE_ID, Some("main"), COMMAND)
            .map_err(|error| error.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let count = self.command_count()?;
            if count == expected {
                return Ok(());
            }
            if count > expected || !self.alive()? {
                return Err("Simulated server command count or liveness violated".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Err("Simulated server did not acknowledge the command within five seconds".into())
    }

    pub fn command_count(&self) -> Result<u32, String> {
        // Each immutable receipt is published only after accepting one exact command.
        let mut count = 0;
        for index in 1..=4 {
            if self
                .root
                .join(format!("data/command-{index}.json"))
                .try_exists()
                .map_err(|error| error.to_string())?
            {
                count = index;
            }
        }
        Ok(count)
    }

    pub fn stop(&mut self) -> Result<(), String> {
        if let Some(mut instance) = self.supervisor.take_running_for_stop(INSTANCE_ID) {
            app_runtime::stop_managed_instance(&mut instance).map_err(|error| error.to_string())?;
            // Dropping the final child owner joins the real output reader.
            drop(instance);
        }
        let wait = unsafe { WaitForSingleObject(self.handle.0 as _, 5_000) };
        if wait != WAIT_OBJECT_0 {
            return Err(format!(
                "Simulated server did not exit after stop: wait={wait}"
            ));
        }
        Ok(())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("Desktop fixture server cleanup: {error}");
        }
    }
}

pub(super) fn run(root: &Path, nonce: &str) -> Result<(), String> {
    verify_server_root(root, nonce)?;
    let mut output = std::io::stdout().lock();
    writeln!(output, "LGSM_RELIABILITY_SERVER_READY")
        .and_then(|()| output.flush())
        .map_err(|error| error.to_string())?;
    let mut input = std::io::stdin().lock();
    let mut count = 0;
    loop {
        let mut line = Vec::new();
        // A fixed protocol with a bounded read also rejects accidental general commands.
        let read = Read::by_ref(&mut input)
            .take(128)
            .read_until(b'\n', &mut line)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            return Ok(());
        }
        let command = line.strip_suffix(b"\n").unwrap_or(&line);
        let command = command.strip_suffix(b"\r").unwrap_or(command);
        if line.len() == 128 || command != COMMAND.as_bytes() {
            return Err("Invalid simulated server command".into());
        }
        count += 1;
        if count > 3 {
            return Err("Duplicate simulated server command".into());
        }
        super::config::write_new_json(
            &root.join(format!("data/command-{count}.json")),
            &serde_json::json!({"count":count}),
        )?;
        for index in 0..450 {
            writeln!(output, "fixture output {count}:{index}")
                .map_err(|error| error.to_string())?;
        }
        writeln!(output, "{COMMAND}_{count}")
            .and_then(|()| output.flush())
            .map_err(|error| error.to_string())?;
    }
}
