use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::watch;

/// One shell owns one connection attempt and one final-exit intent. Publishing
/// readiness never replaces an already installed client's pinned process target.
pub(super) struct Startup {
    result: watch::Sender<Option<Result<(), String>>>,
    exit_requested: Arc<AtomicBool>,
}

impl Default for Startup {
    fn default() -> Self {
        let (result, _) = watch::channel(None);
        Self {
            result,
            exit_requested: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Startup {
    pub(super) fn cancellation(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.exit_requested)
    }

    pub(super) fn is_exit_requested(&self) -> bool {
        self.exit_requested.load(Ordering::SeqCst)
    }

    pub(super) fn is_complete(&self) -> bool {
        self.result.borrow().is_some()
    }

    pub(super) fn try_begin_exit(&self) -> bool {
        self.exit_requested
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub(super) fn complete(&self, result: Result<(), String>) {
        // Keep the first result for every waiter, including WebViews loading
        // after completion. Repeated publication cannot rewrite startup failure.
        self.result.send_if_modified(|current| {
            if current.is_some() {
                return false;
            }
            *current = Some(result);
            true
        });
    }

    pub(super) async fn wait(&self) -> Result<(), String> {
        let mut result = self.result.subscribe();
        loop {
            let completed = result.borrow_and_update().clone();
            if let Some(completed) = completed {
                return completed;
            }
            result
                .changed()
                .await
                .map_err(|_| String::from("Runtime service startup observation ended"))?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::poll_fn;
    use std::task::Poll;

    #[tokio::test]
    async fn startup_waiters_share_completion_and_late_waiters_observe_it() {
        let startup = Startup::default();
        assert!(!startup.is_complete());
        let mut first = Box::pin(startup.wait());
        let mut second = Box::pin(startup.wait());
        assert!(
            poll_fn(|cx| Poll::Ready(first.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        assert!(
            poll_fn(|cx| Poll::Ready(second.as_mut().poll(cx)))
                .await
                .is_pending()
        );

        startup.complete(Ok(()));
        assert!(startup.is_complete());
        assert_eq!(first.await, Ok(()));
        assert_eq!(second.await, Ok(()));
        assert_eq!(startup.wait().await, Ok(()));
    }

    #[tokio::test]
    async fn startup_failure_remains_available_without_retrying_or_replacing_it() {
        let startup = Startup::default();
        startup.complete(Err(String::from("Original service is still shutting down")));
        startup.complete(Ok(()));

        for _ in 0..2 {
            assert_eq!(
                startup.wait().await,
                Err(String::from("Original service is still shutting down"))
            );
        }
    }

    #[test]
    fn pending_exit_cancels_startup_once_and_keeps_its_intent_after_completion() {
        let startup = Startup::default();
        let cancellation = startup.cancellation();
        assert!(!cancellation.load(Ordering::SeqCst));
        assert!(startup.try_begin_exit());
        assert!(cancellation.load(Ordering::SeqCst));
        assert!(!startup.try_begin_exit());

        startup.complete(Err(String::from("Startup was cancelled")));
        assert!(startup.is_exit_requested());
        assert!(!startup.try_begin_exit());
    }
}
