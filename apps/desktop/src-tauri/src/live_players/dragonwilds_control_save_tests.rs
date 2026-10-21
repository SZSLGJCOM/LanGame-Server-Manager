use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
const ID: &str = "1234567890abcdef1234567890abcdef";
const TOKEN: &str = "abcdef1234567890abcdef1234567890";
const FILETIME: u64 = 134054131932282181;
fn reference() -> Reference<'static> {
    Reference {
        run: ID,
        pid: 123,
        creation_time: FILETIME,
    }
}
#[test]
fn observed_native_preflight_binds_the_shipping_process_without_lossy_filetime_conversion() {
    // Actual isolated ready frame. It was rejected before any Save because the
    // old caller compared the responder with its managed bootstrap PID.
    let request = Request {
        protocol: 1,
        request_id: "8b8a95474632410aa32070034392048a",
        requested_at: 1792497739,
        action: "saveprobe",
        boot_id: "",
        world_token: "",
        save_id: None,
    };
    let frame=br#"{"protocol":1,"request_id":"8b8a95474632410aa32070034392048a","timestamp":1792497739,"action":"saveprobe","boot_id":"2c3a61642ec939b6600e81366fc72743","save_id":"","world_token":"713404f315e21df40eb442e94c26cb97","ready":true,"invoked":false,"call_returned":false,"write_confirmed":false,"native_status":"ready","native_pid":"55852","native_creation_time":"134369713226028349","run":"5e9c2690d40344549868cfc5e9fe11aa","error":null}"#;
    let run = "5e9c2690d40344549868cfc5e9fe11aa";
    let reference = Reference {
        run,
        pid: 55852,
        creation_time: 134369713226028349,
    };
    assert!(probe(frame, &request, &reference).is_ok());
    for (pid, creation_time) in [
        (55851, reference.creation_time),
        (55852, reference.creation_time - 1),
        (55852, 134369713226028352),
    ] {
        assert!(
            probe(
                frame,
                &request,
                &Reference {
                    run,
                    pid,
                    creation_time
                }
            )
            .is_err()
        );
    }
}
fn request(action: &str) -> Request<'_> {
    Request {
        protocol: 1,
        request_id: ID,
        requested_at: 100,
        action,
        boot_id: if action == "saveprobe" { "" } else { ID },
        world_token: if action == "saveprobe" { "" } else { TOKEN },
        save_id: if action == "savequery" {
            Some(TOKEN)
        } else {
            None
        },
    }
}
fn reply(request: &Request<'_>, status: &str) -> serde_json::Value {
    serde_json::json!({"protocol":1,"request_id":request.request_id,"timestamp":request.requested_at,
        "action":request.action,"boot_id":ID,"world_token":TOKEN,"save_id":request.save_id.unwrap_or(""),
        "ready":status != "rejected", "invoked": status != "ready" && status != "rejected",
        "call_returned":status == "completed", "write_confirmed":status == "completed",
        "native_status":status,"native_pid":"123","native_creation_time":FILETIME.to_string(),
        "run":ID,"error":if status == "failed" {Some("native_write_failed")} else {None}})
}
fn bytes(value: &serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}
#[test]
fn dragonwilds_control_save_lua_fixture_uses_the_bundled_payload_and_preserves_moderation() {
    let lua = mlua::Lua::new();
    let args = lua.create_table().unwrap();
    args.set(
        1,
        "D:/fixture/RSDragonwilds/Binaries/Win64/ue4ss/Mods/LgsmPlayerControl/Scripts/main.lua",
    )
    .unwrap();
    lua.globals().set("arg", args).unwrap();
    let (payload, _) = super::super::bundled_payload();
    lua.globals()
        .set(
            "LGSM_CONTROL_TEST_SOURCE",
            lua.create_string(payload).unwrap(),
        )
        .unwrap();
    let count: usize = lua
        .load(include_str!(
            "../../../../../modules/runescapedragonwilds/control/LgsmPlayerControl/tests.lua"
        ))
        .eval()
        .unwrap();
    assert!(count >= 51);
}
#[test]
fn dragonwilds_control_save_probe_is_native_current_closed_and_side_effect_free() {
    let request = request("saveprobe");
    let packet = reply(&request, "ready");
    assert!(probe(&bytes(&packet), &request, &reference()).is_ok());
    for (key, value) in [
        ("native_status", serde_json::json!("completed")),
        ("invoked", serde_json::json!(true)),
        ("world_token", serde_json::json!("")),
        ("timestamp", serde_json::json!(107)),
        ("write_confirmed", serde_json::json!(true)),
    ] {
        let mut invalid = packet.clone();
        invalid[key] = value;
        assert!(
            probe(&bytes(&invalid), &request, &reference()).is_err(),
            "{key}"
        );
    }
}
#[test]
fn dragonwilds_control_save_rejects_foreign_identity_lossy_filetime_and_schema_pollution() {
    let request = request("savequery");
    let packet = reply(&request, "completed");
    assert!(parse(&bytes(&packet), &request, &reference()).is_ok());
    for (key, value) in [
        ("request_id", serde_json::json!(TOKEN)),
        ("save_id", serde_json::json!(ID)),
        ("run", serde_json::json!(TOKEN)),
        ("boot_id", serde_json::json!(TOKEN)),
        ("world_token", serde_json::json!(ID)),
        ("native_pid", serde_json::json!("0123")),
        ("native_pid", serde_json::json!("124")),
        ("native_creation_time", serde_json::json!(FILETIME)),
        (
            "native_creation_time",
            serde_json::json!("134054131932282176"),
        ),
        (
            "native_creation_time",
            serde_json::json!("18446744073709551616"),
        ),
        (
            "native_creation_time",
            serde_json::json!("+134054131932282181"),
        ),
        ("error", serde_json::json!("untrusted text")),
        ("write_confirmed", serde_json::json!(false)),
        ("native_status", serde_json::json!("sent")),
        ("extra", serde_json::json!(true)),
    ] {
        let mut invalid = packet.clone();
        invalid[key] = value;
        assert!(
            parse(&bytes(&invalid), &request, &reference()).is_err(),
            "{key}"
        );
    }
    let text = String::from_utf8(bytes(&packet)).unwrap();
    for invalid in [
        format!("{text}{text}"),
        text.replacen("{", "{\"run\":\"bad\",", 1),
        " ".repeat(2049),
    ] {
        assert!(parse(invalid.as_bytes(), &request, &reference()).is_err());
    }
}
#[test]
fn dragonwilds_control_save_sends_once_and_queries_until_all_native_sinks_complete() {
    let request = request("save");
    let mut saves = 0;
    let mut queries = 0;
    complete_once(
        &request,
        &reference(),
        Instant::now() + LIFETIME,
        |sent| {
            if sent.action == "save" {
                saves += 1;
                assert!(sent.save_id.is_none());
            } else {
                queries += 1;
                assert_eq!(sent.action, "savequery");
                assert_eq!(sent.save_id, Some(ID));
                assert_ne!(sent.request_id, ID);
            }
            Ok(bytes(&reply(
                sent,
                if queries == 2 {
                    "completed"
                } else {
                    "dispatching"
                },
            )))
        },
        || {},
    )
    .unwrap();
    assert_eq!((saves, queries), (1, 2));
}
#[test]
fn dragonwilds_control_save_recovers_lost_initial_receipt_only_with_a_read_only_query() {
    for first in [Err("transport timeout".into()), Ok(b"invalid".to_vec())] {
        let request = request("save");
        let mut initial = Some(first);
        let mut calls = 0;
        complete_once(
            &request,
            &reference(),
            Instant::now() + LIFETIME,
            |sent| {
                calls += 1;
                if sent.action == "save" {
                    return initial.take().unwrap();
                }
                assert_eq!(sent.save_id, Some(ID));
                Ok(bytes(&reply(sent, "completed")))
            },
            || {},
        )
        .unwrap();
        assert_eq!(calls, 2);
    }
}
#[test]
fn dragonwilds_control_save_never_promotes_failure_unknown_or_late_success() {
    for status in ["failed", "unknown", "rejected"] {
        let request = request("save");
        let mut calls = 0;
        let error = complete_once(
            &request,
            &reference(),
            Instant::now() + LIFETIME,
            |sent| {
                calls += 1;
                let mut value = reply(sent, status);
                if status != "failed" {
                    value["error"] = serde_json::json!(if status == "rejected" {
                        "save_busy"
                    } else {
                        "native_closure_unavailable"
                    });
                }
                Ok(bytes(&value))
            },
            || {},
        )
        .unwrap_err();
        assert_eq!(calls, 1);
        assert!(error.contains("not resent"));
    }
    let mut calls = 0;
    assert!(
        complete_once(
            &request("save"),
            &reference(),
            Instant::now(),
            |sent| {
                calls += 1;
                Ok(bytes(&reply(sent, "completed")))
            },
            || {}
        )
        .is_err()
    );
    assert_eq!(calls, 1);
}
#[test]
fn dragonwilds_control_save_query_wire_binds_original_save_without_adding_a_second_save() {
    let value = serde_json::to_value(request("savequery")).unwrap();
    assert_eq!(value["action"], "savequery");
    assert_eq!(value["save_id"], TOKEN);
    assert!(
        serde_json::to_value(request("save"))
            .unwrap()
            .get("save_id")
            .is_none()
    );
    assert_eq!(LIFETIME, Duration::from_secs(30));
}
struct Lease(
    Arc<AtomicUsize>,
    Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
);
impl Clone for Lease {
    fn clone(&self) -> Self {
        self.0.fetch_add(1, Ordering::SeqCst);
        Self(self.0.clone(), self.1.clone())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if self.0.fetch_sub(1, Ordering::SeqCst) == 1
            && let Some(finished) = self.1.lock().unwrap().take()
        {
            let _ = finished.send(());
        }
    }
}
impl StorageContextTaskLease for Lease {}

#[tokio::test]
async fn dragonwilds_control_save_cancellation_retains_storage_lease_and_mutation_until_worker_exit()
 {
    let mutex = Arc::new(tokio::sync::Mutex::new(()));
    let mutation = mutex.clone().lock_owned().await;
    let retained = Arc::new(AtomicUsize::new(1));
    let (finished, done) = tokio::sync::oneshot::channel();
    let lease = Lease(
        retained.clone(),
        Arc::new(std::sync::Mutex::new(Some(finished))),
    );
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, waiting) = std::sync::mpsc::sync_channel(1);
    let outer = tokio::spawn(async move {
        let worker = spawn_save_worker(&lease, mutation, move || {
            started.send(()).unwrap();
            waiting.recv().unwrap();
        });
        drop(lease);
        worker.await.unwrap();
    });
    ready.await.unwrap();
    outer.abort();
    assert!(outer.await.unwrap_err().is_cancelled());
    assert!(mutex.try_lock().is_err());
    assert_eq!(retained.load(Ordering::SeqCst), 1);
    release.send(()).unwrap();
    done.await.unwrap();
    assert!(mutex.try_lock().is_ok());
    assert_eq!(retained.load(Ordering::SeqCst), 0);
}
