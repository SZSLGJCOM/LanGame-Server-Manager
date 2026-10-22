use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
const ID: &str = "1234567890abcdef1234567890abcdef";
const TOKEN: &str = "abcdef1234567890abcdef1234567890";
fn reference() -> Reference<'static> {
    Reference { run: ID }
}
fn request(action: &str) -> Request<'_> {
    Request {
        protocol: 1,
        request_id: ID,
        requested_at: 100,
        action,
        boot_id: if action == "saveprobe" { "" } else { ID },
        world_token: if action == "saveprobe" { "" } else { TOKEN },
    }
}
fn reply(request: &Request<'_>) -> serde_json::Value {
    serde_json::json!({"protocol":1,"request_id":request.request_id,"timestamp":request.requested_at,
        "action":request.action,"boot_id":ID,"world_token":TOKEN,
        "ready":true,"invoked":request.action == "save","call_returned":request.action == "save",
        "write_confirmed":false,"run":ID,"error":null})
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
    assert!(count >= 50);
}
#[test]
fn dragonwilds_control_save_preflight_cannot_acknowledge_an_effect() {
    let request = request("saveprobe");
    let packet = reply(&request);
    assert!(probe(&bytes(&packet), &request, &reference()).is_ok());
    for (key, value) in [
        ("invoked", serde_json::json!(true)),
        ("call_returned", serde_json::json!(true)),
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
fn dragonwilds_control_save_binds_current_run_world_and_nonce_and_rejects_old_completion_frames() {
    let request = request("save");
    let packet = reply(&request);
    assert!(parse(&bytes(&packet), &request, &reference()).is_ok());
    for (key, value) in [
        ("request_id", serde_json::json!(TOKEN)),
        ("run", serde_json::json!(TOKEN)),
        ("boot_id", serde_json::json!(TOKEN)),
        ("world_token", serde_json::json!(ID)),
        ("timestamp", serde_json::json!(99)),
        ("timestamp", serde_json::json!(107)),
        ("write_confirmed", serde_json::json!(true)),
        ("error", serde_json::json!("untrusted text")),
        ("native_status", serde_json::json!("completed")),
        ("save_id", serde_json::json!(ID)),
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
fn dragonwilds_control_save_sends_one_request_without_completion_queries() {
    let request = request("save");
    let mut calls = 0;
    dispatch_once(&request, &reference(), |sent| {
        calls += 1;
        assert_eq!(sent.action, "save");
        Ok(bytes(&reply(sent)))
    })
    .unwrap();
    assert_eq!(calls, 1);
}
#[test]
fn dragonwilds_control_save_never_resends_a_missing_invalid_or_ambiguous_acknowledgment() {
    let request = request("save");
    let mut unknown = reply(&request);
    unknown["call_returned"] = serde_json::json!(false);
    unknown["error"] = serde_json::json!("save_call_unknown");
    for response in [
        Err("transport timeout".into()),
        Ok(b"invalid".to_vec()),
        Ok(bytes(&unknown)),
    ] {
        let mut response = Some(response);
        let mut calls = 0;
        let error = dispatch_once(&request, &reference(), |_| {
            calls += 1;
            response.take().unwrap()
        })
        .unwrap_err();
        assert_eq!(calls, 1);
        assert_eq!(error, UNKNOWN);
    }
}
#[test]
fn dragonwilds_control_save_distinguishes_rejection_from_unknown_invocation() {
    let request = request("save");
    let mut rejected = reply(&request);
    rejected["ready"] = serde_json::json!(false);
    rejected["invoked"] = serde_json::json!(false);
    rejected["call_returned"] = serde_json::json!(false);
    rejected["error"] = serde_json::json!("save_busy");
    let error = dispatch_once(&request, &reference(), |_| Ok(bytes(&rejected))).unwrap_err();
    assert!(error.contains("save_busy") && error.contains("No manual save was called"));
    rejected["invoked"] = serde_json::json!(true);
    assert!(parse(&bytes(&rejected), &request, &reference()).is_err());
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
async fn dragonwilds_control_save_cancellation_retains_ownership_until_worker_exit() {
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
