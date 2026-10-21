use super::*;
use std::sync::Arc;
const ID: &str = "1234567890abcdef1234567890abcdef";
const TOKEN: &str = "abcdef1234567890abcdef1234567890";
fn request() -> Request<'static> {
    Request {
        protocol: 1,
        request_id: ID,
        requested_at: 100,
        action: "save",
        boot_id: ID,
        world_token: TOKEN,
    }
}
fn packet() -> serde_json::Value {
    serde_json::json!({"protocol":1,"request_id":ID,"timestamp":100,"action":"save","boot_id":ID,
        "world_token":TOKEN,"ready":true,"invoked":true,"call_returned":true,"write_confirmed":true,
        "error":null,"native_process_id":20,"native_creation_time":"123"})
}
#[test]
fn windrose_lua_actual_bundled_policy_exercises_native_checkpoint_contract() {
    let lua = mlua::Lua::new();
    lua.globals()
        .set("LGSM_TEST_SOURCE", lua.create_string(payload()).unwrap())
        .unwrap();
    let count: usize = lua
        .load(include_str!("windrose_save_fixture.lua"))
        .eval()
        .unwrap();
    assert_eq!(count, 18);
}
#[test]
fn windrose_receipt_is_closed_and_bound_to_request_world_and_generation() {
    let request = request();
    let base = packet();
    assert!(parse(&serde_json::to_vec(&base).unwrap(), &request).is_ok());
    for (key, value) in [
        ("request_id", serde_json::json!(TOKEN)),
        ("boot_id", serde_json::json!(TOKEN)),
        ("world_token", serde_json::json!(ID)),
        ("timestamp", serde_json::json!(107)),
        ("invoked", serde_json::json!(false)),
        ("call_returned", serde_json::json!(false)),
        ("error", serde_json::json!("save_call_unknown")),
        ("extra", serde_json::json!(true)),
    ] {
        let mut invalid = base.clone();
        invalid[key] = value;
        assert!(
            parse(&serde_json::to_vec(&invalid).unwrap(), &request).is_err(),
            "{key}"
        );
    }
    let context = Context {
        root: PathBuf::new(),
        pid: 5,
        identity: app_core::ProcessIdentity {
            creation_time: 100,
            image_path: "root".into(),
        },
        process_key: "main".into(),
        native: process::NativeProcess {
            pid: 20,
            identity: app_core::ProcessIdentity {
                creation_time: 123,
                image_path: "shipping".into(),
            },
        },
    };
    let valid = parse(&serde_json::to_vec(&base).unwrap(), &request).unwrap();
    assert!(receipt_owner(&context, &valid).is_ok());
    for (key, value) in [
        ("native_process_id", serde_json::json!(5)),
        ("native_creation_time", serde_json::json!("124")),
        ("native_process_id", serde_json::Value::Null),
        ("native_creation_time", serde_json::Value::Null),
    ] {
        let mut invalid = base.clone();
        invalid[key] = value;
        assert!(
            receipt_owner(
                &context,
                &parse(&serde_json::to_vec(&invalid).unwrap(), &request).unwrap()
            )
            .is_err()
        );
    }
}
#[test]
fn windrose_only_true_native_receipt_confirms_checkpoint() {
    let request = request();
    let base = packet();
    assert!(
        classify(parse(&serde_json::to_vec(&base).unwrap(), &request))
            .unwrap()
            .0
    );
    let mut pending = base.clone();
    pending["write_confirmed"] = false.into();
    assert!(
        !classify(parse(&serde_json::to_vec(&pending).unwrap(), &request))
            .unwrap()
            .0
    );
    assert!(!classify(Err("timeout".into())).unwrap().0);
    for code in [
        "api_unavailable",
        "target_changed",
        "request_expired",
        "dispatcher_timeout",
    ] {
        let mut body = pending.clone();
        body["ready"] = false.into();
        body["invoked"] = false.into();
        body["call_returned"] = false.into();
        body["error"] = code.into();
        assert!(classify(parse(&serde_json::to_vec(&body).unwrap(), &request)).is_err());
        body["invoked"] = true.into();
        assert!(
            !classify(parse(&serde_json::to_vec(&body).unwrap(), &request))
                .unwrap()
                .0
        );
    }
}
#[derive(Clone)]
struct Lease(Arc<()>);
impl StorageContextTaskLease for Lease {}
#[tokio::test]
async fn windrose_cancelled_caller_keeps_mutation_and_storage_lease_in_actual_worker() {
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
    drop(mutex.lock().await);
    while Arc::strong_count(&lease.0) > 1 {
        tokio::task::yield_now().await;
    }
    assert_eq!(Arc::strong_count(&lease.0), 1);
}
