use super::curseforge_evidence::Evidence;
use super::curseforge_failure;
use super::curseforge_plan::Requirements;
use app_core::ProcessIdentity;
use std::path::PathBuf;

#[derive(Clone)]
pub(super) struct Target {
    log: PathBuf,
    evidence: Evidence,
    pid: u32,
    identity: ProcessIdentity,
    project_count: usize,
    project_ids: Vec<String>,
    cache_root: Option<PathBuf>,
    cache_reservation: Option<super::RuntimeStartReservationLease>,
}

impl Target {
    pub(super) fn new(
        requirements: Requirements,
        pid: u32,
        identity: ProcessIdentity,
    ) -> Result<Self, String> {
        let created_ms = identity
            .creation_time
            .checked_sub(116_444_736_000_000_000)
            .ok_or("ASA CurseForge validation requires the owned process creation token")?
            / 10_000;
        Ok(Self {
            log: requirements.native_log,
            project_count: requirements.project_ids.len(),
            project_ids: requirements.project_ids.clone(),
            cache_root: None,
            cache_reservation: None,
            evidence: Evidence::new(requirements.project_ids, created_ms),
            pid,
            identity,
        })
    }

    pub(super) fn with_cache_root(
        mut self,
        root: PathBuf,
        reservation: super::RuntimeStartReservationLease,
    ) -> Self {
        self.cache_root = Some(root);
        self.cache_reservation = Some(reservation);
        self
    }

    fn failed_initialization(&self, error: String, exited: bool) -> String {
        let Some(root) = &self.cache_root else {
            return error;
        };
        // Diagnostic work has its own short bound; it never makes a failed
        // native initializer look ready, nor proves that its cache is latest.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let cancelled = || {
            std::time::Instant::now() >= deadline
                || self
                    .cache_reservation
                    .as_ref()
                    .is_some_and(|lease| lease.is_cancelled())
        };
        let verified = super::curseforge_cache::inspect(root, &self.project_ids, &cancelled)
            .and_then(|snapshot| super::curseforge_cache::verify_current(&snapshot, &cancelled))
            .is_ok();
        curseforge_failure::with_cache_verification(error, verified, exited)
    }

    pub(super) fn is_complete(&self) -> bool {
        self.evidence.is_complete()
    }

    pub(super) fn progress(&self) -> (usize, usize) {
        (self.evidence.valid_count(), self.project_count)
    }

    pub(super) fn validation_timeout(&self, message: String) -> String {
        curseforge_failure::validation_error(message, &self.log, false)
    }

    fn observe(
        &mut self,
        mut matches: impl FnMut(u32, &ProcessIdentity) -> Result<bool, String>,
    ) -> Result<(), String> {
        let identity_failure = match matches(self.pid, &self.identity) {
            Ok(true) => None,
            Ok(false) => Some((
                curseforge_failure::validation_error(
                    "ASA process exited or changed identity before CurseForge validation.".into(),
                    &self.log,
                    true,
                ),
                true,
            )),
            Err(error) => Some((error, false)),
        };
        let reading = self.evidence.read(&self.log);
        if let Some((fallback, exited)) = identity_failure {
            // An exiting process can deny a new identity handle after writing
            // its terminal native error. Only that current-launch failure may
            // refine the diagnostic; logs cannot establish identity or exit.
            let error = self
                .evidence
                .terminal_failure()
                .map(|error| curseforge_failure::with_native_log(error, &self.log))
                .unwrap_or(fallback);
            return Err(self.failed_initialization(error, exited));
        }
        let matches_after = matches(self.pid, &self.identity);
        if let Err(error) = reading {
            let error = curseforge_failure::with_native_log(error, &self.log);
            // Preserve an observed native failure if the subsequent identity
            // inspection fails. Unknown identity does not prove process exit.
            return Err(self.failed_initialization(error, matches!(matches_after, Ok(false))));
        }
        if !matches_after? {
            return Err(curseforge_failure::validation_error(
                "ASA process exited or changed identity during CurseForge validation.".into(),
                &self.log,
                true,
            ));
        }
        Ok(())
    }
}

/// File reads belong to a blocking worker, while each OS identity is checked
/// on both sides of the read. No cached catalog or persistent success receipt
/// substitutes for this process's native final-validation records.
pub(super) async fn observe(targets: Vec<(usize, Target)>) -> Result<Vec<(usize, Target)>, String> {
    if targets.is_empty() {
        return Ok(targets);
    }
    tokio::task::spawn_blocking(move || {
        let mut targets = targets;
        for (_, target) in &mut targets {
            target.observe(|pid, identity| {
                app_runtime::process_matches_identity(pid, identity)
                    .map_err(|error| error.to_string())
            })?;
        }
        Ok(targets)
    })
    .await
    .map_err(|error| format!("ASA CurseForge evidence worker failed: {error}"))?
}

#[cfg(test)]
#[path = "commands_asa_curseforge_startup_tests.rs"]
mod tests;
