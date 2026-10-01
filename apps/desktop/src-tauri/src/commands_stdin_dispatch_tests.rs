use super::*;

fn stdin_dispatch_target() -> app_runtime::RuntimeCommandDispatchTarget {
    app_runtime::RuntimeCommandDispatchTarget {
        process_key: String::from("main"),
        display_name: String::from("Main process"),
        pid: 4242,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn expired_stdin_confirmation_window_returns_pending_success() {
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let (completed_tx, completed_rx) = tokio::sync::oneshot::channel();
    let target = stdin_dispatch_target();
    let writer_target = target.clone();
    let writer = tauri::async_runtime::spawn_blocking(move || {
        release_rx.recv().expect("release delayed writer");
        let _ = completed_tx.send(());
        Ok(writer_target)
    });

    let receipt = await_runtime_stdin_writer_confirmation(
        "stdin-confirmation-pending",
        target,
        writer,
        Duration::from_millis(10),
    )
    .await
    .expect("an expired confirmation window is an accepted submission");

    assert_eq!(
        receipt.confirmation,
        RuntimeStdinDispatchConfirmation::Pending
    );
    release_tx.send(()).expect("release writer");
    tokio::time::timeout(Duration::from_secs(1), completed_rx)
        .await
        .expect("the retained writer must finish")
        .expect("writer completion signal");
}

#[tokio::test(flavor = "current_thread")]
async fn pending_stdin_writer_late_failure_is_safely_audited() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("stdin-writer-late-failure-audit");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let storage = bootstrap_storage().expect("bootstrap isolated storage");
    let log_path = desktop_app_log_path(&storage);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let target = stdin_dispatch_target();
    let writer = tauri::async_runtime::spawn_blocking(move || {
        release_rx.recv().expect("release delayed writer");
        Err(RuntimeStdinWriterFailure::Write)
    });

    let receipt = await_runtime_stdin_writer_confirmation(
        "stdin-late-failure",
        target,
        writer,
        Duration::from_millis(10),
    )
    .await
    .expect("the accepted submission must not become retryable after its confirmation window");
    assert_eq!(
        receipt.confirmation,
        RuntimeStdinDispatchConfirmation::Pending
    );

    release_tx.send(()).expect("release writer");
    let audit_text = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(text) = fs::read_to_string(&log_path)
                && text.contains("instance.runtime_command.stdin_write_failed")
            {
                break text;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("late writer failure audit");

    assert!(audit_text.contains("\"failure_kind\":\"write\""));
    assert!(!audit_text.contains("\"command\":"));
    let _ = fs::remove_dir_all(run_root);
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_confirmation_wait_keeps_writer_observed_and_audits_late_failure() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("stdin-writer-cancelled-confirmation");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let storage = bootstrap_storage().expect("bootstrap isolated storage");
    let log_path = desktop_app_log_path(&storage);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let target = stdin_dispatch_target();
    let writer = tauri::async_runtime::spawn_blocking(move || {
        release_rx.recv().expect("release delayed writer");
        Err(RuntimeStdinWriterFailure::Write)
    });
    let confirmation = await_runtime_stdin_writer_confirmation(
        "stdin-cancelled-confirmation",
        target,
        writer,
        Duration::from_secs(30),
    );

    assert!(
        tokio::time::timeout(Duration::from_millis(10), confirmation)
            .await
            .is_err(),
        "the outer collection deadline must cancel its confirmation wait",
    );
    release_tx.send(()).expect("release writer");
    let audit_text = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if let Ok(text) = fs::read_to_string(&log_path)
                && text.contains("instance.runtime_command.stdin_write_failed")
            {
                break text;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the immediate observer must audit failure after its caller is cancelled");

    assert!(audit_text.contains("\"failure_kind\":\"write\""));
    assert!(!audit_text.contains("\"command\":"));
    let _ = fs::remove_dir_all(run_root);
}
