use std::future::Future;
use std::time::Duration;

use app_network::NetworkError;
use reqwest::StatusCode;
use tokio::sync::Mutex;
use tokio::time::Instant;

const DEFAULT_COOLDOWN: Duration = Duration::from_secs(60);

struct Deferral {
    started: Instant,
    duration: Duration,
    status: StatusCode,
    origin: String,
}

#[derive(Default)]
pub(super) struct CommunityGate {
    // Both Community connections share one owner and one retry window. Holding
    // this across the request prevents queued lookups racing a newly received 429.
    deferred: Mutex<Option<Deferral>>,
}

impl CommunityGate {
    pub(super) const fn new() -> Self {
        Self {
            deferred: Mutex::const_new(None),
        }
    }

    pub(super) async fn read<T, F, Fut>(&self, budget: Duration, read: F) -> Result<T, NetworkError>
    where
        F: FnOnce(Duration) -> Fut,
        Fut: Future<Output = Result<T, NetworkError>>,
    {
        let deadline = Instant::now() + budget;
        let mut deferred = tokio::time::timeout_at(deadline, self.deferred.lock())
            .await
            .map_err(|_| NetworkError::Deadline {
                attempts: 0,
                origin: "https://steamcommunity.com".into(),
            })?;
        if let Some(window) = deferred.as_ref() {
            let remaining = window.duration.saturating_sub(window.started.elapsed());
            if !remaining.is_zero() {
                return Err(NetworkError::RetryDeferred {
                    status: window.status,
                    origin: window.origin.clone(),
                    retry_after: Some(remaining),
                });
            }
        }
        *deferred = None;
        let result = read(deadline.saturating_duration_since(Instant::now())).await;
        let failure = match &result {
            Err(NetworkError::RetryDeferred {
                status,
                origin,
                retry_after,
            }) => Some((*status, origin, retry_after.unwrap_or(DEFAULT_COOLDOWN))),
            Err(NetworkError::Status { status, origin })
                if *status == StatusCode::TOO_MANY_REQUESTS =>
            {
                Some((*status, origin, DEFAULT_COOLDOWN))
            }
            _ => None,
        };
        if let Some((status, origin, duration)) = failure {
            // A zero server delay permits a later retry, not a queued burst.
            let duration = duration.max(Duration::from_secs(1));
            *deferred = Some(Deferral {
                started: Instant::now(),
                duration,
                status,
                origin: origin.clone(),
            });
            return Err(NetworkError::RetryDeferred {
                status,
                origin: origin.clone(),
                retry_after: Some(duration),
            });
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn throttling_blocks_later_reads_and_keeps_the_server_retry_window() {
        let gate = CommunityGate::new();
        let calls = AtomicUsize::new(0);
        let first: Result<(), _> = gate
            .read(Duration::from_secs(2), |_| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err(NetworkError::RetryDeferred {
                    status: StatusCode::TOO_MANY_REQUESTS,
                    origin: "https://steamcommunity-a.akamaihd.net".into(),
                    retry_after: Some(Duration::from_secs(120)),
                })
            })
            .await;
        assert!(first.is_err());
        let next = gate
            .read(Duration::from_secs(2), |_| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(
            matches!(next, Err(NetworkError::RetryDeferred { retry_after: Some(delay), .. })
            if delay > Duration::from_secs(119) && delay <= Duration::from_secs(120))
        );
        // Expire the same window without real sleeps or timing-dependent retries.
        gate.deferred.lock().await.as_mut().unwrap().started =
            Instant::now() - Duration::from_secs(121);
        assert!(
            gate.read(Duration::from_secs(2), |_| async { Ok(()) })
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn headerless_429_uses_a_bounded_local_cooldown() {
        let gate = CommunityGate::new();
        let result: Result<(), _> = gate
            .read(Duration::from_secs(2), |_| async {
                Err(NetworkError::Status {
                    status: StatusCode::TOO_MANY_REQUESTS,
                    origin: "https://steamcommunity.com".into(),
                })
            })
            .await;
        assert!(matches!(
            result,
            Err(NetworkError::RetryDeferred {
                retry_after: Some(DEFAULT_COOLDOWN),
                ..
            })
        ));
    }
}
