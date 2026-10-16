use super::*;
use std::cell::{Cell, RefCell};
use std::fs;
use std::time::Duration;

use serde_json::{Value, json};

const TOKEN: &str = "0123456789abcdef0123456789abcdef";
const PAYLOAD: &[u8] = b"synthetic owned plugin payload";

struct Fixture {
    base: PathBuf,
    context: Context,
}

impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!("valheim-control-{}", uuid::Uuid::new_v4()));
        let root = base.join("instance/runtime");
        fs::create_dir_all(root.join("BepInEx/plugins")).unwrap();
        fs::create_dir_all(root.join(io::DIRECTORY)).unwrap();
        fs::create_dir_all(base.join("instance/config")).unwrap();
        fs::write(root.join(".langame-private-runtime"), b"managed\n").unwrap();
        fs::write(root.join("valheim_server.exe"), b"synthetic native image").unwrap();
        fs::write(root.join(io::PLUGIN), PAYLOAD).unwrap();
        fs::write(
            root.join(io::OWNER),
            serde_json::to_vec(&json!({
                "protocol":1, "module_id":"valheim", "plugin_sha256":io::digest(PAYLOAD)
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            root.join(io::DIRECTORY).join(io::SESSION),
            serde_json::to_vec(&json!({
                "protocol":1, "token":TOKEN
            }))
            .unwrap(),
        )
        .unwrap();
        let context = Context {
            pid: 42,
            identity: ProcessIdentity {
                creation_time: 7,
                image_path: root
                    .join("valheim_server.exe")
                    .to_string_lossy()
                    .into_owned(),
            },
            root,
        };
        Self { base, context }
    }

    fn request(&self) -> Option<Value> {
        io::read_optional(
            &self.context.root.join(io::DIRECTORY).join("request.json"),
            16 * 1024,
        )
        .unwrap()
        .map(|body| serde_json::from_slice(&body).unwrap())
    }

    fn reply(&self, request: &Value) -> Value {
        json!({"protocol":1,"nonce":request["nonce"],"pid":self.context.pid,
            "op":request["op"],"status":"ok","capabilities":["query","reload","kick"],
            "players":[
                {"session_key":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","name":"Same name",
                 "host_id":"76561198000000001","platform_id":"Steam_76561198000000001","admin":true},
                {"session_key":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","name":"Same name",
                 "host_id":"Xbox_MixedCase","platform_id":"Xbox_MixedCase","admin":false}],
            "access":{"admin":["Steam_76561198000000001"],"banned":["Character Name"],"permitted":["Xbox_MixedCase"]}})
    }

    fn publish_reply(&self, value: &Value) {
        io::publish(
            &self.context.root.join(io::DIRECTORY).join("response.json"),
            &serde_json::to_vec(value).unwrap(),
        )
        .unwrap();
    }

    fn details(&self) -> InstanceDetails {
        serde_json::from_value(json!({
            "summary":{"id":"fixture","name":"Fixture","module_id":"valheim","status":"Running",
                "bind_ip":"127.0.0.1","port_count":0,"autostart":false},
            "config_file_path":self.base.join("instance/config/instance.json"),"saves_path":"",
            "auto_backup_on_stop":false,"backup_retention_count":1,"settings_json":"{}","ports":[],
            "active_run":{"run_id":91,"session_id":"session","pid":42,"log_path":null,"process_count":1,
                "processes":[{"run_id":91,"session_id":"session","process_key":"main","display_name":"Server",
                    "pid":42,"process_identity":self.context.identity,"status":"running","started_at":null,
                    "stopped_at":null,"exit_code":null,"crash_flag":false,"log_path":null,"is_primary":true}]}
        })).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.base.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.base).unwrap();
    }
}

fn exchange(fixture: &Fixture, operation: Operation) -> ControlSnapshot {
    transport::exchange_with(
        &fixture.context,
        operation,
        PAYLOAD,
        Duration::from_secs(1),
        || {
            if let Some(request) = fixture.request() {
                assert_eq!(request["token"], TOKEN);
                assert_eq!(request.as_object().unwrap().len(), 4);
                assert!(request.get("pid").is_none());
                fixture.publish_reply(&fixture.reply(&request));
            }
            Ok(())
        },
    )
    .unwrap()
}

#[test]
fn native_identity_projection_preserves_platform_case_and_never_exposes_control_authority() {
    let fixture = Fixture::new();
    let first = project(
        exchange(&fixture, Operation::Query),
        &fixture.context,
        "fixture",
        "one",
        100,
    );
    let second = project(
        exchange(&fixture, Operation::Query),
        &fixture.context,
        "fixture",
        "two",
        200,
    );
    let entries = &first.public_snapshot.entries;
    assert_eq!(entries.len(), 2);
    assert_eq!(first.public_snapshot.current_players, Some(2));
    assert!(first.public_snapshot.complete);
    assert_eq!(entries[0].display_name, entries[1].display_name);
    assert_ne!(entries[0].player_key, entries[1].player_key);
    assert_eq!(
        entries[0].player_key,
        second.public_snapshot.entries[0].player_key
    );
    assert_eq!(
        entries[0].identifiers[0].kind,
        RuntimePlayerIdentityKind::ValheimPlatformId
    );
    assert_eq!(entries[0].identifiers[0].value, "Steam_76561198000000001");
    assert_eq!(entries[1].identifiers[0].value, "Xbox_MixedCase");
    assert!(entries[1].identifiers[0].stable);
    assert_eq!(
        entries[0].identifiers[1].kind,
        RuntimePlayerIdentityKind::PlayerName
    );
    assert!(!entries[0].identifiers[1].stable);
    assert_eq!(entries[0].role.as_deref(), Some("admin"));
    assert_eq!(entries[1].role, None);
    assert!(
        entries
            .iter()
            .all(|entry| entry.available_action_ids.is_empty())
    );
    assert!(first.private_action_bindings.is_empty());
    let public = serde_json::to_string(&first.public_snapshot).unwrap();
    assert!(!public.contains(TOKEN));
    assert!(!public.contains("host_id"));
    assert!(!public.contains("session_key"));
    let mut restarted = fixture.context.clone();
    restarted.identity.creation_time += 1;
    let third = project(
        exchange(&fixture, Operation::Query),
        &restarted,
        "fixture",
        "three",
        300,
    );
    assert_ne!(
        entries[0].player_key,
        third.public_snapshot.entries[0].player_key
    );
}

#[test]
fn query_and_reload_use_distinct_correlated_requests_and_exact_native_lists() {
    let fixture = Fixture::new();
    let queried = exchange(&fixture, Operation::Query);
    let first = fixture.request().unwrap();
    let reloaded = exchange(&fixture, Operation::Reload);
    let second = fixture.request().unwrap();
    assert_eq!(first["op"], "query");
    assert_eq!(second["op"], "reload");
    assert_ne!(first["nonce"], second["nonce"]);
    assert_eq!(queried.access, reloaded.access);
    assert_eq!(reloaded.access.admin, ["Steam_76561198000000001"]);
    assert_eq!(reloaded.access.permitted, ["Xbox_MixedCase"]);
    assert_eq!(reloaded.access.banned, ["Character Name"]);
}

#[test]
fn owner_session_and_plugin_mismatch_fail_before_any_request_is_published() {
    for kind in ["owner", "plugin", "session"] {
        let fixture = Fixture::new();
        match kind {
            "owner" => fs::write(
                fixture.context.root.join(io::OWNER),
                b"{\"protocol\":1,\"module_id\":\"other\",\"plugin_sha256\":\"bad\"}",
            )
            .unwrap(),
            "plugin" => fs::write(fixture.context.root.join(io::PLUGIN), b"unowned").unwrap(),
            _ => fs::write(
                fixture.context.root.join(io::DIRECTORY).join(io::SESSION),
                b"{\"protocol\":1,\"token\":\"bad-secret\"}",
            )
            .unwrap(),
        }
        let error = transport::exchange_with(
            &fixture.context,
            Operation::Reload,
            PAYLOAD,
            Duration::ZERO,
            || Ok(()),
        )
        .unwrap_err();
        assert!(!error.contains("bad-secret"));
        assert!(fixture.request().is_none(), "{kind}");
    }
}

#[test]
fn process_failure_before_or_after_publication_does_not_report_success_or_resubmit() {
    let fixture = Fixture::new();
    assert!(
        transport::exchange_with(
            &fixture.context,
            Operation::Reload,
            PAYLOAD,
            Duration::ZERO,
            || Err("old process".into())
        )
        .is_err()
    );
    assert!(fixture.request().is_none());
    let calls = Cell::new(0);
    let error = transport::exchange_with(
        &fixture.context,
        Operation::Reload,
        PAYLOAD,
        Duration::from_secs(1),
        || {
            calls.set(calls.get() + 1);
            if calls.get() > 2 {
                Err("new process".into())
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    assert!(error.contains("outcome is unknown"));
    let request = fixture.request().unwrap();
    assert_eq!(request["op"], "reload");
    assert_eq!(calls.get(), 3);
}

#[test]
fn timeout_keeps_one_submission_and_releases_the_inflight_lease() {
    let fixture = Fixture::new();
    let published = RefCell::new(None);
    let error = transport::exchange_with(
        &fixture.context,
        Operation::Reload,
        PAYLOAD,
        Duration::from_millis(110),
        || {
            if let Some(request) = fixture.request() {
                let mut previous = published.borrow_mut();
                if let Some(previous) = previous.as_ref() {
                    assert_eq!(previous, &request);
                } else {
                    *previous = Some(request);
                }
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert!(error.contains("outcome is unknown"));
    assert!(published.borrow().is_some());
    // The completed worker dropped its lease; the next query is independent.
    assert_eq!(exchange(&fixture, Operation::Query).players.len(), 2);
}

#[test]
fn competing_request_cannot_replace_the_active_request_file() {
    let fixture = Fixture::new();
    let checked = Cell::new(false);
    let result = transport::exchange_with(
        &fixture.context,
        Operation::Query,
        PAYLOAD,
        Duration::from_secs(1),
        || {
            if !checked.get()
                && let Some(request) = fixture.request()
            {
                checked.set(true);
                let error = transport::exchange_with(
                    &fixture.context,
                    Operation::Reload,
                    PAYLOAD,
                    Duration::ZERO,
                    || Ok(()),
                )
                .unwrap_err();
                assert!(error.contains("already in progress"));
                assert_eq!(fixture.request().unwrap(), request);
                fixture.publish_reply(&fixture.reply(&request));
            }
            Ok(())
        },
    )
    .unwrap();
    assert!(checked.get());
    assert_eq!(result.players.len(), 2);
}

#[test]
fn matching_nonce_with_wrong_pid_is_rejected_and_old_nonce_is_not_accepted() {
    let fixture = Fixture::new();
    fixture.publish_reply(
        &fixture.reply(&json!({"nonce":"11111111111111111111111111111111","op":"query"})),
    );
    let calls = Cell::new(0);
    let error = transport::exchange_with(
        &fixture.context,
        Operation::Query,
        PAYLOAD,
        Duration::from_secs(1),
        || {
            calls.set(calls.get() + 1);
            if calls.get() > 3 {
                let mut reply = fixture.reply(&fixture.request().unwrap());
                reply["pid"] = json!(43);
                fixture.publish_reply(&reply);
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert!(error.contains("process mismatch"));
    assert!(calls.get() >= 4);
}

#[test]
fn closed_owner_and_session_schema_reject_duplicates_unknown_fields_and_invalid_tokens() {
    for body in [
        format!(r#"{{"protocol":1,"protocol":1,"token":"{TOKEN}"}}"#),
        format!(r#"{{"protocol":1,"token":"{TOKEN}","extra":0}}"#),
        json!({"protocol":2,"token":"a".repeat(32)}).to_string(),
        json!({"protocol":1,"token":"0".repeat(32)}).to_string(),
    ] {
        let error = io::parse_session(body.as_bytes())
            .err()
            .expect("reject malformed credential");
        assert!(!error.contains(TOKEN));
    }
    assert!(io::parse_session(&vec![b' '; 4097]).is_err());
    assert!(io::verify_owner(b"{}", PAYLOAD).is_err());
    let owner =
        json!({"protocol":1,"module_id":"valheim","plugin_sha256":io::digest(PAYLOAD),"unknown":0});
    assert!(io::verify_owner(&serde_json::to_vec(&owner).unwrap(), PAYLOAD).is_err());
    assert!(io::verify_owner(b"{}", b"").is_err());
    let pending = json!({"protocol":1,"module_id":"valheim",
        "plugin_sha256":io::digest(PAYLOAD),"pending_plugin_sha256":io::digest(PAYLOAD)});
    assert!(io::verify_owner(&serde_json::to_vec(&pending).unwrap(), PAYLOAD).is_err());
    let missing = json!({"protocol":1,"module_id":"valheim","plugin_sha256":null});
    assert!(io::verify_owner(&serde_json::to_vec(&missing).unwrap(), PAYLOAD).is_err());
}

#[test]
fn ordinary_file_boundaries_reject_directory_oversize_and_relative_roots() {
    let fixture = Fixture::new();
    let root = &fixture.context.root;
    assert!(io::pin_directories(Path::new("relative/root")).is_err());
    assert!(io::read_regular(root, 4096).is_err());
    assert!(
        io::read_optional(&root.join("missing"), 1)
            .unwrap()
            .is_none()
    );
    let path = root.join("bounded");
    fs::write(&path, b"abcd").unwrap();
    assert!(io::read_regular(&path, 3).is_err());
    assert_eq!(io::read_regular(&path, 4).unwrap(), b"abcd");
    let directory = root.join("directory");
    fs::create_dir(&directory).unwrap();
    assert!(io::publish(&directory, b"do not replace").is_err());
    assert!(directory.is_dir());
    io::publish(&path, b"first").unwrap();
    io::publish(&path, b"second").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"second");
    assert!(!fs::read_dir(root).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".pending")
    }));
}

#[test]
fn instance_context_rejects_wrong_module_untracked_process_and_other_runtime() {
    let fixture = Fixture::new();
    let details = fixture.details();
    assert_eq!(context(&details).unwrap().root, fixture.context.root);
    let mut wrong = details.clone();
    wrong.summary.module_id = "enshrouded".into();
    assert!(context(&wrong).is_err());
    let mut stopped = details.clone();
    stopped.active_run = None;
    assert!(context(&stopped).is_err());
    let mut missing = details.clone();
    missing.active_run.as_mut().unwrap().processes[0].process_identity = None;
    assert!(context(&missing).is_err());
    let mut ambiguous = details.clone();
    let process = ambiguous.active_run.as_ref().unwrap().processes[0].clone();
    ambiguous
        .active_run
        .as_mut()
        .unwrap()
        .processes
        .push(process);
    assert!(context(&ambiguous).is_err());
    let other = Fixture::new();
    let mut foreign = details;
    foreign.active_run.as_mut().unwrap().processes[0].process_identity =
        Some(other.context.identity.clone());
    assert!(context(&foreign).is_err());
}

#[cfg(unix)]
#[test]
fn symbolic_link_leaves_and_ancestors_are_not_control_files() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let linked = fixture.context.root.join("linked");
    symlink(fixture.context.root.join(io::PLUGIN), &linked).unwrap();
    assert!(io::read_regular(&linked, 4096).is_err());
    assert!(io::publish(&linked, b"do not replace").is_err());
    fs::remove_file(linked).unwrap();
    let linked_dir = fixture.context.root.join("linked-dir");
    symlink(fixture.context.root.join(io::DIRECTORY), &linked_dir).unwrap();
    assert!(io::pin_directories(&linked_dir).is_err());
    fs::remove_file(linked_dir).unwrap();
}

#[cfg(windows)]
#[test]
fn windows_junction_ancestor_cannot_redirect_control_publication() {
    use std::os::windows::process::CommandExt;
    let fixture = Fixture::new();
    let root = dunce::canonicalize(&fixture.context.root).expect("canonical fixture root");
    let link = root.join("junction");
    let target = dunce::canonicalize(root.join(io::DIRECTORY)).expect("canonical junction target");
    let output = std::process::Command::new("cmd.exe")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .creation_flags(0x0800_0000)
        .output()
        .expect("create isolated test junction");
    assert!(
        output.status.success(),
        "create isolated ordinary-user junction: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(io::pin_directories(&link).is_err());
    assert!(io::publish(&link.join("redirected.json"), b"do not publish").is_err());
    assert!(!target.join("redirected.json").exists());
    fs::remove_dir(&link).unwrap();
}
