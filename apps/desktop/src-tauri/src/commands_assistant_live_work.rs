use super::*;

/// A deadline stops further steps, but cannot undo a submitted blocking write.
/// Keep ownership until that operation finishes, then let the caller clean up.
pub(super) async fn drain_live_work(
    deadline: tokio::time::Instant,
    operation: impl std::future::Future<Output = LiveResult>,
) -> LiveResult {
    tokio::pin!(operation);
    // A ready future can win timeout_at even after expiry. Check the real
    // deadline around it, while retaining ownership of all submitted work.
    let drained = if tokio::time::Instant::now() >= deadline {
        operation.as_mut().await
    } else {
        match tokio::time::timeout_at(deadline, operation.as_mut()).await {
            Ok(result) if tokio::time::Instant::now() < deadline => return result,
            Ok(result) => result,
            Err(_) => operation.as_mut().await,
        }
    };
    let message = "native launch exceeded its 15-minute watchdog; in-flight work drained";
    match drained {
        Ok(()) => Err(message.into()),
        Err(error) => Err(format!("{message}; operation failed: {error}").into()),
    }
}

#[tokio::test]
async fn expired_watchdog_drains_submitted_work_before_cleanup() {
    let (started, observed_start) = tokio::sync::oneshot::channel();
    let (finish, completed) = tokio::sync::oneshot::channel();
    let completion = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let completed_flag = Arc::clone(&completion);
    let worker = tokio::spawn(async move {
        observed_start.await.expect("operation entered");
        finish.send(()).expect("operation retained its receiver");
    });
    let operation = async move {
        started.send(()).expect("watchdog observer retained");
        completed.await.expect("submitted worker completed");
        completed_flag.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    };
    let result = drain_live_work(tokio::time::Instant::now(), operation).await;
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("in-flight work drained")
    );
    assert!(completion.load(std::sync::atomic::Ordering::SeqCst));
    worker.await.expect("worker joined before cleanup");
}

#[tokio::test]
async fn expired_watchdog_preserves_drained_failure() {
    let operation = async {
        tokio::task::yield_now().await;
        Err("native worker failure".into())
    };
    let error = drain_live_work(tokio::time::Instant::now(), operation)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("in-flight work drained"));
    assert!(error.contains("native worker failure"));
}
