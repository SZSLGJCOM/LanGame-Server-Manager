use super::*;
use std::collections::VecDeque;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn instance() -> InstanceDetails {
    serde_json::from_value(json!({
        "summary": { "id": "asa-mod-cluster", "name": "Mod cluster", "module_id": "arksurvivalascended",
            "status": "Stopped", "bind_ip": "127.0.0.1", "port_count": 8, "autostart": false },
        "config_file_path": "C:/fixture/config/instance.json", "saves_path": "C:/fixture/Saved",
        "auto_backup_on_stop": false, "backup_retention_count": 0,
        "settings_json": json!({ "map_name": "TheIsland_WP", "server_name": "Island",
            "additional_maps": [{ "id": "scorched", "map_name": "ScorchedEarth_WP", "name": "Scorched", "enabled": true }]
        }).to_string(),
        "ports": [
            { "name": "game", "protocol": "udp", "port": 7777 },
            { "name": "query", "protocol": "udp", "port": 27015 },
            { "name": "rcon", "protocol": "tcp", "port": 27020 },
            { "name": "map-scorched-game", "protocol": "udp", "port": 7787 },
            { "name": "map-scorched-query", "protocol": "udp", "port": 27025 },
            { "name": "map-scorched-rcon", "protocol": "tcp", "port": 27030 }
        ], "active_run": null
    })).unwrap()
}

fn endpoint(key: &str, port: u16, address: &str) -> ProcessNetworkEndpoint {
    ProcessNetworkEndpoint {
        protocol: "udp".into(),
        local_address: address.into(),
        local_port: port,
        owning_pid: 101,
        process_key: key.into(),
        relation: "tracked_process".into(),
    }
}

#[test]
fn asa_mod_startup_serializes_only_mod_enabled_asa_clusters() {
    for value in ["-mods=928708", "-passivemods=928708", "-MODS=928708"] {
        let values = args(&["TheIsland_WP", value]);
        assert!(
            requires_barrier("arksurvivalascended", 4, values.iter()),
            "{value}"
        );
        assert!(!requires_barrier("arksurvivalascended", 1, values.iter()));
        assert!(!requires_barrier("arksurvivalevolved", 4, values.iter()));
        assert!(!requires_barrier("dontstarve", 2, values.iter()));
    }
    for value in ["-mods=", "-passivemods= ", "-othermods=928708", "-log"] {
        assert!(
            !requires_barrier("arksurvivalascended", 4, args(&[value]).iter()),
            "{value}"
        );
    }
    assert!(requires_barrier(
        "arksurvivalascended",
        4,
        args(&["-mods", "928708"]).iter()
    ));
    assert!(!requires_barrier(
        "arksurvivalascended",
        4,
        args(&["-mods", "-log"]).iter()
    ));
    assert!(!requires_barrier(
        "arksurvivalascended",
        4,
        args(&["TheIsland_WP?SessionName=-mods=928708"]).iter()
    ));
}

#[test]
fn asa_mod_startup_barrier_requires_previous_map_game_listener_only() {
    let details = instance();
    let main = required_game_listener(&details, "main").unwrap();
    assert_eq!(
        main.iter()
            .map(|port| port.name.as_str())
            .collect::<Vec<_>>(),
        ["main:game"]
    );
    assert_eq!(
        super::super::super::commands_runtime_ark::evaluate_bind_ports(
            &main,
            "127.0.0.1",
            &[endpoint("main", 7777, "127.0.0.1")]
        ),
        StrictBindEvaluation::Ready
    );
    let child = required_game_listener(&details, "map-scorched").unwrap();
    assert_eq!(child[0].name, "map-scorched:game");
    assert_eq!(child[0].port, 7787);
    assert!(required_game_listener(&details, "map-missing").is_err());
}

#[test]
fn asa_mod_startup_never_accepts_another_maps_listener_or_wrong_address() {
    let required = required_game_listener(&instance(), "main").unwrap();
    let evaluate = |endpoints: &[ProcessNetworkEndpoint]| {
        super::super::super::commands_runtime_ark::evaluate_bind_ports(
            &required,
            "127.0.0.1",
            endpoints,
        )
    };
    assert!(matches!(
        evaluate(&[endpoint("map-scorched", 7777, "127.0.0.1")]),
        StrictBindEvaluation::Pending { .. }
    ));
    assert!(matches!(
        evaluate(&[endpoint("main", 7777, "0.0.0.0")]),
        StrictBindEvaluation::Failed { .. }
    ));
    assert!(matches!(
        evaluate(&[endpoint("main", 7787, "127.0.0.1")]),
        StrictBindEvaluation::Pending { .. }
    ));
    assert_eq!(
        super::super::super::commands_runtime_ark::evaluate_bind_ports(
            &required,
            "0.0.0.0",
            &[endpoint("main", 7777, "0.0.0.0")]
        ),
        StrictBindEvaluation::Ready
    );
}

fn reserve(state: &DesktopState) -> RuntimeStartReservationLease {
    match state
        .try_reserve_runtime_start("asa-mod-cluster", "manual")
        .unwrap()
    {
        RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        other => panic!("unexpected startup reservation: {other:?}"),
    }
}

fn pending() -> StrictBindEvaluation {
    StrictBindEvaluation::Pending {
        missing_port_names: vec!["main:game".into()],
    }
}

#[tokio::test]
async fn asa_mod_startup_waits_for_three_consecutive_owned_listener_samples() {
    let state = DesktopState::default();
    let reservation = reserve(&state);
    let mut observations = VecDeque::from([
        pending(),
        StrictBindEvaluation::Ready,
        pending(),
        StrictBindEvaluation::Ready,
        StrictBindEvaluation::Ready,
        StrictBindEvaluation::Ready,
    ]);
    let mut calls = 0;
    wait_for_owned_listener(
        Duration::from_secs(1),
        Duration::ZERO,
        &reservation,
        || Ok(()),
        || {
            calls += 1;
            std::future::ready(Ok(observations
                .pop_front()
                .expect("no extra inspection after ready")))
        },
    )
    .await
    .unwrap();
    assert_eq!(calls, 6);
    assert!(observations.is_empty());
}

#[tokio::test]
async fn asa_mod_startup_four_maps_do_not_overlap_cache_initialization() {
    let state = DesktopState::default();
    let reservation = reserve(&state);
    let mut events = vec![String::from("spawn:main")];
    for (previous, next) in [
        ("main", "map-scorched"),
        ("map-scorched", "map-center"),
        ("map-center", "map-extinction"),
    ] {
        let mut phase = 0;
        wait_for_owned_listener(
            Duration::from_secs(1),
            Duration::ZERO,
            &reservation,
            || Ok(()),
            || {
                phase += 1;
                let observation = if phase < 3 {
                    events.push(format!("cache-initializing:{previous}"));
                    pending()
                } else {
                    events.push(format!("owned-listener:{previous}"));
                    StrictBindEvaluation::Ready
                };
                std::future::ready(Ok(observation))
            },
        )
        .await
        .unwrap();
        assert_eq!(
            phase, 5,
            "the next map must not spawn while cache initialization is pending"
        );
        events.push(format!("spawn:{next}"));
    }
    assert_eq!(
        events
            .iter()
            .filter(|event| event.starts_with("spawn:"))
            .count(),
        4
    );
    for (previous, next) in [
        ("main", "map-scorched"),
        ("map-scorched", "map-center"),
        ("map-center", "map-extinction"),
    ] {
        let ready_index = events
            .iter()
            .rposition(|event| event == &format!("owned-listener:{previous}"))
            .unwrap();
        let spawn_index = events
            .iter()
            .position(|event| event == &format!("spawn:{next}"))
            .unwrap();
        assert!(ready_index < spawn_index);
    }
}

#[tokio::test]
async fn asa_mod_startup_failure_prevents_later_map_spawn() {
    let state = DesktopState::default();
    let reservation = reserve(&state);
    for observation in [
        Err(String::from("owned map exited")),
        Err(String::from("network inspection failed")),
        Ok(StrictBindEvaluation::Failed {
            message: "wrong bind address".into(),
        }),
    ] {
        let mut calls = 0;
        let result = wait_for_owned_listener(
            Duration::from_secs(1),
            Duration::ZERO,
            &reservation,
            || Ok(()),
            || {
                calls += 1;
                std::future::ready(observation.clone())
            },
        )
        .await;
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }
}

#[tokio::test]
async fn asa_mod_startup_bounds_a_stalled_endpoint_inspection() {
    let state = DesktopState::default();
    let reservation = reserve(&state);
    let error = tokio::time::timeout(
        Duration::from_secs(1),
        wait_for_owned_listener(
            Duration::from_millis(10),
            Duration::ZERO,
            &reservation,
            || Ok(()),
            || std::future::pending::<Result<StrictBindEvaluation, String>>(),
        ),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(error.contains("within 10 ms"), "{error}");
}

#[tokio::test]
async fn asa_mod_startup_pending_listener_times_out_instead_of_spawning_next() {
    let state = DesktopState::default();
    let reservation = reserve(&state);
    let error = wait_for_owned_listener(
        Duration::ZERO,
        Duration::ZERO,
        &reservation,
        || Ok(()),
        || std::future::ready(Ok(pending())),
    )
    .await
    .unwrap_err();
    assert!(error.contains("within 0 ms"), "{error}");
}

#[tokio::test]
async fn asa_mod_startup_cancel_interrupts_a_stalled_inspection() {
    let state = DesktopState::default();
    let reservation = reserve(&state);
    let mut wait = Box::pin(wait_for_owned_listener(
        Duration::from_secs(300),
        Duration::from_millis(500),
        &reservation,
        || Ok(()),
        || std::future::pending::<Result<StrictBindEvaluation, String>>(),
    ));
    std::future::poll_fn(|context| {
        assert!(std::future::Future::poll(wait.as_mut(), context).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    state.shutdown_in_progress.store(true, Ordering::SeqCst);
    assert!(
        state
            .drain_storage_operations_for_shutdown(Duration::ZERO)
            .await
            .is_err()
    );
    let error = tokio::time::timeout(Duration::from_secs(1), &mut wait)
        .await
        .unwrap()
        .unwrap_err();
    assert!(error.contains("cancelled"), "{error}");
    assert!(reservation.is_cancelled());
    drop(wait);
    drop(reservation);
    state
        .drain_storage_operations_for_shutdown(Duration::ZERO)
        .await
        .unwrap();
}

#[tokio::test]
async fn asa_mod_startup_rechecks_start_authorization_before_success() {
    let state = DesktopState::default();
    let reservation = reserve(&state);
    let calls = std::cell::Cell::new(0);
    let error = wait_for_owned_listener(
        Duration::from_secs(1),
        Duration::ZERO,
        &reservation,
        || {
            calls.set(calls.get() + 1);
            if calls.get() == 2 {
                Err(String::from("newer runtime action cancelled this start"))
            } else {
                Ok(())
            }
        },
        || std::future::ready(Ok(StrictBindEvaluation::Ready)),
    )
    .await
    .unwrap_err();
    assert_eq!(error, "newer runtime action cancelled this start");
    assert_eq!(calls.get(), 2);
}

#[cfg(windows)]
#[path = "commands_asa_mod_startup_native_tests.rs"]
mod native_tests;
