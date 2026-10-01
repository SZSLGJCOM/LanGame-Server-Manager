use super::*;

const RUNTIME_STDIN_WRITE_CONFIRMATION_WINDOW: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RuntimeStdinDispatchConfirmation {
    Confirmed,
    Pending,
}

#[derive(Debug, Clone)]
pub(super) struct RuntimeStdinDispatchReceipt {
    pub(super) target: app_runtime::RuntimeCommandDispatchTarget,
    pub(super) confirmation: RuntimeStdinDispatchConfirmation,
}

#[derive(Debug, Clone)]
pub(super) struct RuntimeStdinDispatchBudget {
    confirmation_deadline: Instant,
    submission_tracker: Option<app_runtime::RuntimeCommandSubmissionTracker>,
    completion: Option<RuntimeStdinWriteCompletion>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct RuntimeStdinWriteCompletion {
    result: Arc<StdMutex<Option<Result<(), String>>>>,
}

impl RuntimeStdinWriteCompletion {
    pub(super) fn result(&self) -> Result<Option<Result<(), String>>, String> {
        self.result
            .lock()
            .map(|result| result.clone())
            .map_err(|_| String::from("runtime stdin completion lock poisoned"))
    }

    fn record(&self, result: &Result<RuntimeStdinDispatchReceipt, String>) {
        match self.result.lock() {
            Ok(mut completion) => {
                *completion = Some(result.as_ref().map(|_| ()).map_err(Clone::clone))
            }
            Err(_) => eprintln!("runtime stdin completion lock poisoned"),
        }
    }
}

impl RuntimeStdinDispatchBudget {
    fn standard() -> Self {
        Self {
            confirmation_deadline: Instant::now() + RUNTIME_STDIN_WRITE_CONFIRMATION_WINDOW,
            submission_tracker: None,
            completion: None,
        }
    }

    pub(super) fn tracked_until(
        confirmation_deadline: Instant,
        submission_tracker: app_runtime::RuntimeCommandSubmissionTracker,
    ) -> Self {
        Self {
            confirmation_deadline,
            submission_tracker: Some(submission_tracker),
            completion: None,
        }
    }

    pub(super) fn observed(completion: RuntimeStdinWriteCompletion) -> Self {
        Self {
            completion: Some(completion),
            ..Self::standard()
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum RuntimeStdinWriterFailure {
    Write,
    LeaseRestore,
    WriteAndLeaseRestore,
}

impl RuntimeStdinWriterFailure {
    fn audit_name(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::LeaseRestore => "lease_restore",
            Self::WriteAndLeaseRestore => "write_and_lease_restore",
        }
    }

    fn write_succeeded(self) -> bool {
        matches!(self, Self::LeaseRestore)
    }
}

pub(super) async fn dispatch_managed_stdin_command(
    state: &DesktopState,
    instance_id: &str,
    process_key: Option<&str>,
    command: &str,
    expected_run_id: Option<i64>,
) -> Result<RuntimeStdinDispatchReceipt, String> {
    dispatch_managed_stdin_command_with_budget(
        state,
        instance_id,
        process_key,
        command,
        expected_run_id,
        RuntimeStdinDispatchBudget::standard(),
    )
    .await
}

pub(super) async fn dispatch_managed_stdin_command_with_budget(
    state: &DesktopState,
    instance_id: &str,
    process_key: Option<&str>,
    command: &str,
    expected_run_id: Option<i64>,
    budget: RuntimeStdinDispatchBudget,
) -> Result<RuntimeStdinDispatchReceipt, String> {
    let mut lease = {
        let mut runtime = state
            .runtime_supervisor
            .lock()
            .map_err(|_| String::from("runtime supervisor lock poisoned"))?;
        ensure_expected_supervisor_run(&runtime, instance_id, expected_run_id)?;
        runtime
            .begin_command_dispatch(instance_id, process_key)
            .map_err(|error| error.to_string())?
    };
    let target = lease.target().clone();
    let supervisor = state.runtime_supervisor.clone();
    let command = command.to_owned();
    let writer = tauri::async_runtime::spawn_blocking(move || {
        let target = lease.target().clone();
        let write_succeeded = lease.write_stdin_line(&command).is_ok();
        let restore_succeeded = supervisor
            .lock()
            .map(|mut runtime| runtime.finish_command_dispatch(lease))
            .is_ok();
        match (write_succeeded, restore_succeeded) {
            (true, true) => Ok(target),
            (false, true) => Err(RuntimeStdinWriterFailure::Write),
            (true, false) => Err(RuntimeStdinWriterFailure::LeaseRestore),
            (false, false) => Err(RuntimeStdinWriterFailure::WriteAndLeaseRestore),
        }
    });
    await_runtime_stdin_writer_with_budget(instance_id, target, writer, budget).await
}

#[cfg(test)]
pub(super) async fn await_runtime_stdin_writer_confirmation(
    instance_id: &str,
    target: app_runtime::RuntimeCommandDispatchTarget,
    writer: tauri::async_runtime::JoinHandle<
        Result<app_runtime::RuntimeCommandDispatchTarget, RuntimeStdinWriterFailure>,
    >,
    confirmation_window: Duration,
) -> Result<RuntimeStdinDispatchReceipt, String> {
    await_runtime_stdin_writer_with_budget(
        instance_id,
        target,
        writer,
        RuntimeStdinDispatchBudget {
            confirmation_deadline: Instant::now() + confirmation_window,
            submission_tracker: None,
            completion: None,
        },
    )
    .await
}

async fn await_runtime_stdin_writer_with_budget(
    instance_id: &str,
    target: app_runtime::RuntimeCommandDispatchTarget,
    writer: tauri::async_runtime::JoinHandle<
        Result<app_runtime::RuntimeCommandDispatchTarget, RuntimeStdinWriterFailure>,
    >,
    budget: RuntimeStdinDispatchBudget,
) -> Result<RuntimeStdinDispatchReceipt, String> {
    // Transfer ownership before the first await so caller cancellation cannot orphan lease
    // restoration or the sanitized late-failure audit.
    let completion =
        observe_runtime_stdin_writer(instance_id, target.clone(), writer, budget.completion);
    if let Some(submission_tracker) = budget.submission_tracker {
        submission_tracker.mark_submitted();
    }

    let confirmation_budget = budget
        .confirmation_deadline
        .saturating_duration_since(Instant::now());
    if confirmation_budget.is_zero() {
        return Ok(pending_stdin_dispatch(target));
    }
    match tokio::time::timeout(confirmation_budget, completion).await {
        Ok(Ok(completion)) => completion,
        Ok(Err(_)) => {
            audit_runtime_stdin_writer_failure(instance_id, &target, "observer_task");
            Err(String::from("runtime command writer observer failed"))
        }
        Err(_) => Ok(pending_stdin_dispatch(target)),
    }
}

fn observe_runtime_stdin_writer(
    instance_id: &str,
    target: app_runtime::RuntimeCommandDispatchTarget,
    writer: tauri::async_runtime::JoinHandle<
        Result<app_runtime::RuntimeCommandDispatchTarget, RuntimeStdinWriterFailure>,
    >,
    observer: Option<RuntimeStdinWriteCompletion>,
) -> tokio::sync::oneshot::Receiver<Result<RuntimeStdinDispatchReceipt, String>> {
    let (completion_tx, completion_rx) = tokio::sync::oneshot::channel();
    let observer_instance_id = instance_id.to_owned();
    tauri::async_runtime::spawn(async move {
        let completion = complete_runtime_stdin_writer(&observer_instance_id, target, writer.await);
        if let Some(observer) = observer {
            observer.record(&completion);
        }
        let _ = completion_tx.send(completion);
    });
    completion_rx
}

fn pending_stdin_dispatch(
    target: app_runtime::RuntimeCommandDispatchTarget,
) -> RuntimeStdinDispatchReceipt {
    RuntimeStdinDispatchReceipt {
        target,
        confirmation: RuntimeStdinDispatchConfirmation::Pending,
    }
}

fn complete_runtime_stdin_writer(
    instance_id: &str,
    target: app_runtime::RuntimeCommandDispatchTarget,
    completion: tauri::Result<
        Result<app_runtime::RuntimeCommandDispatchTarget, RuntimeStdinWriterFailure>,
    >,
) -> Result<RuntimeStdinDispatchReceipt, String> {
    match completion {
        Ok(Ok(completed_target)) => Ok(RuntimeStdinDispatchReceipt {
            target: completed_target,
            confirmation: RuntimeStdinDispatchConfirmation::Confirmed,
        }),
        Ok(Err(failure)) => {
            audit_runtime_stdin_writer_failure(instance_id, &target, failure.audit_name());
            if failure.write_succeeded() {
                Ok(RuntimeStdinDispatchReceipt {
                    target,
                    confirmation: RuntimeStdinDispatchConfirmation::Confirmed,
                })
            } else {
                Err(String::from("runtime stdin write failed"))
            }
        }
        Err(_) => {
            audit_runtime_stdin_writer_failure(instance_id, &target, "writer_task");
            Err(String::from("runtime command writer task failed"))
        }
    }
}

fn audit_runtime_stdin_writer_failure(
    instance_id: &str,
    target: &app_runtime::RuntimeCommandDispatchTarget,
    failure_kind: &str,
) {
    let Ok(storage) = bootstrap_storage() else {
        return;
    };
    let safe_reference = |value: &str| {
        value
            .trim()
            .chars()
            .filter(|character| !character.is_control())
            .take(128)
            .collect::<String>()
    };
    append_desktop_app_log(
        &storage,
        "warning",
        "instance.runtime_command.stdin_write_failed",
        "Managed stdin writer completed without a clean confirmation",
        json!({
            "instance_id": safe_reference(instance_id),
            "process_key": safe_reference(&target.process_key),
            "display_name": safe_reference(&target.display_name),
            "pid": target.pid,
            "failure_kind": failure_kind,
        }),
    );
}

pub(super) fn ensure_expected_supervisor_run(
    runtime: &app_runtime::RuntimeSupervisor,
    instance_id: &str,
    expected_run_id: Option<i64>,
) -> Result<(), String> {
    let Some(expected_run_id) = expected_run_id else {
        return Ok(());
    };
    let tracked_run_id = runtime
        .tracked_instances()
        .into_iter()
        .find(|instance| instance.summary.id == instance_id)
        .map(|instance| instance.run_id);
    if tracked_run_id != Some(expected_run_id) {
        return Err(String::from(
            "the managed server run changed before the live-player action could be dispatched",
        ));
    }
    Ok(())
}

pub(super) fn ensure_expected_supervisor_state(
    state: &tauri::State<'_, DesktopState>,
    instance_id: &str,
    expected_run_id: Option<i64>,
) -> Result<(), String> {
    let runtime = state
        .runtime_supervisor
        .lock()
        .map_err(|_| String::from("runtime supervisor lock poisoned"))?;
    ensure_expected_supervisor_run(&runtime, instance_id, expected_run_id)
}

#[cfg(test)]
mod completion_tests {
    use super::*;

    #[tokio::test]
    async fn managed_save_observes_writer_completion_after_confirmation_timeout() {
        let completion = RuntimeStdinWriteCompletion::default();
        let (release, ready) = tokio::sync::oneshot::channel();
        let target = app_runtime::RuntimeCommandDispatchTarget {
            process_key: String::from("main"),
            display_name: String::from("fixture"),
            pid: 42,
        };
        let completed_target = target.clone();
        let writer = tauri::async_runtime::spawn(async move {
            ready.await.unwrap();
            Ok(completed_target)
        });
        let receipt = await_runtime_stdin_writer_with_budget(
            "periodic-save-fixture",
            target,
            writer,
            RuntimeStdinDispatchBudget {
                confirmation_deadline: Instant::now(),
                submission_tracker: None,
                completion: Some(completion.clone()),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            receipt.confirmation,
            RuntimeStdinDispatchConfirmation::Pending
        );
        assert!(completion.result().unwrap().is_none());
        release.send(()).unwrap();
        let result = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(result) = completion.result().unwrap() {
                    break result;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn managed_save_completion_preserves_failed_write_result() {
        let completion = RuntimeStdinWriteCompletion::default();
        completion.record(&Err(String::from("fixture stdin write failed")));
        assert_eq!(
            completion.result().unwrap(),
            Some(Err(String::from("fixture stdin write failed")))
        );
    }
}
