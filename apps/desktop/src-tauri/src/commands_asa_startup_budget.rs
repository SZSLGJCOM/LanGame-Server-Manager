//! ASA's native Mod validation precedes owned-listener readiness. Each
//! CurseForge observation and deadline belongs to one current process identity.
use super::RuntimeStartReservationLease;
use super::commands_runtime_supervision::StrictBindEvaluation;
use app_core::{InstanceDetails, PortBinding, ProcessLaunchPlan};
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
#[path = "commands_asa_curseforge_cache.rs"]
mod curseforge_cache;
#[path = "commands_asa_curseforge_evidence.rs"]
mod curseforge_evidence;
#[path = "commands_asa_curseforge_failure.rs"]
mod curseforge_failure;
pub(super) use curseforge_failure::{
    for_instance as mod_failure_for_instance, for_map as mod_failure_for_map,
    readable as mod_failure_readable, with_cleanup_note as mod_failure_with_cleanup_note,
};
#[path = "commands_asa_curseforge_start_plan.rs"]
mod curseforge_plan;
#[path = "commands_asa_curseforge_startup.rs"]
mod curseforge_startup;
pub(super) use curseforge_plan::requires_validation as requires_mod_validation;

pub(super) fn validate_mod_start_plans(
    instance: &InstanceDetails,
    plans: &[ProcessLaunchPlan],
) -> Result<(), String> {
    for plan in plans {
        curseforge_plan::collect(instance, plan)?;
    }
    Ok(())
}

const MOD_VALIDATION_TIMEOUT: Duration = Duration::from_secs(15 * 60);

fn process_creation_time(identity: &app_core::ProcessIdentity) -> Option<SystemTime> {
    let ticks = identity
        .creation_time
        .checked_sub(116_444_736_000_000_000)?;
    UNIX_EPOCH.checked_add(Duration::from_nanos(ticks.checked_mul(100)?))
}

#[path = "commands_asa_startup_clock.rs"]
mod clock;
use clock::PhaseClock;
type TargetId = (String, u32, u64, String);

pub(super) struct StartupSession {
    started: tokio::time::Instant,
    states: BTreeMap<TargetId, TargetState>,
}
impl Default for StartupSession {
    fn default() -> Self {
        Self {
            started: tokio::time::Instant::now(),
            states: BTreeMap::new(),
        }
    }
}

struct TargetState {
    clock: PhaseClock,
    ready_samples: u8,
    mods: Option<curseforge_startup::Target>,
}
impl TargetState {
    fn mods_ready(&self) -> bool {
        self.clock.mods_ready()
            && self
                .mods
                .as_ref()
                .is_none_or(curseforge_startup::Target::is_complete)
    }
}
#[derive(Clone)]
struct Target {
    id: TargetId,
    required_ports: Vec<String>,
}

pub(super) struct StartupBudget<'a> {
    session: &'a mut StartupSession,
    started: tokio::time::Instant,
    normal_timeout: Duration,
    targets: Vec<Target>,
    console_log: Option<String>,
    reported_mod_check: bool,
    reported_mod_complete: bool,
    reported_mod_count: usize,
}
impl<'a> StartupBudget<'a> {
    pub(super) async fn prepare(
        instance: &InstanceDetails,
        processes: &mut [(ProcessLaunchPlan, app_runtime::SpawnedProcess)],
        normal_timeout: Duration,
        reservation: &RuntimeStartReservationLease,
        session: &'a mut StartupSession,
        required_ports: &[PortBinding],
    ) -> Result<Self, String> {
        let console_log = processes.first().map(|(plan, _)| plan.log_path.clone());
        let candidates = if instance.summary.module_id == "arksurvivalascended" {
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
                        Ok::<_, String>((
                            plan.process_key.clone(),
                            child.pid,
                            child.process_identity.clone(),
                            required,
                            curseforge_plan::collect(instance, plan)?,
                            std::path::PathBuf::from(&plan.launch_plan.working_directory)
                                .join("ShooterGame"),
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };
        if reservation.is_cancelled() {
            return Err("ASA startup preparation was cancelled.".into());
        }
        // Process age keeps the Mod deadline anchored to native process start.
        let wall_now = SystemTime::now();
        let now = session.started.elapsed();
        let mut targets = Vec::new();
        for (key, pid, identity, required_ports, requirements, sdk_root) in candidates {
            let id = (
                key,
                pid,
                identity.creation_time,
                identity.image_path.clone(),
            );
            let age = process_creation_time(&identity)
                .and_then(|created| wall_now.duration_since(created).ok())
                .unwrap_or_default();
            let mods = requirements
                .map(|requirements| {
                    curseforge_startup::Target::new(requirements, id.1, identity)
                        .map(|target| target.with_cache_root(sdk_root, reservation.clone()))
                })
                .transpose()?;
            session.states.entry(id.clone()).or_insert_with(|| {
                let mut clock = PhaseClock::new(normal_timeout, age, now);
                if mods.is_some() {
                    clock.require_mods();
                }
                TargetState {
                    clock,
                    ready_samples: 0,
                    mods,
                }
            });
            targets.push(Target { id, required_ports });
        }
        Ok(Self {
            session,
            started: tokio::time::Instant::now(),
            normal_timeout,
            targets,
            console_log,
            reported_mod_check: false,
            reported_mod_complete: false,
            reported_mod_count: 0,
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
        if self
            .targets
            .iter()
            .all(|target| self.session.states[&target.id].ready_samples >= 3)
            && self
                .targets
                .iter()
                .any(|target| !self.session.states[&target.id].mods_ready())
            && now >= self.final_log_deadline()
        {
            let error = "ASA CurseForge startup logs were not fully consumed before this start's absolute deadline; inspect the native logs and retry.".to_string();
            return Err(self
                .targets
                .iter()
                .find(|target| !self.session.states[&target.id].mods_ready())
                .map_or_else(
                    || error.clone(),
                    |target| self.target_error(target, error.clone()),
                ));
        }
        for target in &self.targets {
            let state = &self.session.states[&target.id];
            if state.ready_samples < 3 {
                state
                    .clock
                    .check(now)
                    .map_err(|error| self.target_error(target, error))?;
            }
        }
        Ok(())
    }

    fn target_error(&self, target: &Target, error: String) -> String {
        let state = &self.session.states[&target.id];
        let error = if !state.mods_ready() {
            state.mods.as_ref().map_or_else(
                || error.clone(),
                |mods| mods.validation_timeout(error.clone()),
            )
        } else {
            error
        };
        mod_failure_for_map(error, &target.id.0, None, None)
    }

    fn final_log_deadline(&self) -> Duration {
        self.targets
            .iter()
            .map(|target| self.session.states[&target.id].clock.hard_deadline)
            .max()
            .unwrap_or(Duration::ZERO)
    }

    pub(super) async fn remaining(&mut self) -> Result<Duration, String> {
        self.check_at(self.session.started.elapsed())?;
        let mods = self
            .targets
            .iter()
            .enumerate()
            .filter_map(|(index, target)| {
                let state = &self.session.states[&target.id];
                state.mods.clone().map(|mods| (index, mods))
            })
            .collect();
        let observed = curseforge_startup::observe(mods)
            .await
            .inspect_err(|error| {
                if let Some(path) = self.console_log.as_deref() {
                    let _ =
                        super::append_startup_console_line(path, &mod_failure_readable(error), "");
                }
            })?;
        for (index, mods) in observed {
            let now = self.session.started.elapsed();
            let state = self
                .session
                .states
                .get_mut(&self.targets[index].id)
                .ok_or("ASA CurseForge startup target disappeared")?;
            if state.ready_samples < 3 {
                state
                    .clock
                    .observe_mods(mods.is_complete(), now, self.normal_timeout)
                    .map_err(|error| {
                        let error = if !state.mods_ready() {
                            mods.validation_timeout(error)
                        } else {
                            error
                        };
                        mod_failure_for_map(error, &self.targets[index].id.0, None, None)
                    })?;
            }
            state.mods = Some(mods);
        }
        self.report_phase();
        self.remaining_at(self.session.started.elapsed())
    }

    /// Native listeners cannot stand in for the SDK's final validation. This
    /// also covers single-map servers bound to 0.0.0.0, which ordinarily need
    /// no strict-address wait.
    pub(super) fn filter_evaluation(
        &self,
        evaluation: StrictBindEvaluation,
    ) -> StrictBindEvaluation {
        let mut missing = match evaluation {
            StrictBindEvaluation::Failed { .. } => return evaluation,
            StrictBindEvaluation::Pending { missing_port_names } => missing_port_names,
            StrictBindEvaluation::Ready => Vec::new(),
        };
        for target in &self.targets {
            let state = &self.session.states[&target.id];
            if !state.mods_ready() {
                missing.push(format!("{}:CurseForge-final-validation", target.id.0));
            }
        }
        if missing.is_empty() {
            StrictBindEvaluation::Ready
        } else {
            StrictBindEvaluation::Pending {
                missing_port_names: missing,
            }
        }
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
                if state.ready_samples < 3 {
                    Some(state.clock.deadline().saturating_sub(now))
                } else if !state.mods_ready() {
                    Some(self.final_log_deadline().saturating_sub(now))
                } else {
                    None
                }
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
                if let Err(error) = state.clock.check(now) {
                    return Err(self.target_error(target, error));
                }
                if ready && state.mods_ready() {
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
            return self.target_error(
                target,
                self.session.states[&target.id].clock.timeout_message(),
            );
        }
        format!(
            "owned game listener was not verified within {} ms",
            self.normal_timeout.as_millis()
        )
    }

    fn report_phase(&mut self) {
        let mods_required = self
            .targets
            .iter()
            .any(|target| self.session.states[&target.id].clock.mods_required);
        let mods_complete = !self
            .targets
            .iter()
            .any(|target| !self.session.states[&target.id].mods_ready());
        let (validated, requested) = self
            .targets
            .iter()
            .filter_map(|target| self.session.states[&target.id].mods.as_ref())
            .map(curseforge_startup::Target::progress)
            .fold((0, 0), |(valid, expected), (next_valid, next_expected)| {
                (valid + next_valid, expected + next_expected)
            });
        let mod_message = if mods_required && !self.reported_mod_check {
            self.reported_mod_check = true;
            Some(format!(
                "ASA CurseForge: the native SDK is checking {requested} enabled/passive Mods against the server catalog, including existing caches. Waiting for this run's final validation (maximum 15 minutes from process start)."
            ))
        } else if validated > self.reported_mod_count {
            self.reported_mod_count = validated;
            Some(format!(
                "ASA CurseForge: {validated}/{requested} requested Mods passed native final validation."
            ))
        } else if mods_required && mods_complete && !self.reported_mod_complete {
            self.reported_mod_complete = true;
            Some(String::from(
                "ASA CurseForge: all requested Mods passed this run's native final validation. Waiting for the owned game listeners before completing startup.",
            ))
        } else {
            None
        };
        if let (Some(message), Some(path)) = (mod_message, self.console_log.as_deref()) {
            let _ = super::append_startup_console_line(path, &message, "");
        }
    }
}
#[cfg(test)]
#[path = "commands_asa_startup_budget_tests.rs"]
mod tests;
