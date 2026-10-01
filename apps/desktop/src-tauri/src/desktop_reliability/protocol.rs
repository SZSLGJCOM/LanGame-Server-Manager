use serde::{Deserialize, Serialize};

use crate::webview_recovery::RecoverySnapshot;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Action {
    SendCommand,
    CrashRenderer,
    CrashBrowser,
    Finish,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Stage {
    InitialCommand,
    RendererCrash,
    RendererRecovery,
    AfterRendererCommand,
    BrowserCrash,
    BrowserRecovery,
    AfterBrowserCommand,
    Finish,
    Finished,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Observations {
    pub event_command_counts: Vec<u32>,
    pub dom_command_counts: Vec<u32>,
    pub max_tail_lines: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dom_nodes_after_unmount: Option<usize>,
    pub native_bridge: bool,
    pub browser_errors: Vec<String>,
}

impl Observations {
    pub fn validate(&self, count: u32, finished: bool) -> Result<(), String> {
        let expected = (1..=count).collect::<Vec<_>>();
        if self.event_command_counts != expected
            || self.dom_command_counts != expected
            || self.max_tail_lines != 400
            || !self.native_bridge
            || !self.browser_errors.is_empty()
            || (finished && self.dom_nodes_after_unmount != Some(0))
            || (!finished && self.dom_nodes_after_unmount.is_some())
        {
            return Err("Real event/DOM observations do not satisfy this fixture stage".into());
        }
        Ok(())
    }
}

pub(super) struct Progress {
    pub stage: Stage,
    pub busy: bool,
    pub command_count: u32,
    pub error: Option<String>,
    pub observations: Option<Observations>,
    pub faults_requested: Vec<String>,
    pub native_failures: Vec<String>,
    baseline: Option<(u64, u64)>,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            stage: Stage::InitialCommand,
            busy: false,
            command_count: 0,
            error: None,
            observations: None,
            faults_requested: Vec::with_capacity(2),
            native_failures: Vec::with_capacity(2),
            baseline: None,
        }
    }
}

impl Progress {
    pub fn update_recovery(&mut self, snapshot: &RecoverySnapshot) -> Result<(), String> {
        let expected = match self.stage {
            Stage::RendererRecovery => "renderer_exited",
            Stage::BrowserRecovery => "browser_exited",
            _ => return Ok(()),
        };
        let Some((recoveries, failures)) = self.baseline else {
            return Err("Missing fault baseline".into());
        };
        if snapshot.recoveries <= recoveries || !snapshot.observer_ready {
            return Ok(());
        }
        if snapshot.failures <= failures || snapshot.last_failure_kind.as_deref() != Some(expected)
        {
            return Err(format!(
                "Recovery did not observe the requested native failure {expected}"
            ));
        }
        self.native_failures.push(expected.into());
        self.stage = if self.stage == Stage::RendererRecovery {
            Stage::AfterRendererCommand
        } else {
            Stage::AfterBrowserCommand
        };
        self.baseline = None;
        Ok(())
    }

    pub fn begin(
        &mut self,
        action: Action,
        observations: Option<Observations>,
        snapshot: &RecoverySnapshot,
    ) -> Result<(), String> {
        self.update_recovery(snapshot)?;
        if self.busy || self.error.is_some() || !snapshot.observer_ready || snapshot.paused {
            return Err(
                "Fixture is busy, failed, or the native recovery observer is not ready".into(),
            );
        }
        let valid = matches!(
            (self.stage, action),
            (
                Stage::InitialCommand | Stage::AfterRendererCommand | Stage::AfterBrowserCommand,
                Action::SendCommand
            ) | (Stage::RendererCrash, Action::CrashRenderer)
                | (Stage::BrowserCrash, Action::CrashBrowser)
                | (Stage::Finish, Action::Finish)
        );
        if !valid {
            return Err("Fixture action does not match the current stage".into());
        }
        if !matches!(action, Action::SendCommand) {
            let observations = observations.ok_or("This action requires real UI observations")?;
            observations.validate(self.command_count, matches!(action, Action::Finish))?;
            self.observations = Some(observations);
        } else if observations.is_some() {
            return Err("Command actions do not accept observations".into());
        }
        self.busy = true;
        if matches!(action, Action::CrashRenderer | Action::CrashBrowser) {
            self.baseline = Some((snapshot.recoveries, snapshot.failures));
            self.stage = if matches!(action, Action::CrashRenderer) {
                Stage::RendererRecovery
            } else {
                Stage::BrowserRecovery
            };
            self.faults_requested.push(
                if matches!(action, Action::CrashRenderer) {
                    "renderer"
                } else {
                    "browser"
                }
                .into(),
            );
        }
        Ok(())
    }

    pub fn command_completed(&mut self) {
        self.command_count += 1;
        self.stage = match self.command_count {
            1 => Stage::RendererCrash,
            2 => Stage::BrowserCrash,
            _ => Stage::Finish,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> RecoverySnapshot {
        RecoverySnapshot {
            observer_ready: true,
            generation: 1,
            recoveries: 0,
            failures: 0,
            page_loads: 1,
            paused: false,
            last_failure_kind: None,
            last_error: None,
        }
    }

    #[test]
    fn desktop_reliability_protocol_rejects_duplicate_commands_and_requires_native_recovery() {
        let mut progress = Progress::default();
        let mut recovery = snapshot();
        progress
            .begin(Action::SendCommand, None, &recovery)
            .unwrap();
        assert!(
            progress
                .begin(Action::SendCommand, None, &recovery)
                .is_err()
        );
        progress.command_completed();
        progress.busy = false;
        assert!(
            progress
                .begin(Action::SendCommand, None, &recovery)
                .is_err()
        );
        let observed = Observations {
            event_command_counts: vec![1],
            dom_command_counts: vec![1],
            max_tail_lines: 400,
            native_bridge: true,
            ..Default::default()
        };
        progress
            .begin(Action::CrashRenderer, Some(observed), &recovery)
            .unwrap();
        progress.busy = false;
        progress.update_recovery(&recovery).unwrap();
        assert_eq!(progress.stage, Stage::RendererRecovery);
        recovery.recoveries = 1;
        assert!(progress.update_recovery(&recovery).is_err());
        recovery.failures = 1;
        recovery.last_failure_kind = Some("renderer_exited".into());
        progress.update_recovery(&recovery).unwrap();
        assert_eq!(progress.stage, Stage::AfterRendererCommand);
    }

    #[test]
    fn desktop_reliability_finish_requires_actual_unmount_observation() {
        let mut observed = Observations {
            event_command_counts: vec![1, 2, 3],
            dom_command_counts: vec![1, 2, 3],
            max_tail_lines: 400,
            native_bridge: true,
            ..Default::default()
        };
        assert!(observed.validate(3, true).is_err());
        observed.dom_nodes_after_unmount = Some(0);
        assert!(observed.validate(3, true).is_ok());
        assert!(observed.validate(3, false).is_err());
    }
}
