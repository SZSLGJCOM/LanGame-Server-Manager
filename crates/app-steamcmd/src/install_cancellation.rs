use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Notify;

tokio::task_local! {
    static CURRENT: InstallCancellation;
}

/// Cooperatively cancels one installation and its owned resources.
#[derive(Clone, Debug, Default)]
pub struct InstallCancellation {
    state: Arc<CancellationState>,
}

#[derive(Debug, Default)]
struct CancellationState {
    cancelled: AtomicBool,
    notification: Notify,
}

impl InstallCancellation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        if !self.state.cancelled.swap(true, Ordering::AcqRel) {
            self.state.notification.notify_waiters();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }

    pub(super) async fn scope<F: Future>(&self, future: F) -> F::Output {
        CURRENT.scope(self.clone(), future).await
    }

    pub(crate) fn current() -> Option<Self> {
        CURRENT.try_with(Clone::clone).ok()
    }

    pub async fn cancelled(&self) {
        // Register before checking the atomic flag so cancellation cannot be
        // lost between observing the flag and waiting for a notification.
        let notified = self.state.notification.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if !self.is_cancelled() {
            notified.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_wakes_every_waiter_and_remains_latched() {
        let cancellation = InstallCancellation::new();
        let first = cancellation.cancelled();
        let second = cancellation.cancelled();
        let cancel = async { cancellation.cancel() };
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            tokio::join!(first, second, cancel);
            cancellation.cancelled().await;
        })
        .await
        .expect("all cancellation waiters wake");
        assert!(cancellation.is_cancelled());
    }

    #[tokio::test]
    async fn scopes_restore_the_previous_operation_token() {
        let outer = InstallCancellation::new();
        let inner = InstallCancellation::new();
        inner.cancel();
        outer
            .scope(async {
                assert!(!InstallCancellation::current().unwrap().is_cancelled());
                inner
                    .scope(async {
                        assert!(InstallCancellation::current().unwrap().is_cancelled());
                    })
                    .await;
                assert!(!InstallCancellation::current().unwrap().is_cancelled());
            })
            .await;
        assert!(InstallCancellation::current().is_none());
    }
}
