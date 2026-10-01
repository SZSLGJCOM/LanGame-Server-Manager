use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Notify;

fn instance() -> InstanceDetails {
    serde_json::from_value(serde_json::json!({
        "summary":{"id":"astroneer-fixture","name":"Fixture","module_id":"astroneer",
            "status":"Running","bind_ip":"0.0.0.0","port_count":1,"autostart":false,"active_process_count":1},
        "config_file_path":"","saves_path":"","auto_backup_on_stop":false,"backup_retention_count":1,
        "settings_json":"{\"console_password\":\"fixture-password\"}",
        "ports":[{"name":"console","protocol":"tcp","port":1234}],
        "active_run":{"run_id":1,"session_id":"session-1","pid":123,"log_path":null,"process_count":1,
            "processes":[{"run_id":1,"session_id":"session-1","process_key":"main","display_name":"Fixture",
                "pid":123,"process_identity":{"creation_time":456,"image_path":"fixture.exe"},"status":"running",
                "started_at":null,"stopped_at":null,"exit_code":null,"crash_flag":false,"log_path":null,"is_primary":true}]}
    })).unwrap()
}

fn key() -> Key {
    context(&instance(), PathBuf::from("fixture.db"))
        .unwrap_or_else(|_| panic!("fixture context"))
        .key
}

#[test]
fn astroneer_health_start_preflight_requires_explicit_ipv4_only_for_astroneer() {
    for value in [
        "",
        " ",
        "not-an-address",
        "999.2.3.4",
        "::1",
        "203.0.113.1\n",
    ] {
        let mut details = instance();
        details.settings_json = serde_json::json!({"public_ip":value}).to_string();
        let error = crate::astroneer_console::validate_start_settings(&details).unwrap_err();
        assert!(error.contains("PublicIP"));
        assert!(!error.contains("999.2.3.4"));
        details.summary.module_id = "another-game".into();
        assert!(crate::astroneer_console::validate_start_settings(&details).is_ok());
    }
    let mut details = instance();
    assert!(crate::astroneer_console::validate_start_settings(&details).is_err());
    details.settings_json = serde_json::json!({"public_ip":"203.0.113.10"}).to_string();
    assert!(crate::astroneer_console::validate_start_settings(&details).is_ok());
}

#[test]
fn astroneer_health_requires_consistent_current_world_not_players_or_a_listening_port() {
    let good = serde_json::json!({"activeSaveName":"FixtureWorld","gameList":[]});
    let stats = serde_json::json!({"saveGameName":"FixtureWorld"});
    assert!(selected_world_matches(&good, &stats));
    for (games, statistics) in [
        (serde_json::json!({"playerInfo":[]}), stats.clone()),
        (
            serde_json::json!({"activeSaveName":"","gameList":[]}),
            serde_json::json!({"saveGameName":""}),
        ),
        (
            good.clone(),
            serde_json::json!({"saveGameName":"OtherWorld"}),
        ),
        (
            serde_json::json!({"activeSaveName":"FixtureWorld"}),
            stats.clone(),
        ),
        (
            serde_json::json!({"activeSaveName":"bad\nname","gameList":[]}),
            serde_json::json!({"saveGameName":"bad\nname"}),
        ),
        (good, serde_json::json!({"saveGameName":true})),
    ] {
        assert!(!selected_world_matches(&games, &statistics));
    }
    let public = serde_json::to_string(&Observation::Ready.health()).unwrap();
    assert!(!public.contains("FixtureWorld"));
}

#[test]
fn astroneer_health_context_rejects_old_or_ambiguous_runs_and_configuration() {
    let original = instance();
    let mut variants = Vec::new();
    let mut value = original.clone();
    value.active_run = None;
    variants.push(value);
    let mut value = original.clone();
    value.summary.status = InstanceStatus::Stopped;
    variants.push(value);
    let mut value = original.clone();
    value.active_run.as_mut().unwrap().processes[0].session_id = Some("other".into());
    variants.push(value);
    let mut value = original.clone();
    value.active_run.as_mut().unwrap().processes[0].pid = Some(124);
    variants.push(value);
    let mut value = original.clone();
    value.active_run.as_mut().unwrap().processes[0].process_identity = None;
    variants.push(value);
    let mut value = original.clone();
    value.active_run.as_mut().unwrap().processes[0].exit_code = Some(0);
    variants.push(value);
    let mut value = original.clone();
    value.ports.push(value.ports[0].clone());
    variants.push(value);
    let mut value = original.clone();
    value.settings_json = "{}".into();
    variants.push(value);
    for value in variants {
        assert!(context(&value, PathBuf::new()).is_err());
    }
    for status in ["error", "warning", "idle", "stopping"] {
        let mut health = Observation::Waiting.health();
        health.status = status.into();
        assert!(!eligible(&original, &health));
    }
}

#[test]
fn astroneer_health_generation_and_credentials_never_share_cached_evidence() {
    let registry = AstroneerHealthRegistry::default();
    let original = key();
    let cached = registry.cache(&original).unwrap().0;
    cached.lock().unwrap().store(Observation::Ready);
    let mut variants = Vec::new();
    let mut value = original.clone();
    value.run_id += 1;
    variants.push(value);
    let mut value = original.clone();
    value.session_id.push('2');
    variants.push(value);
    let mut value = original.clone();
    value.identity.creation_time += 1;
    variants.push(value);
    let mut value = original.clone();
    value.port += 1;
    variants.push(value);
    let mut value = original.clone();
    value.credential[0] ^= 1;
    variants.push(value);
    let mut value = original.clone();
    value.database = "other.db".into();
    variants.push(value);
    for changed in variants {
        assert!(
            registry
                .cache(&changed)
                .unwrap()
                .0
                .lock()
                .unwrap()
                .fresh(CACHE_TTL)
                .is_none()
        );
    }
    // A late old worker owns only its displaced cache, never the current generation's slot.
    cached.lock().unwrap().store(Observation::Ready);
    assert!(
        registry
            .cache(&original)
            .unwrap()
            .0
            .lock()
            .unwrap()
            .fresh(CACHE_TTL)
            .is_none()
    );
}

#[tokio::test]
async fn astroneer_health_single_flight_survives_request_cancellation_and_caches_failures() {
    let registry = Arc::new(AstroneerHealthRegistry::default());
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let worker_registry = registry.clone();
    let worker_entered = entered.clone();
    let worker_release = release.clone();
    let caller = tokio::spawn(async move {
        worker_registry
            .observe(&key(), async move {
                worker_entered.notify_one();
                worker_release.notified().await;
                Observation::Unavailable
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let duplicate = calls.clone();
    let second_key = key();
    let second = registry.observe(&second_key, async move {
        duplicate.fetch_add(1, Ordering::SeqCst);
        Observation::Ready
    });
    tokio::pin!(second);
    let pending = std::future::poll_fn(|cx| {
        std::task::Poll::Ready(std::future::Future::poll(second.as_mut(), cx))
    })
    .await;
    assert!(pending.is_pending());
    caller.abort();
    release.notify_one();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), second)
            .await
            .unwrap(),
        Observation::Unavailable
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        while registry
            .cache(&key())
            .unwrap()
            .0
            .lock()
            .unwrap()
            .fresh(CACHE_TTL)
            .is_none()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let duplicate = calls.clone();
    assert_eq!(
        registry
            .observe(&key(), async move {
                duplicate.fetch_add(1, Ordering::SeqCst);
                Observation::Ready
            })
            .await,
        Observation::Unavailable
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn astroneer_health_cache_has_a_fixed_instance_bound_and_does_not_evict_inflight_slots() {
    let registry = AstroneerHealthRegistry::default();
    let held = (0..MAX_INSTANCES)
        .map(|index| {
            let mut key = key();
            key.instance_id = format!("fixture-{index}");
            registry.cache(&key).unwrap()
        })
        .collect::<Vec<_>>();
    assert!(registry.cache(&key()).is_none());
    assert_eq!(registry.entries.lock().unwrap().len(), MAX_INSTANCES);
    drop(held);
    assert!(registry.cache(&key()).is_some());
    assert_eq!(registry.entries.lock().unwrap().len(), MAX_INSTANCES);
}

#[tokio::test]
async fn astroneer_health_expired_ready_readers_share_and_wait_for_one_fresh_result() {
    use std::future::{Future, poll_fn};
    use std::task::Poll;

    let registry = Arc::new(AstroneerHealthRegistry::default());
    assert_eq!(
        registry.observe(&key(), async { Observation::Ready }).await,
        Observation::Ready
    );
    // Exercise the real production TTL, not an empty-cache approximation.
    tokio::time::sleep(CACHE_TTL + Duration::from_millis(1)).await;
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let queries = Arc::new(AtomicUsize::new(0));
    let worker_registry = registry.clone();
    let worker_entered = entered.clone();
    let worker_release = release.clone();
    let worker_queries = queries.clone();
    let first = tokio::spawn(async move {
        worker_registry
            .observe(&key(), async move {
                worker_queries.fetch_add(1, Ordering::SeqCst);
                worker_entered.notify_one();
                worker_release.notified().await;
                Observation::Ready
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    let duplicate_queries = queries.clone();
    let second_key = key();
    let second = registry.observe(&second_key, async move {
        duplicate_queries.fetch_add(1, Ordering::SeqCst);
        Observation::Unavailable
    });
    tokio::pin!(second);
    // Poll deterministically while the actual refresher is behind a barrier.
    let pending = poll_fn(|cx| Poll::Ready(second.as_mut().poll(cx))).await;
    let waited = pending.is_pending();
    release.notify_one();
    let first_result = first.await.unwrap();
    let second_result = match pending {
        Poll::Ready(value) => value,
        Poll::Pending => tokio::time::timeout(Duration::from_secs(2), second)
            .await
            .unwrap(),
    };
    assert_eq!(first_result, Observation::Ready);
    assert_eq!(
        second_result,
        Observation::Ready,
        "a concurrent reader must not synthesize starting"
    );
    assert!(
        waited,
        "expired Ready cannot be served while the new query is unresolved"
    );
    assert_eq!(queries.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn astroneer_health_queries_two_authenticated_framed_responses() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        for (command, response) in [
            (
                "DSListGames",
                "{\"activeSaveName\":\"FixtureWorld\",\"gameList\":[]}\r\n",
            ),
            (
                "DSServerStatistics",
                "{\"saveGameName\":\"FixtureWorld\"}\r\n",
            ),
        ] {
            let (mut socket, _) = listener.accept().await.unwrap();
            let expected = format!("fixture-password\n{command}\n");
            let mut request = vec![0; expected.len()];
            socket.read_exact(&mut request).await.unwrap();
            assert_eq!(request, expected.as_bytes());
            for bytes in response.as_bytes().chunks(3) {
                socket.write_all(bytes).await.unwrap();
                tokio::task::yield_now().await;
            }
        }
    });
    assert_eq!(
        query_world(port, "fixture-password").await,
        Observation::Ready
    );
    server.await.unwrap();
}

#[tokio::test]
async fn astroneer_health_incomplete_or_over_limit_console_never_becomes_ready() {
    for response in [
        b"{\"activeSaveName\":\"FixtureWorld\",\"gameList\":[]}".to_vec(),
        vec![b' '; MAX_RESPONSE_BYTES + 1],
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 29];
            socket.read_exact(&mut request).await.unwrap();
            let _ = socket.write_all(&response).await;
        });
        assert_eq!(
            query_world(port, "fixture-password").await,
            Observation::Unavailable
        );
        server.await.unwrap();
    }
}
