use super::*;
use std::sync::Arc;

const ID: &str = "1234567890abcdef1234567890abcdef";
const TOKEN: &str = "abcdef1234567890abcdef1234567890";
const LAUNCH: &str = "LGSM_UWS_abcdef1234567890abcdef1234567890";

fn request() -> Request<'static> {
    Request {
        protocol: 1,
        request_id: ID,
        requested_at: 100,
        action: "save",
        boot_id: ID,
        world_token: TOKEN,
        launch_token: LAUNCH,
    }
}
fn packet() -> serde_json::Value {
    serde_json::json!({"protocol":1,"request_id":ID,"timestamp":100,"action":"save",
        "boot_id":ID,"world_token":TOKEN,"ready":true,"invoked":true,"call_returned":true,
        "write_confirmed":false,"error":null,"launch_token":LAUNCH})
}

fn check_lua_host_receipt_contract(lua: &mlua::Lua, confirmed_save: bool) {
    for action in ["PROBE", "SAVE"] {
        let request_text: String = lua
            .globals()
            .get(format!("LGSM_TEST_{action}_REQUEST"))
            .unwrap();
        let reply_text: String = lua
            .globals()
            .get(format!("LGSM_TEST_{action}_REPLY"))
            .unwrap();
        let frame: serde_json::Value = serde_json::from_str(&request_text).unwrap();
        let launch = format!("LGSM_UWS_{}", "a".repeat(32));
        let request = Request {
            protocol: 1,
            request_id: frame["request_id"].as_str().unwrap(),
            requested_at: frame["requested_at"].as_u64().unwrap(),
            action: frame["action"].as_str().unwrap(),
            boot_id: frame["boot_id"].as_str().unwrap(),
            world_token: frame["world_token"].as_str().unwrap(),
            launch_token: &launch,
        };
        let reply = parse(reply_text.as_bytes(), &request).unwrap();
        let native = process::NativeProcess {
            pid: 11,
            identity: ProcessIdentity {
                image_path: "D:/fixture/native.exe".into(),
                creation_time: 123456789012345678,
            },
        };
        validate_native_identity(&reply, Some(&native)).unwrap();
        assert!(reply.ready);
        assert_eq!(reply.write_confirmed, action == "SAVE" && confirmed_save);
        assert_eq!(reply.invoked, action == "SAVE");
        if action == "SAVE" && confirmed_save {
            assert!(reply.call_returned && reply.error.is_none());
            for (field, invalid) in [
                ("native_process_id", serde_json::json!(12)),
                (
                    "native_creation_time",
                    serde_json::json!("123456789012345679"),
                ),
                ("native_process_id", serde_json::Value::Null),
                ("native_creation_time", serde_json::Value::Null),
            ] {
                let mut changed: serde_json::Value = serde_json::from_str(&reply_text).unwrap();
                changed[field] = invalid;
                let altered = parse(&serde_json::to_vec(&changed).unwrap(), &request).unwrap();
                assert!(
                    validate_native_identity(&altered, Some(&native)).is_err(),
                    "{field}"
                );
            }
            assert!(
                classify(Ok(reply))
                    .unwrap()
                    .contains("completed successfully")
            );
        }
    }
}

#[test]
fn unreal_world_save_lua_runs_both_actual_bundled_payloads() {
    for (game, name) in [(Game::Abiotic, "abiotic"), (Game::Moria, "moria")] {
        // Repository-owned fixture supplies every io/debug/UObject boundary;
        // Lua::new does not enable unsafe native module loading or a game DLL.
        let lua = mlua::Lua::new();
        lua.globals()
            .set(
                "LGSM_TEST_SOURCE",
                lua.create_string(game.payload()).unwrap(),
            )
            .unwrap();
        lua.globals().set("LGSM_TEST_GAME", name).unwrap();
        let cases: usize = lua
            .load(include_str!("unreal_save_fixture.lua"))
            .eval()
            .unwrap();
        assert_eq!(
            cases, 23,
            "each real game policy must exercise every native-save fixture"
        );
    }
}

#[test]
fn native_world_save_policies_run_real_nightingale_and_conan_payloads() {
    for (game, name) in [(Game::Nightingale, "nightingale"), (Game::Conan, "conan")] {
        let lua = mlua::Lua::new();
        lua.globals()
            .set(
                "LGSM_TEST_SOURCE",
                lua.create_string(game.payload()).unwrap(),
            )
            .unwrap();
        lua.globals().set("LGSM_TEST_GAME", name).unwrap();
        let cases: usize = lua
            .load(include_str!(
                "../../native/tests/native_save_policy_fixture.lua"
            ))
            .eval()
            .unwrap();
        check_lua_host_receipt_contract(&lua, game == Game::Conan);
        assert_eq!(
            cases,
            if game == Game::Conan { 34 } else { 26 },
            "{name}: every actual native policy case must run"
        );
    }
}

#[test]
fn native_scum_policy_requests_one_save_without_claiming_completion() {
    let lua = mlua::Lua::new();
    lua.globals()
        .set(
            "LGSM_TEST_SOURCE",
            lua.create_string(Game::Scum.payload()).unwrap(),
        )
        .unwrap();
    lua.globals().set("LGSM_TEST_GAME", "scum").unwrap();
    let cases: usize = lua
        .load(include_str!(
            "../../../../../modules/scum/control/tests/policy_fixture.lua"
        ))
        .eval()
        .unwrap();
    check_lua_host_receipt_contract(&lua, false);
    assert_eq!(
        cases, 31,
        "every actual persistent-state policy case must run"
    );
}

#[test]
fn unreal_world_save_receipt_is_closed_and_bound_to_current_request() {
    let request = request();
    let base = packet();
    assert!(parse(&serde_json::to_vec(&base).unwrap(), &request).is_ok());
    for (key, value) in [
        ("request_id", serde_json::json!(TOKEN)),
        ("boot_id", serde_json::json!(TOKEN)),
        ("world_token", serde_json::json!(ID)),
        ("timestamp", serde_json::json!(107)),
        ("extra", serde_json::json!(true)),
        ("launch_token", serde_json::json!(format!("LGSM_UWS_{ID}"))),
    ] {
        let mut body = base.clone();
        body[key] = value;
        assert!(
            parse(&serde_json::to_vec(&body).unwrap(), &request).is_err(),
            "{key}"
        );
    }
}

#[test]
fn unreal_world_save_confirmation_requires_own_completed_success() {
    let mut body = packet();
    body["write_confirmed"] = true.into();
    assert!(
        classify(parse(&serde_json::to_vec(&body).unwrap(), &request()))
            .unwrap()
            .contains("completed successfully")
    );
    for (key, value) in [
        ("ready", false.into()),
        ("invoked", false.into()),
        ("call_returned", false.into()),
        ("error", serde_json::json!("save_failed")),
    ] {
        let mut invalid = body.clone();
        invalid[key] = value;
        assert!(
            parse(&serde_json::to_vec(&invalid).unwrap(), &request()).is_err(),
            "{key}"
        );
    }
    body["write_confirmed"] = false.into();
    body["error"] = "save_failed".into();
    assert!(classify(parse(&serde_json::to_vec(&body).unwrap(), &request())).is_err());
    body["error"] = serde_json::Value::Null;
    body["timestamp"] = 130.into();
    let bytes = serde_json::to_vec(&body).unwrap();
    assert!(parse(&bytes, &request()).is_err());
    assert!(parse_with_window(&bytes, &request(), 36).is_ok());
    body["timestamp"] = 137.into();
    assert!(parse_with_window(&serde_json::to_vec(&body).unwrap(), &request(), 36).is_err());
}

#[test]
fn native_void_actions_report_requested_without_claiming_disk_completion() {
    for game in [Game::Moria, Game::Abiotic, Game::Scum, Game::Nightingale] {
        let identity = ProcessIdentity {
            creation_time: 123,
            image_path: format!("D:/fixture/{}", game.executable()),
        };
        let context = Context {
            game,
            root: PathBuf::new(),
            pid: 5,
            identity: identity.clone(),
            process_key: "main".into(),
            launch_token: LAUNCH.into(),
            native: process::NativeProcess { pid: 42, identity },
        };
        let evaluate = |body: &serde_json::Value| {
            classify_completion(
                game,
                parse_context(&context, &serde_json::to_vec(body).unwrap(), &request()),
            )
        };
        let mut body = packet();
        if matches!(game, Game::Scum | Game::Nightingale) {
            body["native_process_id"] = 42.into();
            body["native_creation_time"] = "123".into();
        }
        let (message, completion) = evaluate(&body).unwrap();
        assert!(matches!(completion, WorldSaveCompletion::Requested));
        assert!(message.contains("request was submitted"));
        assert!(!message.contains("completed successfully"));
        for (field, invalid) in [
            ("request_id", serde_json::json!(TOKEN)),
            ("boot_id", serde_json::json!(TOKEN)),
            ("world_token", serde_json::json!(ID)),
            ("launch_token", serde_json::json!(format!("LGSM_UWS_{ID}"))),
            ("timestamp", serde_json::json!(137)),
            ("action", serde_json::json!("saveprobe")),
            ("ready", serde_json::json!(false)),
            ("invoked", serde_json::json!(false)),
            ("call_returned", serde_json::json!(false)),
            ("error", serde_json::json!("save_failed")),
            ("native_process_id", serde_json::json!(43)),
            ("extra", serde_json::json!(true)),
        ] {
            let mut invalid_body = body.clone();
            invalid_body[field] = invalid;
            assert!(evaluate(&invalid_body).is_err(), "{field}");
        }
        let mut rejected = body.clone();
        rejected["ready"] = false.into();
        rejected["invoked"] = false.into();
        rejected["call_returned"] = false.into();
        rejected["error"] = "world_unavailable".into();
        assert!(
            evaluate(&rejected)
                .unwrap_err()
                .contains("before invocation")
        );
        assert!(
            classify_completion(
                game,
                Err(SaveFailure::new(
                    "awaiting_receipt",
                    "timeout",
                    "timeout or missing receipt"
                ))
            )
            .is_err()
        );
        // A void native save call cannot attest a native write result.
        let mut unsupported_confirmation = body;
        unsupported_confirmation["write_confirmed"] = true.into();
        assert!(evaluate(&unsupported_confirmation).is_err());
    }
}

#[test]
fn request_semantics_do_not_relax_other_native_completion_contracts() {
    let game = Game::Conan;
    let body = packet();
    let bytes = serde_json::to_vec(&body).unwrap();
    assert!(
        classify_completion(game, parse(&bytes, &request())).is_err(),
        "{game:?}"
    );
    let mut confirmed = body;
    confirmed["write_confirmed"] = true.into();
    let (_, completion) = classify_completion(
        game,
        parse(&serde_json::to_vec(&confirmed).unwrap(), &request()),
    )
    .unwrap();
    assert!(
        matches!(completion, WorldSaveCompletion::Confirmed),
        "{game:?}"
    );
}

#[test]
fn conan_confirmation_binds_native_process_and_the_existing_completion_window() {
    let identity = ProcessIdentity {
        creation_time: 123,
        image_path: "D:/fixture/ConanSandboxServer.exe".into(),
    };
    let context = Context {
        game: Game::Conan,
        root: PathBuf::new(),
        pid: 5,
        identity: identity.clone(),
        process_key: "main".into(),
        launch_token: LAUNCH.into(),
        native: process::NativeProcess { pid: 42, identity },
    };
    let mut body = packet();
    body["timestamp"] = 136.into();
    body["write_confirmed"] = true.into();
    body["native_process_id"] = 42.into();
    body["native_creation_time"] = "123".into();
    let parse = |body: &serde_json::Value| {
        parse_context(&context, &serde_json::to_vec(body).unwrap(), &request())
    };
    assert!(parse(&body).unwrap().write_confirmed);
    for (key, bad) in [
        ("timestamp", serde_json::json!(137)),
        ("native_process_id", serde_json::json!(43)),
        ("native_creation_time", serde_json::json!("124")),
        ("native_creation_time", serde_json::Value::Null),
    ] {
        let mut changed = body.clone();
        changed[key] = bad;
        assert!(parse(&changed).is_err(), "{key}");
    }
}

#[test]
fn native_world_save_receipt_accepts_real_lua_fields_and_binds_the_shipping_generation() {
    let mut packet = packet();
    packet["native_process_id"] = 42.into();
    packet["native_creation_time"] = "123".into();
    let native = process::NativeProcess {
        pid: 42,
        identity: ProcessIdentity {
            image_path: "D:/game/server.exe".into(),
            creation_time: 123,
        },
    };
    let parse_packet =
        |value: &serde_json::Value| parse(&serde_json::to_vec(value).unwrap(), &request()).unwrap();
    let reply = parse_packet(&packet);
    validate_native_identity(&reply, Some(&native)).unwrap();
    assert!(validate_native_identity(&reply, None).is_err());
    for (key, bad) in [
        ("native_process_id", serde_json::json!(43)),
        ("native_creation_time", serde_json::json!("124")),
        ("native_creation_time", serde_json::json!("0123")),
        ("native_process_id", serde_json::Value::Null),
        ("native_creation_time", serde_json::Value::Null),
    ] {
        let mut changed = packet.clone();
        changed[key] = bad;
        assert!(
            validate_native_identity(&parse_packet(&changed), Some(&native)).is_err(),
            "{key}"
        );
    }
    let old = parse_packet(&super::tests::packet());
    assert!(validate_native_identity(&old, Some(&native)).is_err());
    validate_native_identity(&old, None).unwrap();
    packet["ready"] = false.into();
    packet["invoked"] = false.into();
    packet["call_returned"] = false.into();
    packet.as_object_mut().unwrap().remove("native_process_id");
    packet
        .as_object_mut()
        .unwrap()
        .remove("native_creation_time");
    validate_native_identity(&parse_packet(&packet), Some(&native)).unwrap();
}

#[test]
fn unreal_world_save_known_rejection_is_distinct_from_unknown_delivery() {
    let request = request();
    let body = packet();
    assert!(
        classify(parse(&serde_json::to_vec(&body).unwrap(), &request))
            .unwrap_err()
            .contains("Save verification failed")
    );
    assert!(
        classify(Err(SaveFailure::new(
            "awaiting_receipt",
            "timeout",
            "timeout"
        )))
        .unwrap_err()
        .contains("was not resent")
    );
    for code in [
        "save_busy",
        "target_changed",
        "dispatcher_timeout",
        "api_unavailable",
    ] {
        let mut rejected = body.clone();
        rejected["ready"] = false.into();
        rejected["invoked"] = false.into();
        rejected["call_returned"] = false.into();
        rejected["error"] = code.into();
        assert!(classify(parse(&serde_json::to_vec(&rejected).unwrap(), &request)).is_err());
        rejected["invoked"] = true.into();
        assert!(
            classify(parse(&serde_json::to_vec(&rejected).unwrap(), &request))
                .unwrap_err()
                .contains("Save verification failed")
        );
    }
}

#[test]
fn native_save_failure_diagnostics_preserve_stage_without_changing_unknown_results() {
    let mut mismatched = packet();
    mismatched["request_id"] = TOKEN.into();
    let mut native_failure = packet();
    native_failure["call_returned"] = false.into();
    native_failure["error"] = "save_call_unknown".into();
    for (result, expected) in [
        (
            Err(SaveFailure::new(
                "awaiting_receipt",
                "timeout",
                "request timed out",
            )),
            "phase=awaiting_receipt cause=timeout",
        ),
        (
            parse(b"{", &request()),
            "phase=receipt_validation cause=invalid_frame",
        ),
        (
            parse(&serde_json::to_vec(&mismatched).unwrap(), &request()),
            "phase=receipt_validation cause=request_mismatch",
        ),
        (
            parse(&serde_json::to_vec(&native_failure).unwrap(), &request()),
            "native_error=save_call_unknown",
        ),
    ] {
        let line = receipt_diagnostic(Game::Moria, "save", Duration::from_millis(5010), &result);
        assert!(line.contains(expected), "{line}");
        assert!(line.contains("module=returntomoria action=save"), "{line}");
        assert!(line.contains("elapsed_ms=5010"), "{line}");
        assert!(
            classify_completion(Game::Moria, result)
                .unwrap_err()
                .contains("Save verification failed")
        );
    }
    let result = parse(&serde_json::to_vec(&packet()).unwrap(), &request());
    let line = receipt_diagnostic(Game::Moria, "save", Duration::from_millis(250), &result);
    assert!(line.contains("call_returned=true write_confirmed=false native_error=none"));
    assert!(matches!(
        classify_completion(Game::Moria, result).unwrap().1,
        WorldSaveCompletion::Requested
    ));
}

#[test]
fn native_save_diagnostics_never_publish_receipt_identities_or_untrusted_errors() {
    let private = format!("{ID} {TOKEN} {LAUNCH} D:/private/world\nforged_log=true");
    let mut body = packet();
    body["error"] = private.clone().into();
    let result = parse(&serde_json::to_vec(&body).unwrap(), &request());
    let received = receipt_diagnostic(Game::Moria, "save", Duration::ZERO, &result);
    assert!(received.contains("native_error=unrecognized"));
    let failed = receipt_diagnostic(
        Game::Moria,
        &private,
        Duration::ZERO,
        &Err(SaveFailure::new(
            "awaiting_receipt",
            "ownership_unverified",
            &private,
        )),
    );
    assert!(failed.contains("action=invalid"));
    assert!(failed.contains("ready=unknown invoked=unknown"));
    for line in [received, failed] {
        for secret in [ID, TOKEN, LAUNCH, "D:/private", "forged_log", "\n"] {
            assert!(!line.contains(secret), "private diagnostic content escaped");
        }
    }
}

#[test]
fn unreal_world_save_profiles_are_exact_and_payloads_have_no_moderation_dispatch() {
    assert_eq!(
        Game::Conan.profile(),
        include_str!("../../../../../modules/conanexiles/control/UE4SS-settings.ini")
    );
    let a = Game::Abiotic.profile();
    assert_eq!(
        a,
        include_str!("../../../../../modules/returntomoria/control/UE4SS-settings.ini")
    );
    let expected = installation::profile_entries(a).unwrap();
    for extra in [
        "\n[Overrides]\nModsFolderPath=../Other",
        "\n[Hooks]\nHookEngineTick=0",
    ] {
        assert_ne!(
            installation::profile_entries(&format!("{a}{extra}")),
            Some(expected.clone())
        );
    }
    for game in [Game::Abiotic, Game::Moria] {
        let payload = String::from_utf8(game.payload()).unwrap();
        assert!(
            payload.contains("ExecuteInGameThread")
                && payload.contains("EGameThreadMethod.EngineTick")
        );
        assert!(!payload.contains("KickPlayer"));
        assert!(!payload.contains("loadlib"));
    }
}

#[test]
fn unreal_world_save_startup_refresh_never_installs_first_payload_or_loader() {
    let instance = std::env::temp_dir().join(format!(
        "langame-unreal-save-start-{}",
        uuid::Uuid::new_v4()
    ));
    let runtime = instance.join("runtime");
    fs::create_dir_all(&runtime).unwrap();
    fs::write(runtime.join(".langame-private-runtime"), b"managed\n").unwrap();
    let details = InstanceDetails {
        summary: app_core::InstanceSummary {
            id: "fixture".into(),
            name: "fixture".into(),
            module_id: "returntomoria".into(),
            status: InstanceStatus::Stopped,
            active_process_count: 0,
            bind_ip: "127.0.0.1".into(),
            port_count: 0,
            autostart: false,
        },
        config_file_path: instance
            .join("config/server.ini")
            .to_string_lossy()
            .into_owned(),
        saves_path: String::new(),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 0,
        settings_json: "{}".into(),
        ports: vec![],
        active_run: None,
    };
    assert!(!refresh(&details).unwrap());
    assert!(!runtime.join("Moria").exists());
    assert!(!runtime.join("langame_world_save").exists());
    assert_eq!(fs::read_dir(&runtime).unwrap().count(), 1);
    let mut running = details.clone();
    running.summary.status = InstanceStatus::Running;
    assert!(stopped(&running).is_err());
    assert_eq!(instance.parent(), Some(std::env::temp_dir().as_path()));
    fs::remove_dir_all(instance).unwrap();
}

#[test]
fn unreal_world_save_launch_session_is_namespaced_and_bound_to_the_game() {
    let root = std::env::temp_dir().join(format!(
        "langame-unreal-save-session-{}",
        uuid::Uuid::new_v4()
    ));
    let directory = root.join("langame_world_save");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("session.txt");
    fs::write(
        &path,
        format!("LGSM_UNREAL_SAVE_1\nmodule=returntomoria\nrun={LAUNCH}\n"),
    )
    .unwrap();
    assert_eq!(session_token(Game::Moria, &root).unwrap(), LAUNCH);
    assert!(session_token(Game::Abiotic, &root).is_err());
    for invalid in [
        ID,
        "LGSM_UWS_bad",
        "LGSM_UWS_abcdef1234567890abcdef1234567890\nextra",
    ] {
        fs::write(
            &path,
            format!("LGSM_UNREAL_SAVE_1\nmodule=returntomoria\nrun={invalid}\n"),
        )
        .unwrap();
        assert!(session_token(Game::Moria, &root).is_err());
    }
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    fs::remove_dir_all(root).unwrap();
}

#[derive(Clone)]
struct Lease(Arc<()>);
impl StorageContextTaskLease for Lease {}

#[tokio::test]
async fn unreal_world_save_cancelled_caller_keeps_mutation_and_storage_lease_in_actual_worker() {
    let lease = Lease(Arc::new(()));
    let worker_lease = lease.clone();
    let mutex = Arc::new(tokio::sync::Mutex::new(()));
    let mutation = mutex.clone().lock_owned().await;
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, waiting) = std::sync::mpsc::sync_channel(1);
    let caller = tokio::spawn(async move {
        owned_worker(&worker_lease, mutation, move || {
            started.send(()).unwrap();
            waiting.recv().unwrap();
        })
        .await
        .unwrap();
    });
    ready.await.unwrap();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    assert!(mutex.try_lock().is_err());
    assert_eq!(Arc::strong_count(&lease.0), 2);
    release.send(()).unwrap();
    let guard = mutex.lock().await;
    drop(guard);
    // Lock drop occurs before the helper's storage lease drop, so wait for the
    // exact remaining owner count rather than timing-based sleeps.
    tokio::time::timeout(Duration::from_secs(2), async {
        while Arc::strong_count(&lease.0) > 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(Arc::strong_count(&lease.0), 1);
}
