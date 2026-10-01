use super::*;

#[test]
fn reliability_timed_cache_expires_only_after_the_exact_ttl_boundary() {
    let now = Instant::now();
    let ttl = Duration::from_secs(10);
    let mut cache = TimedCache::<u32>::default();
    assert_eq!(cache.fresh_at(ttl, now), None);
    assert_eq!(cache.latest(), None);

    cache.store_at(42, now);
    assert_eq!(cache.fresh_at(ttl, now), Some(42));
    assert_eq!(cache.fresh_at(ttl, now + ttl), Some(42));
    assert_eq!(cache.latest(), Some(42));

    assert_eq!(
        cache.fresh_at(ttl, now + ttl + Duration::from_nanos(1)),
        None
    );
    assert_eq!(cache.latest(), Some(42));
}

#[test]
fn reliability_timed_cache_years_of_elapsed_time_expire_without_discarding_the_last_value() {
    let now = Instant::now();
    let later = now + Duration::from_secs(5 * 365 * 24 * 60 * 60);
    let mut cache = TimedCache::default();
    cache.store_at(42, now);
    assert_eq!(cache.fresh_at(Duration::from_secs(60), later), None);
    assert_eq!(cache.latest(), Some(42));
    cache.store_at(43, later);
    assert_eq!(cache.fresh_at(Duration::ZERO, later), Some(43));
    assert_eq!(
        cache.fresh_at(Duration::ZERO, later + Duration::from_nanos(1)),
        None
    );
    assert_eq!(cache.latest(), Some(43));
}

#[test]
fn timed_cache_allows_only_one_refresh_in_flight() {
    let mut cache = TimedCache::<u32>::default();

    assert!(cache.try_begin_refresh());
    assert!(!cache.try_begin_refresh());

    cache.store(7);
    assert!(cache.try_begin_refresh());
    assert!(!cache.try_begin_refresh());
}

#[tokio::test]
async fn reliability_cancelled_request_still_publishes_refresh_and_releases_reservation() {
    let cache = Arc::new(Mutex::new(TimedCache::<u32>::default()));
    assert!(cache.lock().unwrap().try_begin_refresh());
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let request_cache = Arc::clone(&cache);
    let request = tokio::spawn(async move {
        spawn_timed_cache_refresh(request_cache, "test", async move {
            started_tx.send(()).unwrap();
            release_rx.await.unwrap();
            Ok(42)
        })
        .await
    });
    started_rx.await.unwrap();
    assert!(!cache.lock().unwrap().try_begin_refresh());
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    release_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while cache.lock().unwrap().latest() != Some(42) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("detached refresh must publish its result");

    let mut cache = cache.lock().unwrap();
    assert_eq!(cache.latest(), Some(42));
    assert!(cache.try_begin_refresh());
}

#[tokio::test]
async fn reliability_failed_refresh_preserves_the_last_value_and_allows_retry() {
    let cache = Arc::new(Mutex::new(TimedCache::<u32>::default()));
    {
        let mut cache = cache.lock().unwrap();
        cache.store(7);
        assert!(cache.try_begin_refresh());
    }
    let result = spawn_timed_cache_refresh(Arc::clone(&cache), "test", async {
        Err(String::from("probe failed"))
    })
    .await
    .unwrap();

    assert_eq!(result.unwrap_err(), "probe failed");
    let mut cache = cache.lock().unwrap();
    assert_eq!(cache.latest(), Some(7));
    assert!(cache.try_begin_refresh());
}

#[tokio::test]
async fn reliability_aborted_worker_keeps_stale_value_and_releases_the_refresh_reservation() {
    let now = Instant::now();
    let later = now + Duration::from_secs(365 * 24 * 60 * 60);
    let cache = Arc::new(Mutex::new(TimedCache::<u32>::default()));
    {
        let mut cache = cache.lock().unwrap();
        cache.store_at(7, now);
        assert!(cache.try_begin_refresh());
    }
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let worker = spawn_timed_cache_refresh(Arc::clone(&cache), "test", async move {
        started_tx.send(()).unwrap();
        std::future::pending::<Result<u32, String>>().await
    });
    started_rx.await.unwrap();
    worker.abort();
    assert!(worker.await.is_err());
    {
        let mut cache = cache.lock().unwrap();
        assert_eq!(cache.fresh_at(Duration::from_secs(60), later), None);
        assert_eq!(cache.latest(), Some(7));
        assert!(cache.try_begin_refresh());
    }
    assert_eq!(
        spawn_timed_cache_refresh(Arc::clone(&cache), "retry", async { Ok(8) })
            .await
            .unwrap()
            .unwrap(),
        8
    );
    let mut cache = cache.lock().unwrap();
    assert_eq!(cache.latest(), Some(8));
    assert!(cache.try_begin_refresh());
}

#[tokio::test]
async fn reliability_panicked_worker_releases_the_refresh_reservation() {
    let cache = Arc::new(Mutex::new(TimedCache::<u32>::default()));
    assert!(cache.lock().unwrap().try_begin_refresh());
    let worker = spawn_timed_cache_refresh(Arc::clone(&cache), "test", async {
        panic!("synthetic probe panic");
    });

    assert!(worker.await.is_err());
    assert!(cache.lock().unwrap().try_begin_refresh());
}
