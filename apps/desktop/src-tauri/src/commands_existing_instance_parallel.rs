//! Test-only bounded orchestration. Every admitted future is joined, including
//! after a worker panic, an unsafe stop, or a receipt write failure.
use super::{ExistingClient, InstanceDetails, InstanceSummary, ModuleDescriptor, Receipt};
use super::{all_inactive, evidence, read, run_one, schedule, selection};
use schedule::{Candidate, Scheduler};
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::RwLock;
use tokio::task::{Id, JoinSet};

const MIN_AVAILABLE_MEMORY: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Clone, Default)]
pub(super) struct Ownership {
    state: Arc<RwLock<OwnedState>>,
    halted: Arc<AtomicBool>,
}

#[derive(Default)]
struct OwnedState {
    reserved: BTreeSet<String>,
    start_requested: BTreeSet<String>,
}

struct WorkerGuard {
    ownership: Ownership,
    finished: bool,
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.ownership.halt();
        }
    }
}

impl Ownership {
    pub(super) fn halt(&self) {
        self.halted.store(true, Ordering::SeqCst);
    }
    fn halted(&self) -> bool {
        self.halted.load(Ordering::SeqCst)
    }

    async fn reserve(&self, id: &str) {
        self.state.write().await.reserved.insert(id.into());
    }

    async fn release(&self, id: &str) {
        let mut state = self.state.write().await;
        state.reserved.remove(id);
        state.start_requested.remove(id);
    }

    pub(super) async fn mark_start_requested(&self, id: &str) -> bool {
        let mut state = self.state.write().await;
        if self.halted() || !state.reserved.contains(id) {
            return false;
        }
        state.start_requested.insert(id.into());
        true
    }

    pub(super) async fn verify_stopped_target(
        &self,
        client: &ExistingClient,
        id: &str,
    ) -> Result<(), String> {
        // Hold the reservation lease through both reads. Admission/release must
        // not race a backend snapshot and falsely classify a sibling as foreign.
        let state = self.state.read().await;
        if !state.reserved.contains(id) {
            return Err("target_has_no_reservation".into());
        }
        let instances: Vec<InstanceSummary> =
            read(client, "list_instances_from_storage", json!({})).await?;
        schedule::verify_owned_active(&instances, &state.reserved)?;
        let target: InstanceDetails = read(
            client,
            "read_instance_details_from_storage",
            json!({"instanceId": id}),
        )
        .await?;
        if !selection::is_inactive(&target.summary) || target.active_run.is_some() {
            return Err("selected_instance_not_stopped".into());
        }
        Ok(())
    }
}

#[derive(Default, Serialize)]
struct BatchReceipt {
    selected: usize,
    admitted: usize,
    completed: usize,
    passed: usize,
    pending: usize,
    halted: bool,
    failures: Vec<String>,
}

fn tighten_resources(candidate: &mut Candidate, settings: &serde_json::Value) {
    if candidate.module_id == "minecraft" {
        candidate.exclusive |= settings
            .get("memory_max_mb")
            .and_then(serde_json::Value::as_u64)
            .is_none_or(|heap| heap > 4096);
    }
    if candidate.module_id == "dontstarve" {
        candidate.exclusive |=
            app_core::dst_shards::dst_shards(settings).map_or(true, |shards| shards.len() > 1);
    }
}

fn available_memory() -> Result<u64, String> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut memory: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    memory.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut memory) } == 0 {
        return Err("available_physical_memory_read_failed".into());
    }
    Ok(memory.ullAvailPhys)
}

pub(super) async fn run(
    client: ExistingClient,
    descriptors: Vec<ModuleDescriptor>,
    selected: Vec<InstanceSummary>,
    directory: PathBuf,
) -> Result<(), String> {
    let client = Arc::new(client);
    let mut contexts = BTreeMap::new();
    let mut candidates = Vec::new();
    for summary in &selected {
        let descriptor = descriptors
            .iter()
            .find(|item| item.summary.id == summary.module_id)
            .ok_or("selected_module_disappeared")?
            .clone();
        let details: InstanceDetails = read(
            &client,
            "read_instance_details_from_storage",
            json!({"instanceId": summary.id}),
        )
        .await?;
        if details.summary.id != summary.id
            || details.summary.module_id != summary.module_id
            || !selection::is_inactive(&details.summary)
            || details.active_run.is_some()
        {
            return Err("reservation_target_not_stopped_or_changed".into());
        }
        let mut candidate =
            Candidate::from_bindings(&summary.id, &summary.module_id, &details.ports)?;
        let settings = serde_json::from_str(&details.settings_json)
            .map_err(|_| "reservation_settings_json_invalid")?;
        tighten_resources(&mut candidate, &settings);
        candidates.push(candidate);
        contexts.insert(summary.id.clone(), (descriptor, details));
    }
    // Read every port before the first start; persisted metadata contains no
    // passwords/settings. Per-worker prepare checks these reservations again.
    candidates.sort_by_key(|candidate| candidate.exclusive);
    evidence::write_receipt(&directory, "schedule", &candidates)?;
    all_inactive(&client).await?;
    let contexts = Arc::new(contexts);
    let worker_client = client.clone();
    let worker_directory = directory.clone();
    let recovery_client = client.clone();
    let ownership = Ownership::default();
    let result = drive(
        candidates,
        ownership,
        move |candidate, ownership| {
            let client = worker_client.clone();
            let directory = worker_directory.clone();
            let contexts = contexts.clone();
            async move {
                let Some((descriptor, before)) = contexts.get(&candidate.instance_id) else {
                    return (
                        failed_receipt(&candidate, "reservation_context_missing"),
                        false,
                    );
                };
                run_one(
                    &client,
                    descriptor,
                    &candidate.instance_id,
                    &directory,
                    before,
                    &ownership,
                )
                .await
            }
        },
        move |candidate, ownership| {
            let client = recovery_client.clone();
            async move { recover_panicked_worker(&client, &candidate, &ownership).await }
        },
        available_memory,
        |candidate, receipt, safe| {
            println!(
                "EXISTING_ACCEPTANCE module={} passed={} native_ready={} health={} stopped={}",
                candidate.module_id,
                receipt.passed,
                receipt.native_ready,
                receipt.health_status.as_deref().unwrap_or("unavailable"),
                safe
            );
            evidence::write_receipt(&directory, &candidate.module_id, receipt)
        },
    )
    .await?;
    // No worker remains when either of these operations can return an error.
    let final_state = all_inactive(&client).await;
    evidence::write_receipt(&directory, "batch", &result)?;
    final_state?;
    if result.passed != result.selected || result.halted || !result.failures.is_empty() {
        return Err(format!(
            "existing-instance acceptance passed {}/{}; completed={}; pending={}; receipts={}",
            result.passed,
            result.selected,
            result.completed,
            result.pending,
            directory.display()
        ));
    }
    Ok(())
}

fn failed_receipt(candidate: &Candidate, reason: &str) -> Receipt {
    Receipt {
        module_id: candidate.module_id.clone(),
        instance_id: candidate.instance_id.clone(),
        phase: "aborted".into(),
        failures: vec![reason.into()],
        ..Default::default()
    }
}

async fn recover_panicked_worker(
    client: &ExistingClient,
    candidate: &Candidate,
    ownership: &Ownership,
) -> (Receipt, bool) {
    let mut receipt = failed_receipt(candidate, "acceptance_worker_panicked");
    let requested = ownership
        .state
        .read()
        .await
        .start_requested
        .contains(&candidate.instance_id);
    if requested {
        // The start request may have published a run before the task panicked.
        // Only this harness's requested targets receive recovery normal-stop.
        let began = std::time::Instant::now();
        let stopped = read::<app_core::StopInstanceResult>(
            client,
            "stop_instance_process",
            json!({"instanceId": candidate.instance_id}),
        )
        .await;
        receipt.stop_elapsed_ms = began.elapsed().as_millis();
        receipt.stop_api_succeeded = stopped.is_ok();
        if let Ok(stopped) = stopped {
            receipt.stop_exit_codes = stopped
                .processes
                .iter()
                .map(|process| process.exit_code)
                .collect();
            receipt.stop_evidence.observe_response(None, &stopped);
        }
    }
    receipt.stopped_state_confirmed = ownership
        .verify_stopped_target(client, &candidate.instance_id)
        .await
        .is_ok();
    // Lost local identities cannot prove that unpublished children exited.
    (receipt, false)
}

async fn drive<W, WF, R, RF, M, P>(
    candidates: Vec<Candidate>,
    ownership: Ownership,
    worker: W,
    recover: R,
    mut memory: M,
    mut persist: P,
) -> Result<BatchReceipt, String>
where
    W: Fn(Candidate, Ownership) -> WF,
    WF: Future<Output = (Receipt, bool)> + Send + 'static,
    R: Fn(Candidate, Ownership) -> RF,
    RF: Future<Output = (Receipt, bool)>,
    M: FnMut() -> Result<u64, String>,
    P: FnMut(&Candidate, &Receipt, bool) -> Result<(), String>,
{
    let mut result = BatchReceipt {
        selected: candidates.len(),
        ..Default::default()
    };
    let mut scheduler = Scheduler::new(candidates, 3)?;
    let mut tasks = JoinSet::new();
    let mut identities = HashMap::<Id, Candidate>::new();
    loop {
        while !ownership.halted()
            && !scheduler.halted()
            && scheduler.pending_count() > 0
            && scheduler.active_count() < 3
        {
            match memory() {
                Ok(available) if available >= MIN_AVAILABLE_MEMORY => {}
                Ok(_) => {
                    result
                        .failures
                        .push("available_physical_memory_below_8_gib".into());
                    ownership.halt();
                    break;
                }
                Err(reason) => {
                    result.failures.push(reason);
                    ownership.halt();
                    break;
                }
            }
            let Some(candidate) = scheduler.admit() else {
                break;
            };
            ownership.reserve(&candidate.instance_id).await;
            let future = worker(candidate.clone(), ownership.clone());
            let worker_ownership = ownership.clone();
            let handle = tasks.spawn(async move {
                let mut guard = WorkerGuard {
                    ownership: worker_ownership,
                    finished: false,
                };
                let outcome = future.await;
                // Close admission when the worker discovers unsafe cleanup,
                // before JoinSet ordering can reap a safe sibling and refill.
                if !outcome.1 {
                    guard.ownership.halt();
                }
                guard.finished = true;
                outcome
            });
            identities.insert(handle.id(), candidate);
            result.admitted += 1;
        }
        let Some(joined) = tasks.join_next_with_id().await else {
            break;
        };
        let (id, outcome) = match joined {
            Ok((id, outcome)) => (id, Some(outcome)),
            Err(error) => {
                ownership.halt();
                result.failures.push("worker_join_failed".into());
                (error.id(), None)
            }
        };
        let Some(candidate) = identities.remove(&id) else {
            ownership.halt();
            result
                .failures
                .push("joined_worker_identity_missing".into());
            continue;
        };
        let (receipt, safe) = match outcome {
            Some(outcome) => outcome,
            None => recover(candidate.clone(), ownership.clone()).await,
        };
        if !safe {
            ownership.halt();
        }
        if let Err(reason) = persist(&candidate, &receipt, safe) {
            ownership.halt();
            result
                .failures
                .push(format!("{}: {reason}", candidate.module_id));
        }
        result.completed += 1;
        result.passed += usize::from(receipt.passed);
        if let Err(reason) = scheduler.complete(&candidate.instance_id, safe) {
            ownership.halt();
            result.failures.push(reason);
        }
        ownership.release(&candidate.instance_id).await;
        // There is deliberately no early-return path while JoinSet owns tasks.
        // Siblings continue their observation and normal-stop sequence.
    }
    result.pending = scheduler.pending_count();
    result.halted = ownership.halted() || scheduler.halted();
    if scheduler.active_count() != 0 || !identities.is_empty() {
        result.halted = true;
        result
            .failures
            .push("worker_reservation_not_drained".into());
    }
    Ok(result)
}

#[path = "commands_existing_instance_parallel_tests.rs"]
mod tests;
