//! ASA API's cold offset download precedes ordinary listener initialization.
//! Every deadline belongs to one process identity in the current start operation.
use super::RuntimeStartReservationLease;
use super::commands_runtime_supervision::StrictBindEvaluation;
use app_core::{PortBinding, ProcessLaunchPlan};
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};
#[path = "commands_asa_startup_evidence.rs"]
mod evidence;
use evidence::{Evidence, ImageCache, Observation, capture, observe, process_creation_time};
#[cfg(test)]
use evidence::{MAX_LOG_BYTES, cached_image, parse_observation, read_observation};

const COLD_CACHE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
type TargetId = (String, u32, u64, String);

pub(super) struct StartupSession {
    started: tokio::time::Instant,
    images: ImageCache,
    states: BTreeMap<TargetId, TargetState>,
}
impl Default for StartupSession {
    fn default() -> Self {
        Self {
            started: tokio::time::Instant::now(),
            images: ImageCache::default(),
            states: BTreeMap::new(),
        }
    }
}

struct TargetState {
    clock: PhaseClock,
    ready_samples: u8,
}
#[derive(Clone)]
struct Target {
    id: TargetId,
    evidence: Option<Evidence>,
    required_ports: Vec<String>,
}

#[derive(Debug)]
struct PhaseClock {
    cold_deadline: Duration,
    hard_deadline: Duration,
    binding_deadline: Duration,
    cold_seen: bool,
    initialized: bool,
}
impl PhaseClock {
    fn new(normal: Duration, process_age: Duration, now: Duration) -> Self {
        Self {
            cold_deadline: now + COLD_CACHE_TIMEOUT.saturating_sub(process_age),
            hard_deadline: now + (COLD_CACHE_TIMEOUT + normal).saturating_sub(process_age),
            binding_deadline: now + normal,
            cold_seen: false,
            initialized: false,
        }
    }
    fn check(&self, now: Duration) -> Result<(), String> {
        if now < self.deadline() {
            return Ok(());
        }
        Err(self.timeout_message())
    }
    fn observe(
        &mut self,
        observation: &Observation,
        now: Duration,
        normal: Duration,
    ) -> Result<(), String> {
        // Check before a completion record can change the active deadline.
        self.check(now)?;
        if self.initialized {
            return Ok(());
        }
        if observation.cold_download && now >= self.cold_deadline {
            return Err(format!(
                "ASA API offset initialization exceeded its {} ms phase budget",
                COLD_CACHE_TIMEOUT.as_millis()
            ));
        }
        self.cold_seen |= observation.cold_download;
        if observation.initialized {
            self.initialized = true;
            if self.cold_seen {
                self.binding_deadline = (now + normal).min(self.hard_deadline);
            }
        }
        Ok(())
    }
    fn deadline(&self) -> Duration {
        if self.cold_seen && !self.initialized {
            self.cold_deadline
        } else {
            self.binding_deadline
        }
    }
    fn timeout_message(&self) -> String {
        if self.cold_seen && !self.initialized {
            format!(
                "ASA API offset cache initialization did not complete within {} ms",
                COLD_CACHE_TIMEOUT.as_millis()
            )
        } else {
            "owned game listener was not verified before the normal startup deadline".into()
        }
    }
}

pub(super) struct StartupBudget<'a> {
    session: &'a mut StartupSession,
    started: tokio::time::Instant,
    normal_timeout: Duration,
    targets: Vec<Target>,
    console_log: Option<String>,
    reported_cold: bool,
    reported_binding: bool,
}
impl<'a> StartupBudget<'a> {
    pub(super) async fn prepare(
        module_id: &str,
        processes: &mut [(ProcessLaunchPlan, app_runtime::SpawnedProcess)],
        normal_timeout: Duration,
        reservation: &RuntimeStartReservationLease,
        session: &'a mut StartupSession,
        required_ports: &[PortBinding],
    ) -> Result<Self, String> {
        let console_log = processes.first().map(|(plan, _)| plan.log_path.clone());
        let candidates = if module_id == "arksurvivalascended" {
            processes
                .iter()
                .filter_map(|(plan, child)| {
                    let required = required_ports
                        .iter()
                        .filter(|port| {
                            port.name
                                .split_once(':')
                                .is_some_and(|(key, _)| key == plan.process_key)
                        })
                        .map(|port| port.name.clone())
                        .collect::<Vec<_>>();
                    (!required.is_empty()).then(|| {
                        (
                            plan.process_key.clone(),
                            plan.launch_plan.install_root.clone(),
                            plan.launch_plan.executable_path.clone(),
                            child.pid,
                            child.process_identity.clone(),
                            required,
                        )
                    })
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let mut images = std::mem::take(&mut session.images);
        let (captured, images, issues) = tokio::select! {
            biased;
            _ = reservation.cancelled() => return Err("ASA startup preparation was cancelled.".into()),
            result = tokio::task::spawn_blocking(move || {
                let mut captured = Vec::new();
                let mut issues = Vec::new();
                for (key, root, executable, pid, identity, required) in candidates {
                    let evidence = if cfg!(windows) {
                        match capture(&root, &executable, pid, identity.clone(), &mut images) {
                            Ok(evidence) => evidence,
                            Err(error) => { issues.push(format!("pid {pid}: {error}")); None }
                        }
                    } else { None };
                    captured.push(((key, pid, identity.creation_time, identity.image_path.clone()), identity, required, evidence));
                }
                (captured, images, issues)
            }) => result.map_err(|error| format!("ASA startup evidence worker failed: {error}"))?,
        };
        session.images = images;
        if !issues.is_empty()
            && let Some(path) = console_log.as_deref()
        {
            let _ = super::append_startup_console_line(
                path,
                &format!(
                    "ASA offset evidence is unavailable; retaining the normal startup budget: {}",
                    issues.into_iter().take(4).collect::<Vec<_>>().join("; ")
                ),
                "",
            );
        }
        // Both process age and monotonic reference are sampled after preparation.
        // Hashing time is included exactly once in the absolute cold deadline.
        let wall_now = SystemTime::now();
        let now = session.started.elapsed();
        let mut targets = Vec::new();
        for (id, identity, required_ports, evidence) in captured {
            let age = process_creation_time(&identity)
                .and_then(|created| wall_now.duration_since(created).ok())
                .unwrap_or_default();
            session
                .states
                .entry(id.clone())
                .or_insert_with(|| TargetState {
                    clock: PhaseClock::new(normal_timeout, age, now),
                    ready_samples: 0,
                });
            targets.push(Target {
                id,
                evidence,
                required_ports,
            });
        }
        Ok(Self {
            session,
            started: tokio::time::Instant::now(),
            normal_timeout,
            targets,
            console_log,
            reported_cold: false,
            reported_binding: false,
        })
    }

    fn check_at(&self, now: Duration) -> Result<(), String> {
        if self.targets.is_empty() {
            return if self.started.elapsed() >= self.normal_timeout {
                Err(self.timeout_message())
            } else {
                Ok(())
            };
        }
        for target in &self.targets {
            let state = &self.session.states[&target.id];
            if state.ready_samples < 3 {
                state
                    .clock
                    .check(now)
                    .map_err(|error| format!("ASA map `{}`: {error}", target.id.0))?;
            }
        }
        Ok(())
    }

    pub(super) async fn remaining(&mut self) -> Result<Duration, String> {
        self.check_at(self.session.started.elapsed())?;
        let evidence = self
            .targets
            .iter()
            .enumerate()
            .filter_map(|(index, target)| target.evidence.clone().map(|evidence| (index, evidence)))
            .collect::<Vec<_>>();
        if !evidence.is_empty() {
            let observations = tokio::task::spawn_blocking(move || {
                evidence
                    .iter()
                    .map(|(index, evidence)| {
                        observe(evidence).map(|observation| (*index, observation))
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .await
            .map_err(|error| format!("ASA startup evidence worker failed: {error}"))??;
            let now = self.session.started.elapsed();
            self.check_at(now)?;
            for (index, observation) in observations {
                let target = &self.targets[index];
                let state = self
                    .session
                    .states
                    .get_mut(&target.id)
                    .expect("owned startup target");
                if state.ready_samples < 3 {
                    state
                        .clock
                        .observe(&observation, now, self.normal_timeout)?;
                }
            }
        }
        self.report_phase();
        self.remaining_at(self.session.started.elapsed())
    }

    fn remaining_at(&self, now: Duration) -> Result<Duration, String> {
        self.check_at(now)?;
        if self.targets.is_empty() {
            return Ok(self.normal_timeout.saturating_sub(self.started.elapsed()));
        }
        Ok(self
            .targets
            .iter()
            .filter_map(|target| {
                let state = &self.session.states[&target.id];
                (state.ready_samples < 3).then(|| state.clock.deadline().saturating_sub(now))
            })
            .min()
            .unwrap_or(self.normal_timeout))
    }

    pub(super) fn record_evaluation(
        &mut self,
        evaluation: &StrictBindEvaluation,
    ) -> Result<(), String> {
        self.record_evaluation_at(evaluation, self.session.started.elapsed())
    }

    fn record_evaluation_at(
        &mut self,
        evaluation: &StrictBindEvaluation,
        now: Duration,
    ) -> Result<(), String> {
        if self.targets.is_empty() {
            return self.check_at(now);
        }
        for target in &self.targets {
            let ready = match evaluation {
                StrictBindEvaluation::Ready => true,
                StrictBindEvaluation::Pending { missing_port_names } => !target
                    .required_ports
                    .iter()
                    .any(|port| missing_port_names.contains(port)),
                StrictBindEvaluation::Failed { .. } => return Ok(()),
            };
            let state = self
                .session
                .states
                .get_mut(&target.id)
                .expect("owned startup target");
            if !ready {
                state.ready_samples = 0;
            }
            if state.ready_samples < 3 {
                state
                    .clock
                    .check(now)
                    .map_err(|error| format!("ASA map `{}`: {error}", target.id.0))?;
                if ready {
                    state.clock.observe(
                        &Observation {
                            cold_download: false,
                            initialized: true,
                        },
                        now,
                        self.normal_timeout,
                    )?;
                    state.ready_samples += 1;
                }
            }
        }
        self.report_phase();
        Ok(())
    }

    pub(super) fn timeout_message(&self) -> String {
        if let Some(target) = self
            .targets
            .iter()
            .filter(|target| self.session.states[&target.id].ready_samples < 3)
            .min_by_key(|target| self.session.states[&target.id].clock.deadline())
        {
            return format!(
                "ASA map `{}`: {}",
                target.id.0,
                self.session.states[&target.id].clock.timeout_message()
            );
        }
        format!(
            "owned game listener was not verified within {} ms",
            self.normal_timeout.as_millis()
        )
    }

    fn report_phase(&mut self) {
        let cold_seen = self
            .targets
            .iter()
            .any(|target| self.session.states[&target.id].clock.cold_seen);
        let initializing = self.targets.iter().any(|target| {
            let clock = &self.session.states[&target.id].clock;
            clock.cold_seen && !clock.initialized
        });
        let message = if cold_seen && !self.reported_cold {
            self.reported_cold = true;
            Some(
                "ASA API is downloading offsets for this server executable (maximum 15 minutes from process start). Game listener verification remains pending.",
            )
        } else if cold_seen && !initializing && !self.reported_binding {
            self.reported_binding = true;
            Some(
                "ASA API offset initialization finished. Waiting for the game's owned listeners within the normal startup budget.",
            )
        } else {
            None
        };
        if let (Some(message), Some(path)) = (message, self.console_log.as_deref()) {
            let _ = super::append_startup_console_line(path, message, "");
        }
    }
}
#[cfg(test)]
#[path = "commands_asa_startup_budget_tests.rs"]
mod tests;
