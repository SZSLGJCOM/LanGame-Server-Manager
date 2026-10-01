use std::time::Duration;

use super::*;

pub(super) async fn wait_for_instance_exit(
    state: &DesktopState,
    instance_id: &str,
    timeout: Duration,
) -> Result<bool, String> {
    wait_until_exited(timeout, || {
        let runtime = Arc::clone(&state.runtime_supervisor);
        let instance_id = instance_id.to_owned();
        async move {
            tokio::task::spawn_blocking(move || {
                runtime
                    .lock()
                    .map_err(|_| String::from("runtime supervisor lock poisoned"))?
                    .instance_process_tree_is_running(&instance_id)
                    .map_err(|error| error.to_string())
            })
            .await
            .map_err(|error| format!("shutdown process inspection task failed: {error}"))?
        }
    })
    .await
}

async fn wait_until_exited<F>(
    timeout: Duration,
    mut inspect: impl FnMut() -> F,
) -> Result<bool, String>
where
    F: std::future::Future<Output = Result<Option<bool>, String>>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        // Only complete process-tree evidence permits an early return. Unknown
        // ownership keeps the module's full existing save/stop allowance.
        if inspect().await? == Some(false) {
            return Ok(true);
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Ok(false);
        }
        tokio::time::sleep_until((now + Duration::from_millis(100)).min(deadline)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shutdown_wait_completes_immediately_after_the_entire_tree_exits() {
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            wait_until_exited(Duration::from_secs(30), || {
                std::future::ready(Ok(Some(false)))
            }),
        )
        .await
        .expect("an exited server must not incur the fixed shutdown delay");
        assert_eq!(result, Ok(true));
    }

    #[tokio::test]
    async fn shutdown_wait_observes_exit_after_a_running_probe() {
        let mut probes = 0;
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            wait_until_exited(Duration::from_secs(30), || {
                probes += 1;
                std::future::ready(Ok(Some(probes == 1)))
            }),
        )
        .await
        .expect("a newly exited server must release its remaining allowance");
        assert_eq!(result, Ok(true));
        assert_eq!(probes, 2);
    }

    #[tokio::test]
    async fn shutdown_wait_does_not_invent_exit_for_unknown_or_live_processes() {
        for observed in [None, Some(true)] {
            assert_eq!(
                wait_until_exited(Duration::ZERO, || std::future::ready(Ok(observed))).await,
                Ok(false)
            );
        }
    }

    #[tokio::test]
    async fn shutdown_wait_preserves_process_inspection_failures() {
        assert_eq!(
            wait_until_exited(Duration::from_secs(30), || std::future::ready(Err(
                "inspection failed".into()
            )))
            .await,
            Err("inspection failed".into())
        );
    }
}
