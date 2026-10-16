//! Current-run IL2CPP generation evidence, captured through the owned ConPTY.
//! BepInEx 1.733.2 creates LogOutput.log only after generation; it is not evidence.
use crate::commands::{RuntimeStartReservationLease, commands_mod_staging};
use app_core::{ProcessHostSurface, ProcessIdentity, ProcessLaunchPlan};
use app_storage::managed_console_log::{ManagedConsoleLog, open_log_segments};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

// An LGSM resource bound, matching the existing ASA cold-start budget. Upstream
// documents a multi-minute first run, but promises no maximum generation time.
const INITIALIZATION_LIMIT: Duration = Duration::from_secs(15 * 60);
const READ_BYTES: usize = 1024 * 1024;
const MAX_LINE_BYTES: usize = 8 * 1024;

#[derive(Default, Debug, PartialEq, Eq)]
struct Observation {
    generating: bool,
    initialized: bool,
    failed: bool,
}

fn observe_line(line: &[u8], result: &mut Observation) {
    let Ok(line) = std::str::from_utf8(line) else {
        return;
    };
    let Some((header, message)) = line
        .trim()
        .strip_prefix('[')
        .and_then(|s| s.split_once(']'))
    else {
        return;
    };
    let Some((level, source)) = header.split_once(':') else {
        return;
    };
    let (level, source, message) = (level.trim(), source.trim(), message.trim());
    if source == "InteropManager" {
        result.failed |= level == "Error"
            && message.starts_with("Failed to generate Il2Cpp interop assemblies:");
        if matches!(level, "Info" | "Message") {
            result.generating |= message
                == "Detected outdated interop assemblies, will regenerate them now"
                || message.starts_with("Downloading unity base libraries ")
                || message.starts_with("Reading unity base libraries from file ")
                || message == "Extracting downloaded unity base libraries"
                || message.starts_with("Running Cpp2IL to generate dummy assemblies from ")
                || message == "Generating interop assemblies";
        }
    }
    result.initialized |= source == "BepInEx"
        && level == "Message"
        && matches!(
            message,
            "Chainloader initialized" | "Chainloader startup complete"
        );
}

#[derive(Clone, Default)]
struct ConsoleCursor {
    offset: u64,
    pending: Vec<u8>,
    discarding: bool,
}
impl ConsoleCursor {
    fn consume(&mut self, bytes: &[u8], result: &mut Observation) {
        for &byte in bytes {
            if byte == b'\n' {
                if !self.discarding {
                    observe_line(&self.pending, result);
                }
                self.pending.clear();
                self.discarding = false;
            } else if !self.discarding {
                if self.pending.len() == MAX_LINE_BYTES {
                    self.pending.clear();
                    self.discarding = true;
                } else {
                    self.pending.push(byte);
                }
            }
        }
    }

    fn read(&mut self, path: &Path) -> Result<Observation, String> {
        if let Some(error) = app_storage::managed_console_log::failure_summary_for_path(path) {
            return Err(format!(
                "V Rising initialization console recording failed: {error}. Free disk space and retry; inspect this run's console log."
            ));
        }
        let segments = open_log_segments(path)
            .map_err(|error| {
                format!("Cannot read this V Rising run's initialization console: {error}")
            })?
            .ok_or("V Rising initialization requires its owned managed console log")?;
        if segments
            .first()
            .is_some_and(|part| part.start_offset > self.offset)
        {
            return Err("V Rising initialization evidence expired under console retention; inspect this run's console and retry with less verbose plugin logging.".into());
        }
        let mut remaining = READ_BYTES;
        let mut result = Observation::default();
        for mut part in segments {
            let length = part
                .file
                .metadata()
                .map_err(|error| error.to_string())?
                .len();
            let end = part.start_offset.saturating_add(length);
            if end <= self.offset {
                continue;
            }
            if part.start_offset > self.offset {
                return Err("V Rising initialization console has a missing segment; inspect this run's console before retrying.".into());
            }
            part.file
                .seek(SeekFrom::Start(self.offset - part.start_offset))
                .map_err(|error| error.to_string())?;
            let size = (end - self.offset).min(remaining as u64);
            let mut bytes = Vec::with_capacity(size as usize);
            part.file
                .take(size)
                .read_to_end(&mut bytes)
                .map_err(|error| error.to_string())?;
            self.offset += bytes.len() as u64;
            remaining -= bytes.len();
            self.consume(&bytes, &mut result);
            if remaining == 0 {
                break;
            }
        }
        Ok(result)
    }
}

#[derive(Debug)]
struct PhaseClock {
    normal: Duration,
    binding_deadline: Duration,
    generating: bool,
    initialized: bool,
}
impl PhaseClock {
    fn new(normal: Duration) -> Self {
        Self {
            normal,
            binding_deadline: normal,
            generating: false,
            initialized: false,
        }
    }
    fn deadline(&self) -> Duration {
        if self.generating && !self.initialized {
            INITIALIZATION_LIMIT
        } else {
            self.binding_deadline
        }
    }
    fn check(&self, now: Duration) -> Result<Duration, String> {
        if now >= self.deadline() {
            Err(self.timeout_message())
        } else {
            Ok(self.deadline() - now)
        }
    }
    fn observe(&mut self, result: &Observation, now: Duration) -> Result<(), String> {
        self.check(now)?;
        if result.failed {
            return Err("V Rising BepInEx failed to generate IL2CPP interop assemblies. Inspect this run's console for the generator error, check disk space and access to Unity base libraries, then retry; existing plugins were preserved.".into());
        }
        if !self.initialized {
            self.generating |= result.generating;
            if result.initialized {
                self.initialized = true;
                if self.generating {
                    self.binding_deadline =
                        (now + self.normal).min(INITIALIZATION_LIMIT + self.normal);
                }
            }
        }
        Ok(())
    }
    fn timeout_message(&self) -> String {
        if self.generating && !self.initialized {
            "V Rising BepInEx IL2CPP initialization exceeded LGSM's 15-minute limit. Inspect this run's console for Unity library download/Cpp2IL errors, check disk space and network access, then retry; existing plugins were preserved.".into()
        } else if self.generating {
            format!(
                "V Rising IL2CPP initialization completed, but its owned game listener was not verified within {} ms; inspect this run's console and selected bind address.",
                self.normal.as_millis()
            )
        } else {
            format!(
                "V Rising did not expose its owned game listener or current-run IL2CPP generation evidence within {} ms. Inspect this run's console; BepInEx Logging.Console.LogLevels must include Message/Info to observe initialization.",
                self.normal.as_millis()
            )
        }
    }
}

pub(in crate::commands) struct StartupSession {
    log: PathBuf,
    executable: PathBuf,
    started: tokio::time::Instant,
    cursor: ConsoleCursor,
}

impl StartupSession {
    /// Called before spawn, while the existing instance/program guards are held.
    /// Native servers and unknown loaders retain their existing launch surface.
    pub(in crate::commands) async fn prepare(
        module_id: &str,
        plans: &mut [ProcessLaunchPlan],
        reservation: &RuntimeStartReservationLease,
    ) -> Result<Option<Self>, String> {
        if !cfg!(windows) || module_id != "vrising" || plans.len() != 1 {
            return Ok(None);
        }
        let plan = &mut plans[0];
        if plan.launch_plan.requires_admin || plan.launch_plan.uses_script_entrypoint {
            return Ok(None);
        }
        let root = PathBuf::from(&plan.launch_plan.install_root);
        let executable = PathBuf::from(&plan.launch_plan.executable_path);
        let log = PathBuf::from(&plan.log_path);
        let mut worker = tokio::task::spawn_blocking(move || {
            if !root
                .join(".langame-vrising-loader.json")
                .try_exists()
                .map_err(|error| error.to_string())?
            {
                return Ok::<_, String>(None);
            }
            if commands_mod_staging::runtime_loader_requires_archive("vrising", &root)? {
                return Ok(None);
            }
            let executable = fs::canonicalize(executable).map_err(|error| error.to_string())?;
            if !executable.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .eq_ignore_ascii_case("VRisingServer.exe")
            }) || executable.parent()
                != Some(
                    fs::canonicalize(root)
                        .map_err(|error| error.to_string())?
                        .as_path(),
                )
            {
                return Err(
                    "Managed V Rising loader does not belong to this launch executable".into(),
                );
            }
            let _writer = ManagedConsoleLog::open(&log).map_err(|error| error.to_string())?;
            let parts = open_log_segments(&log)
                .map_err(|error| error.to_string())?
                .ok_or("V Rising startup requires a managed run log")?;
            let mut cursor = ConsoleCursor::default();
            for part in parts {
                cursor.offset = cursor.offset.max(
                    part.start_offset
                        + part
                            .file
                            .metadata()
                            .map_err(|error| error.to_string())?
                            .len(),
                );
            }
            Ok(Some((log, executable, cursor)))
        });
        let prepared = tokio::select! {
            biased;
            _ = reservation.cancelled() => {
                // The worker may still open the run log. Join it before the
                // caller can release instance/program ownership on cancellation.
                worker.await.map_err(|error| format!("V Rising startup preparation worker failed during cancellation: {error}"))??;
                return Err("V Rising startup preparation was cancelled.".into());
            },
            result = &mut worker => result.map_err(|error| format!("V Rising startup preparation worker failed: {error}"))??,
        };
        let Some((log, executable, cursor)) = prepared else {
            return Ok(None);
        };
        plan.launch_plan.host_surface = ProcessHostSurface::ManagedPseudoConsole;
        // Official preloader CLI override; do not rewrite the operator's config.
        plan.launch_plan
            .args
            .extend(["--enable-console".into(), "true".into()]);
        Ok(Some(Self {
            log,
            executable,
            started: tokio::time::Instant::now(),
            cursor,
        }))
    }
}

pub(super) struct StartupBudget<'a> {
    session: &'a mut StartupSession,
    pid: u32,
    identity: ProcessIdentity,
    clock: PhaseClock,
    reported_generation: bool,
    reported_initialized: bool,
}
impl<'a> StartupBudget<'a> {
    pub(super) fn prepare(
        session: &'a mut StartupSession,
        processes: &[(ProcessLaunchPlan, app_runtime::SpawnedProcess)],
        normal: Duration,
    ) -> Result<Self, String> {
        let [(plan, process)] = processes else {
            return Err("V Rising initialization requires exactly one owned process".into());
        };
        let image = fs::canonicalize(&process.process_identity.image_path)
            .map_err(|error| error.to_string())?;
        if image != session.executable
            || Path::new(&process.log_path) != session.log
            || Path::new(&plan.log_path) != session.log
            || process.process_identity.creation_time == 0
            || plan.launch_plan.host_surface != ProcessHostSurface::ManagedPseudoConsole
        {
            return Err(
                "V Rising initialization console does not belong to this process identity".into(),
            );
        }
        Ok(Self {
            session,
            pid: process.pid,
            identity: process.process_identity.clone(),
            clock: PhaseClock::new(normal),
            reported_generation: false,
            reported_initialized: false,
        })
    }

    pub(super) async fn remaining(&mut self) -> Result<Duration, String> {
        self.clock.check(self.session.started.elapsed())?;
        let (pid, identity, path, mut cursor) = (
            self.pid,
            self.identity.clone(),
            self.session.log.clone(),
            self.session.cursor.clone(),
        );
        let (cursor, observation) = tokio::task::spawn_blocking(move || {
            let matches = || {
                app_runtime::process_matches_identity(pid, &identity)
                    .map_err(|error| error.to_string())
            };
            if !matches()? {
                return Err("V Rising initialization process exited or changed identity".into());
            }
            let observation = cursor.read(&path)?;
            if !matches()? {
                return Err("V Rising initialization process exited or changed identity".into());
            }
            Ok::<_, String>((cursor, observation))
        })
        .await
        .map_err(|error| format!("V Rising initialization evidence worker failed: {error}"))??;
        let now = self.session.started.elapsed();
        self.clock.observe(&observation, now)?;
        self.session.cursor = cursor;
        self.report_phase();
        self.clock.check(now)
    }

    pub(super) fn record_evaluation(&self) -> Result<(), String> {
        self.clock.check(self.session.started.elapsed()).map(|_| ())
    }
    pub(super) fn timeout_message(&self) -> String {
        self.clock.timeout_message()
    }

    fn report_phase(&mut self) {
        let message = if self.clock.generating && !self.reported_generation {
            self.reported_generation = true;
            Some(
                "V Rising BepInEx is generating IL2CPP interop assemblies for this run (LGSM limit: 15 minutes from launch). Game listener verification remains pending.",
            )
        } else if self.clock.generating && self.clock.initialized && !self.reported_initialized {
            self.reported_initialized = true;
            Some(
                "V Rising BepInEx IL2CPP initialization finished. Waiting for this process's game listener within the normal startup budget.",
            )
        } else {
            None
        };
        if let Some(message) = message {
            let _ = crate::commands::append_startup_console_line(&self.session.log, message, "");
        }
    }
}

#[cfg(test)]
#[path = "commands_vrising_startup_tests.rs"]
mod tests;
