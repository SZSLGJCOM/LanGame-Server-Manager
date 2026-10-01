use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct CachedValue<T> {
    value: T,
    captured_at: Instant,
}

#[derive(Debug)]
pub struct TimedCache<T> {
    value: Option<CachedValue<T>>,
    refresh_in_flight: bool,
}

impl<T> Default for TimedCache<T> {
    fn default() -> Self {
        Self {
            value: None,
            refresh_in_flight: false,
        }
    }
}

impl<T: Clone> TimedCache<T> {
    pub fn fresh(&self, max_age: Duration) -> Option<T> {
        self.fresh_at(max_age, Instant::now())
    }

    fn fresh_at(&self, max_age: Duration, now: Instant) -> Option<T> {
        let cached = self.value.as_ref()?;
        if now.saturating_duration_since(cached.captured_at) <= max_age {
            Some(cached.value.clone())
        } else {
            None
        }
    }

    pub fn latest(&self) -> Option<T> {
        self.value.as_ref().map(|cached| cached.value.clone())
    }

    pub fn store(&mut self, value: T) {
        self.store_at(value, Instant::now());
    }

    fn store_at(&mut self, value: T, now: Instant) {
        self.value = Some(CachedValue {
            value,
            captured_at: now,
        });
        self.refresh_in_flight = false;
    }

    pub fn try_begin_refresh(&mut self) -> bool {
        if self.refresh_in_flight {
            return false;
        }
        self.refresh_in_flight = true;
        true
    }
}

struct CacheRefreshLease<T> {
    cache: Arc<Mutex<TimedCache<T>>>,
    completed: bool,
}

impl<T: Clone> CacheRefreshLease<T> {
    fn complete(mut self, result: Result<T, String>, cache_name: &str) -> Result<T, String> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| format!("{cache_name} cache lock poisoned"))?;
        self.completed = true;
        cache.refresh_in_flight = false;
        if let Ok(value) = &result {
            cache.store(value.clone());
        }
        result
    }
}

impl<T> Drop for CacheRefreshLease<T> {
    fn drop(&mut self) {
        if !self.completed
            && let Ok(mut cache) = self.cache.lock()
        {
            cache.refresh_in_flight = false;
        }
    }
}

/// Completes an already reserved refresh even if its requesting command is cancelled.
pub(crate) fn spawn_timed_cache_refresh<T, F>(
    cache: Arc<Mutex<TimedCache<T>>>,
    cache_name: &'static str,
    refresh: F,
) -> tauri::async_runtime::JoinHandle<Result<T, String>>
where
    T: Clone + Send + 'static,
    F: std::future::Future<Output = Result<T, String>> + Send + 'static,
{
    // The worker owns the reservation before the caller reaches its first await.
    // Dropping a request's handle detaches this worker; aborting the worker releases it.
    let lease = CacheRefreshLease {
        cache,
        completed: false,
    };
    tauri::async_runtime::spawn(async move {
        let result = refresh.await;
        lease.complete(result, cache_name)
    })
}

#[cfg(test)]
#[path = "timed_cache_tests.rs"]
mod tests;
