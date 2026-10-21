//! Owner-thread world saves use their own fresh context, never a player target.
use super::{Context, Target, context, io, response, transport};
use crate::commands::{InstanceRuntimeCommandResult, WorldSaveCompletion};
use app_core::InstanceDetails;
use serde::Deserialize;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    protocol: u32,
    pid: u32,
    creation_time: String,
    run_id: String,
    sequence: String,
    tick_ms: String,
    generation: String,
    ready: bool,
    issue: String,
}

fn parse(context: &Context, bytes: &[u8], now: u64) -> Result<Target, String> {
    let packet: Snapshot =
        serde_json::from_slice(bytes).map_err(|_| "The native save context is invalid.")?;
    let tick = response::decimal(&packet.tick_ms)?;
    let sequence = response::decimal(&packet.sequence)?;
    let generation = response::decimal(&packet.generation)?;
    if packet.protocol == 1
        && packet.pid == context.pid
        && response::decimal(&packet.creation_time)? == context.identity.creation_time
        && response::nonce(&packet.run_id)
        && packet.issue == "unsupported_save_profile"
    {
        return Err(unsupported_build());
    }
    if packet.protocol != 1
        || packet.pid != context.pid
        || response::decimal(&packet.creation_time)? != context.identity.creation_time
        || !response::nonce(&packet.run_id)
        || tick > now
        || now - tick > 2000
        || sequence == 0
        || generation == 0
        || !packet.ready
        || packet.issue != "none"
    {
        return Err(
            "The native world is unavailable, busy, stale, or unsupported for immediate saving."
                .into(),
        );
    }
    Ok(Target {
        run_id: packet.run_id,
        sequence,
        tick,
        handle: 0,
        generation,
        account: 0,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    protocol: u32,
    pid: u32,
    creation_time: String,
    run_id: String,
    request_id: String,
    action: String,
    account_id: String,
    status: String,
    issue: String,
}

fn unknown() -> String {
    "The native save outcome is unknown. It may already have executed; verify the server before retrying.".into()
}

fn unsupported_build() -> String {
    "This Enshrouded game version is not supported by the native save bridge. Update LanGame or install a compatible bridge, then restart the server normally. No save request was sent.".into()
}

fn save_executable(root: &Path) -> Result<String, String> {
    let executable = io::digest(&io::read_regular(
        &root.join("enshrouded_server.exe"),
        32 * 1024 * 1024,
    )?);
    if io::manager_status_for_executable(root, &executable)
        == Some(app_core::RuntimePlayerExtensionStatus::UnsupportedGameBuild)
    {
        return Err(unsupported_build());
    }
    Ok(executable)
}

fn classify_reply(
    context: &Context,
    target: &Target,
    request_id: &str,
    reply: &Reply,
) -> Result<bool, String> {
    if reply.protocol != 1
        || reply.pid != context.pid
        || response::decimal(&reply.creation_time).ok() != Some(context.identity.creation_time)
        || reply.request_id != request_id
        || reply.run_id != target.run_id
        || reply.action != "save_world"
        || reply.account_id != "0"
    {
        return Err(unknown());
    }
    match (reply.status.as_str(), reply.issue.as_str()) {
        ("completed", "none") => Ok(true),
        ("dispatched", "none" | "native_save_outcome_unknown") => Ok(false),
        ("dispatched", "native_save_failed") => Err("The native persistence state machine reported that saving failed; no completion was claimed.".into()),
        ("rejected", issue) => Err(format!("The native server rejected immediate saving ({issue}).")),
        _ => Err(unknown()),
    }
}

fn dispatch(context: &Context) -> Result<bool, String> {
    transport::verify_process(context)?;
    let _root = io::pin_directory(&context.root)?;
    // Reuse this exact digest for installed-proof verification; readiness must
    // not add a second complete executable read to each manual save.
    let executable = save_executable(&context.root)?;
    let _pins = transport::open_with_executable(context, Some(&executable))?;
    let directory = context.root.join(io::DIRECTORY);
    let target = parse(
        context,
        &io::read_regular(&directory.join("world-save.json"), 4096)?,
        transport::tick()?,
    )?;
    let request_id = uuid::Uuid::new_v4().simple().to_string();
    let body = transport::request_body(
        "save_world",
        &request_id,
        &target,
        target.tick.saturating_add(5000),
    );
    transport::verify_process(context)?;
    // Only one publication. A timeout or stopped process cannot authorize retry.
    io::publish_request(&directory.join("request.txt"), body.as_bytes()).map_err(|_| unknown())?;
    let deadline = Instant::now() + Duration::from_secs(125);
    while Instant::now() < deadline {
        transport::verify_process(context).map_err(|_| unknown())?;
        if let Ok(bytes) = io::read_regular(&directory.join("response.json"), 4096)
            && let Ok(reply) = serde_json::from_slice::<Reply>(&bytes)
            && reply.request_id == request_id
        {
            return classify_reply(context, &target, &request_id, &reply);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(false)
}

pub(crate) async fn save_world(
    state: &crate::state::DesktopState,
    details: &InstanceDetails,
    submitted_at_unix_ms: u128,
    mutation: Option<tokio::sync::OwnedMutexGuard<()>>,
) -> Result<InstanceRuntimeCommandResult, String> {
    let mutation = mutation.ok_or("Native saving requires an instance mutation reservation.")?;
    let operation = state.begin_storage_context_operation("Enshrouded immediate save")?;
    let context = context(details)?;
    let details = details.clone();
    crate::state::spawn_blocking_storage_context_task(&operation, move || {
        let _mutation = mutation;
        let completed = dispatch(&context)?;
        Ok(InstanceRuntimeCommandResult {
            instance_id: details.summary.id,
            process_key: "main".into(),
            display_name: "Enshrouded native save".into(),
            pid: context.pid,
            command: "save".into(),
            response_text: Some(
                if completed { "The native persistence state machine completed this world's save successfully." }
                else { "The world-save request was attempted once, but its completion remains unconfirmed. Verify the native server before another request; it was not retried." }.into(),
            ),
            write_confirmation_pending: !completed,
            save_completion: Some(if completed { WorldSaveCompletion::Confirmed } else { WorldSaveCompletion::Unconfirmed }),
            submitted_at_unix_ms,
        })
    })
    .await
    .map_err(|_| unknown())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_unsupported_build_is_reported_before_missing_control_files() {
        let root =
            std::env::temp_dir().join(format!("enshrouded-save-build-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("enshrouded_server.exe"), b"unknown build fixture").unwrap();
        let digest = io::digest(b"unknown build fixture");
        io::publish_manager_state(
            &root,
            &digest,
            app_core::RuntimePlayerExtensionStatus::UnsupportedGameBuild,
        )
        .unwrap();
        assert_eq!(save_executable(&root).unwrap_err(), unsupported_build());
        assert!(!root.join(io::DIRECTORY).exists());
        assert!(!root.join("request.txt").exists());
        std::fs::write(root.join("enshrouded_server.exe"), b"changed build fixture").unwrap();
        assert_eq!(
            save_executable(&root).unwrap(),
            io::digest(b"changed build fixture")
        );
        assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
        std::fs::remove_file(root.join("enshrouded_server.exe")).unwrap();
        std::fs::remove_file(root.join(".langame-enshrouded-control-state.json")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    fn reply_fixture() -> (Context, Target, Reply) {
        let context = Context {
            pid: 42,
            identity: app_core::ProcessIdentity {
                image_path: "D:/server/enshrouded_server.exe".into(),
                creation_time: 123,
            },
            root: "D:/server".into(),
        };
        let target = Target {
            run_id: "11111111111111111111111111111111".into(),
            sequence: 1,
            tick: 1000,
            handle: 0,
            generation: 2,
            account: 0,
        };
        let reply = Reply {
            protocol: 1,
            pid: 42,
            creation_time: "123".into(),
            run_id: target.run_id.clone(),
            request_id: "request".into(),
            action: "save_world".into(),
            account_id: "0".into(),
            status: "completed".into(),
            issue: "none".into(),
        };
        (context, target, reply)
    }

    #[test]
    fn only_the_matching_native_completion_receipt_confirms_a_save() {
        let (context, target, mut reply) = reply_fixture();
        assert_eq!(
            classify_reply(&context, &target, "request", &reply),
            Ok(true)
        );
        for issue in ["none", "native_save_outcome_unknown"] {
            reply.status = "dispatched".into();
            reply.issue = issue.into();
            assert_eq!(
                classify_reply(&context, &target, "request", &reply),
                Ok(false)
            );
        }
        for (status, issue) in [
            ("completed", "native_save_outcome_unknown"),
            ("dispatched", "native_save_failed"),
            ("rejected", "world_not_ready"),
            ("unknown", "none"),
        ] {
            reply.status = status.into();
            reply.issue = issue.into();
            assert!(classify_reply(&context, &target, "request", &reply).is_err());
        }
    }

    #[test]
    fn foreign_or_stale_completion_receipts_never_confirm() {
        for field in 0..8 {
            let (context, target, mut reply) = reply_fixture();
            match field {
                0 => reply.protocol = 2,
                1 => reply.pid += 1,
                2 => reply.creation_time = "124".into(),
                3 => reply.run_id = "22222222222222222222222222222222".into(),
                4 => reply.request_id = "another".into(),
                5 => reply.action = "ban".into(),
                6 => reply.account_id = "1".into(),
                _ => reply.creation_time = "malformed".into(),
            }
            assert!(
                classify_reply(&context, &target, "request", &reply).is_err(),
                "field={field}"
            );
        }
    }

    #[test]
    fn save_context_binds_fresh_process_generation_and_never_uses_a_player() {
        let context = Context {
            pid: 42,
            identity: app_core::ProcessIdentity {
                image_path: "D:/server/enshrouded_server.exe".into(),
                creation_time: 123,
            },
            root: "D:/server".into(),
        };
        let value = serde_json::json!({"protocol":1,"pid":42,"creation_time":"123","run_id":"11111111111111111111111111111111","sequence":"1","tick_ms":"1000","generation":"2","ready":true,"issue":"none"});
        let target = parse(&context, &serde_json::to_vec(&value).unwrap(), 1100).unwrap();
        assert_eq!(
            (target.handle, target.account, target.generation),
            (0, 0, 2)
        );
        for (key, bad) in [
            ("pid", serde_json::json!(43)),
            ("creation_time", serde_json::json!("124")),
            ("generation", serde_json::json!("0")),
            ("ready", serde_json::json!(false)),
            ("issue", serde_json::json!("save_busy")),
            ("tick_ms", serde_json::json!("1101")),
            ("sequence", serde_json::json!("0")),
        ] {
            let mut changed = value.clone();
            changed[key] = bad;
            assert!(
                parse(&context, &serde_json::to_vec(&changed).unwrap(), 1100).is_err(),
                "{key}"
            );
        }
        assert!(parse(&context, &serde_json::to_vec(&value).unwrap(), 3001).is_err());
        let mut unsupported = value;
        unsupported["ready"] = serde_json::json!(false);
        unsupported["generation"] = serde_json::json!("0");
        unsupported["issue"] = serde_json::json!("unsupported_save_profile");
        assert_eq!(
            parse(&context, &serde_json::to_vec(&unsupported).unwrap(), 1100).unwrap_err(),
            unsupported_build()
        );
    }
}
