use super::*;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const WORLD: &str = "palworld-dedicated-server-1-aaa5a218068b32bb";

fn instance() -> InstanceDetails {
    serde_json::from_value(json!({
        "summary":{"id":"palworld-fixture","name":"Fixture","module_id":"palworld",
            "status":"Running","bind_ip":"0.0.0.0","port_count":1,"autostart":false,"active_process_count":1},
        "config_file_path":"","saves_path":"","auto_backup_on_stop":false,"backup_retention_count":1,
        "settings_json":"{\"rest_api_enabled\":true,\"admin_password\":\"fixture-password\"}",
        "ports":[{"name":"rest_api","protocol":"tcp","port":8212}],
        "active_run":{"run_id":1,"session_id":"session-1","pid":123,"log_path":null,"process_count":1,
            "processes":[{"run_id":1,"session_id":"session-1","process_key":"main","display_name":"Fixture",
                "pid":123,"process_identity":{"creation_time":456,"image_path":"fixture.exe"},"status":"running",
                "started_at":null,"stopped_at":null,"exit_code":null,"crash_flag":false,"log_path":null,"is_primary":true}]}
    })).unwrap()
}

fn key() -> Key {
    context(&instance(), PathBuf::from("fixture.db"))
        .unwrap()
        .key
}

fn sample(uptime: f64) -> Observation {
    Observation::World {
        sample: WorldSample {
            world: WORLD.into(),
            uptime,
        },
        advancing: false,
    }
}

fn starting() -> RuntimeHealth {
    RuntimeHealth {
        status: "starting".into(),
        summary: "Original log interpretation".into(),
        reason: RuntimeHealthReason {
            code: "starting_tasks".into(),
            params: Default::default(),
        },
        matched_line: None,
    }
}

#[test]
fn palworld_health_requires_active_world_positive_fps_and_advancing_uptime() {
    let info = json!({"version":"v1.0.5.102999", "worldguid":WORLD}).to_string();
    let metrics = json!({"serverfps":60,"uptime":71}).to_string();
    assert_eq!(
        world_sample(info.as_bytes(), metrics.as_bytes()),
        match sample(71.0) {
            Observation::World { sample, .. } => Some(sample),
            _ => unreachable!(),
        }
    );
    assert!(
        world_sample(
            json!({"version":"v1", "worldguid":"aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"})
                .to_string()
                .as_bytes(),
            metrics.as_bytes()
        )
        .is_some()
    );
    for info in [
        "{}".to_string(),
        json!({"version":"v1", "worldguid":""}).to_string(),
        json!({"version":"v1", "worldguid":"invalid\nworld"}).to_string(),
        json!({"version":"v1", "worldguid":"a".repeat(257)}).to_string(),
        json!({"version":"", "worldguid":WORLD}).to_string(),
    ] {
        assert!(world_sample(info.as_bytes(), metrics.as_bytes()).is_none());
    }
    for metrics in [
        "{}",
        r#"{"serverfps":0,"uptime":71}"#,
        r#"{"serverfps":-1,"uptime":71}"#,
        r#"{"serverfps":60,"uptime":-1}"#,
        r#"{"serverfps":60,"uptime":"71"}"#,
        r#"{"serverfps":1e999,"uptime":71}"#,
    ] {
        assert!(world_sample(info.as_bytes(), metrics.as_bytes()).is_none());
    }
    assert!(!sample(71.0).with_previous(None).is_ready());
    assert!(!sample(71.0).with_previous(Some(&sample(71.0))).is_ready());
    assert!(!sample(70.0).with_previous(Some(&sample(71.0))).is_ready());
    assert!(sample(72.0).with_previous(Some(&sample(71.0))).is_ready());
    let mut changed = sample(72.0);
    if let Observation::World { sample, .. } = &mut changed {
        sample.world = "different-world".into();
    }
    assert!(!changed.with_previous(Some(&sample(71.0))).is_ready());
    assert!(
        !sample(72.0)
            .with_previous(Some(&Observation::Unavailable))
            .is_ready()
    );
}

#[tokio::test]
async fn palworld_health_preserves_errors_stops_other_games_and_disabled_rest() {
    let state = DesktopState::default();
    let paths = StoragePaths::default();
    let original = instance();
    for status in ["error", "warning", "idle", "ready"] {
        let mut health = starting();
        health.status = status.into();
        let before = serde_json::to_value(&health).unwrap();
        apply(&state, &paths, &original, &mut health).await;
        assert_eq!(serde_json::to_value(health).unwrap(), before);
    }
    for status in [
        InstanceStatus::Stopped,
        InstanceStatus::Stopping,
        InstanceStatus::Error,
    ] {
        let mut details = original.clone();
        details.summary.status = status;
        let mut health = starting();
        apply(&state, &paths, &details, &mut health).await;
        assert_eq!(health.summary, starting().summary);
    }
    let mut disabled = original.clone();
    disabled.settings_json =
        json!({"rest_api_enabled":false,"admin_password":"fixture-password"}).to_string();
    assert!(context(&disabled, PathBuf::new()).is_none());
    let mut health = starting();
    apply(&state, &paths, &disabled, &mut health).await;
    assert_eq!(health.summary, starting().summary);
    disabled.summary.module_id = "valheim".into();
    assert!(!eligible(&disabled, &starting()));
}

#[test]
fn palworld_health_rejects_inconsistent_run_and_endpoint_context() {
    let original = instance();
    let mut variants = Vec::new();
    let mut value = original.clone();
    value.active_run = None;
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
    value.active_run.as_mut().unwrap().process_count = 2;
    variants.push(value);
    let mut value = original.clone();
    value.ports.push(value.ports[0].clone());
    variants.push(value);
    let mut value = original.clone();
    value.settings_json = "{}".into();
    variants.push(value);
    for value in variants {
        assert!(context(&value, PathBuf::new()).is_none());
    }
}

#[test]
fn palworld_health_generation_and_credentials_do_not_reuse_observations() {
    let registry = PalworldHealthRegistry::default();
    let original = key();
    let old = registry.cache(&original).unwrap().0;
    old.lock()
        .unwrap()
        .store(sample(72.0).with_previous(Some(&sample(71.0))));
    let mut variants = Vec::new();
    let mut changed = original.clone();
    changed.run_id += 1;
    variants.push(changed);
    let mut changed = original.clone();
    changed.session_id.push('2');
    variants.push(changed);
    let mut changed = original.clone();
    changed.identity.creation_time += 1;
    variants.push(changed);
    let mut changed = original.clone();
    changed.pid += 1;
    variants.push(changed);
    let mut changed = original.clone();
    changed.port += 1;
    variants.push(changed);
    let mut changed = original.clone();
    changed.credential[0] ^= 1;
    variants.push(changed);
    let mut changed = original.clone();
    changed.database = "other.db".into();
    variants.push(changed);
    for changed in variants {
        assert!(
            registry
                .cache(&changed)
                .unwrap()
                .0
                .lock()
                .unwrap()
                .latest()
                .is_none()
        );
    }
    old.lock().unwrap().store(sample(100.0));
    assert!(
        registry
            .cache(&original)
            .unwrap()
            .0
            .lock()
            .unwrap()
            .latest()
            .is_none()
    );
}

#[test]
fn palworld_health_keeps_fatal_logs_written_during_a_ready_probe() {
    let current = instance();
    let mut health = RuntimeHealth {
        status: "error".into(),
        summary: "A fatal runtime error pattern was detected in the recent log.".into(),
        reason: RuntimeHealthReason {
            code: "fatal_log_pattern".into(),
            params: Default::default(),
        },
        matched_line: Some("Fatal error: fixture".into()),
    };
    let expected = health.clone();
    apply_ready_observation(&key(), true, &current, &mut health);
    assert_eq!(json!(health), json!(expected));
}

#[test]
fn palworld_health_keeps_stop_requested_during_a_ready_probe() {
    let mut current = instance();
    current.summary.status = InstanceStatus::Stopping;
    let mut health = RuntimeHealth {
        status: "stopping".into(),
        summary: "The server is stopping.".into(),
        reason: RuntimeHealthReason {
            code: "stopping".into(),
            params: Default::default(),
        },
        matched_line: None,
    };
    let expected = health.clone();
    apply_ready_observation(&key(), true, &current, &mut health);
    assert_eq!(json!(health), json!(expected));
    // The database stop flag is authoritative even before its log interpretation catches up.
    let mut pending_log = starting();
    let expected = pending_log.clone();
    apply_ready_observation(&key(), true, &current, &mut pending_log);
    assert_eq!(json!(pending_log), json!(expected));
}

#[test]
fn palworld_health_rejects_settings_changed_during_a_ready_probe() {
    let original = instance();
    let expected_key = key();
    let mut variants = Vec::new();
    let mut current = original.clone();
    current.settings_json =
        r#"{"rest_api_enabled":true,"admin_password":"fixture-admin-password"}"#.into();
    variants.push(current);
    let mut current = original.clone();
    current.settings_json =
        r#"{"rest_api_enabled":false,"admin_password":"fixture-password"}"#.into();
    variants.push(current);
    let mut current = original.clone();
    current.ports[0].port += 1;
    variants.push(current);
    for current in variants {
        let mut health = starting();
        let expected = health.clone();
        apply_ready_observation(&expected_key, true, &current, &mut health);
        assert_eq!(json!(health), json!(expected));
    }
    let mut health = starting();
    apply_ready_observation(&expected_key, false, &original, &mut health);
    assert_eq!(json!(health), json!(starting()));
    apply_ready_observation(&expected_key, true, &original, &mut health);
    assert_eq!(health.status, "ready");
    assert_eq!(health.reason.code, "ready_signal");
}

#[tokio::test]
async fn palworld_health_requires_the_exact_process_to_own_the_loopback_rest_port() {
    // Use the production netstat path before checking both the expected owner
    // and a conflicting owner; fabricated endpoint lists could hide filtering.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut current = key();
    current.port = listener.local_addr().unwrap().port();
    current.pid = std::process::id();
    current.identity = app_runtime::inspect_process_identity(current.pid)
        .unwrap()
        .unwrap();
    let mut owners = WindowsPlatform::loopback_tcp_listener_owners(current.port).unwrap();
    assert!(endpoint_matches(current.pid, &owners));
    let other_pid = current.pid + 1;
    assert!(!endpoint_matches(other_pid, &owners));
    owners.insert(other_pid);
    assert!(!endpoint_matches(current.pid, &owners));
    assert!(!endpoint_matches(current.pid, &BTreeSet::new()));
    assert!(endpoint_owned(current.clone()).await);
    let mut stale_identity = current.clone();
    stale_identity.identity.creation_time += 1;
    assert!(!endpoint_owned(stale_identity).await);
    listener.set_nonblocking(true).unwrap();
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[tokio::test]
async fn palworld_health_single_flight_survives_caller_cancellation() {
    let registry = Arc::new(PalworldHealthRegistry::default());
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
        sample(100.0)
    });
    tokio::pin!(second);
    assert!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(std::future::Future::poll(
            second.as_mut(),
            cx
        )))
        .await
        .is_pending()
    );
    caller.abort();
    release.notify_one();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), second)
            .await
            .unwrap(),
        Observation::Unavailable
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn palworld_health_refresh_requires_two_samples_from_the_same_run() {
    let registry = PalworldHealthRegistry::default();
    let original = key();
    assert!(
        !registry
            .observe(&original, async { sample(71.0) })
            .await
            .is_ready()
    );
    // Exercise the production cache lifetime before obtaining the next native sample.
    tokio::time::sleep(CACHE_TTL + Duration::from_millis(1)).await;
    assert!(
        registry
            .observe(&original, async { sample(72.0) })
            .await
            .is_ready()
    );
    let mut restarted = original;
    restarted.run_id += 1;
    assert!(
        !registry
            .observe(&restarted, async { sample(73.0) })
            .await
            .is_ready()
    );
}

#[test]
fn palworld_health_cache_is_bounded_and_does_not_evict_active_readers() {
    let registry = PalworldHealthRegistry::default();
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
}

async fn serve(listener: TcpListener, responses: Vec<(&'static str, u16, String)>) {
    for (path, status, body) in responses {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        loop {
            let mut chunk = [0; 1024];
            let count = socket.read(&mut chunk).await.unwrap();
            assert!(count > 0);
            request.extend_from_slice(&chunk[..count]);
            assert!(request.len() < 16 * 1024);
            if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                break;
            }
        }
        let request = String::from_utf8(request).unwrap();
        assert!(request.starts_with(&format!("GET {path} HTTP/1.1\r\n")));
        assert!(request.lines().any(|line| {
            line.split_once(':').is_some_and(|(key, value)| {
                key.eq_ignore_ascii_case("authorization")
                    && value.trim() == "Basic YWRtaW46Zml4dHVyZS1wYXNzd29yZA=="
            })
        }));
        let response = format!(
            "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(response.as_bytes()).await.unwrap();
    }
}

#[tokio::test]
async fn palworld_health_uses_authenticated_read_only_info_and_metrics() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(serve(
        listener,
        vec![
            (
                "/v1/api/info",
                200,
                json!({"version":"v1.0.5.102999","worldguid":WORLD}).to_string(),
            ),
            (
                "/v1/api/metrics",
                200,
                json!({"serverfps":60,"uptime":71}).to_string(),
            ),
        ],
    ));
    let observed = query_world(port, "fixture-password").await;
    assert_eq!(observed, sample(71.0));
    assert!(
        !observed.is_ready(),
        "one HTTP success cannot establish advancing world state"
    );
    server.await.unwrap();
}

#[tokio::test]
async fn palworld_health_rejects_success_without_world_data_and_authentication_failures() {
    for (status, responses) in [
        (
            200,
            vec![
                ("/v1/api/info", 200, "{}".to_string()),
                ("/v1/api/metrics", 200, "{}".to_string()),
            ],
        ),
        (401, vec![("/v1/api/info", 401, "{}".to_string())]),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(serve(listener, responses));
        assert_eq!(
            query_world(port, "fixture-password").await,
            Observation::Unavailable,
            "HTTP {status}"
        );
        server.await.unwrap();
    }
}
