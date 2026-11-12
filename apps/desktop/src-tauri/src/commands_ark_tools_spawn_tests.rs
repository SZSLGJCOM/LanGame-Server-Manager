use super::*;
use std::collections::VecDeque;

fn envelope(action: &str, ok: bool, extra: serde_json::Value) -> String {
    let mut value = json!({"version":1,"edition":"asa","action":action,
        "requestId":spawn_input().request_id,"ok":ok});
    value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    format!("LGSM_ARK_TOOLS {value}")
}

fn entity(action: &str) -> String {
    envelope(action, true, serde_json::to_value(observed()).unwrap())
}

fn read_entries(path: &Path) -> Vec<serde_json::Value> {
    crate::desktop_app_log::recent_lines(path, None, 100, 256 * 1024)
        .unwrap()
        .into_iter()
        .map(|line| serde_json::from_str(&line).unwrap())
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn ark_spawn_failure_stages_are_persisted_without_retries_or_raw_replies() {
    let cases = [
        (
            "handshake_dispatch",
            "not_sent",
            None,
            vec![Err("RCON password=fixture-password".into())],
        ),
        (
            "handshake_reply",
            "not_sent",
            Some("world_not_ready"),
            vec![Ok(envelope(
                "status",
                false,
                json!({"error":"world_not_ready"}),
            ))],
        ),
        (
            "spawn_dispatch",
            "outcome_unknown",
            None,
            vec![
                Ok(envelope("status", true, json!({}))),
                Err("password=fixture-password".into()),
            ],
        ),
        (
            "spawn_reply",
            "outcome_unknown",
            None,
            vec![
                Ok(envelope("status", true, json!({}))),
                Ok("Server received, But no response!! password=fixture-password".into()),
            ],
        ),
        (
            "spawn_reply",
            "rejected",
            Some("invalid_dino_class"),
            vec![
                Ok(envelope("status", true, json!({}))),
                Ok(envelope(
                    "spawn",
                    false,
                    json!({"error":"invalid_dino_class"}),
                )),
            ],
        ),
        (
            "spawn_reply",
            "rejected",
            Some("owner_not_online"),
            vec![
                Ok(envelope("status", true, json!({}))),
                Ok(envelope(
                    "spawn",
                    false,
                    json!({"error":"owner_not_online","diagnostics":{"stage":"owner","ownerFailure":"character_unavailable"}}),
                )),
            ],
        ),
        (
            "spawn_reply",
            "native_failed",
            Some("tame_failed"),
            vec![
                Ok(envelope("status", true, json!({}))),
                Ok(envelope(
                    "spawn",
                    false,
                    json!({"error":"tame_failed","diagnostics":{"stage":"tame_verify","id1":123,"id2":456,"cleanup":"destroy_requested","actualTamed":false,"ownerTeam":123456,"actualTeam":7,"password":"fixture-password"}}),
                )),
            ],
        ),
        (
            "spawn_reply",
            "outcome_unknown",
            Some("request_id_conflict"),
            vec![
                Ok(envelope("status", true, json!({}))),
                Ok(envelope(
                    "spawn",
                    false,
                    json!({"error":"request_id_conflict"}),
                )),
            ],
        ),
        (
            "spawn_reply",
            "outcome_unknown",
            Some("unknown_native_error"),
            vec![
                Ok(envelope("status", true, json!({}))),
                Ok(envelope(
                    "spawn",
                    false,
                    json!({"error":"password=fixture-password"}),
                )),
            ],
        ),
        (
            "inspect_dispatch",
            "spawned_unverified",
            None,
            vec![
                Ok(envelope("status", true, json!({}))),
                Ok(entity("spawn")),
                Err("password=fixture-password".into()),
            ],
        ),
        (
            "inspect_reply",
            "spawned_unverified",
            Some("entity_not_found"),
            vec![
                Ok(envelope("status", true, json!({}))),
                Ok(entity("spawn")),
                Ok(envelope(
                    "inspect",
                    false,
                    json!({"error":"entity_not_found"}),
                )),
            ],
        ),
        (
            "verify",
            "spawned_unverified",
            None,
            vec![
                Ok(envelope("status", true, json!({}))),
                Ok(entity("spawn")),
                Ok(entity("inspect").replace("\"level\":150", "\"level\":151")),
            ],
        ),
    ];
    for (phase, outcome, native_code, replies) in cases {
        let root = temp_test_dir("ark-spawn-audit");
        let path = root.join("app.log");
        let input = spawn_input();
        let expected_calls = replies.len();
        let mut replies = VecDeque::from(replies);
        let mut commands = Vec::new();
        let mut audit = ark::SpawnAudit::new(Some(path.clone()), &input);
        let result = ark::execute_spawn(
            &input,
            "arksurvivalascended",
            protocol::spawn_command(&input).unwrap(),
            &mut audit,
            |command| {
                commands.push(command);
                std::future::ready(replies.pop_front().expect("mutation must not be retried"))
            },
        )
        .await;
        let message = audit.fail(result.expect_err("fixture must fail at its selected boundary"));
        drop(audit);
        assert_eq!(commands.len(), expected_calls, "{phase}");
        assert!(
            commands
                .iter()
                .filter(|command| command.starts_with("LgsmArkTools.Spawn "))
                .count()
                <= 1
        );
        let entries = read_entries(&path);
        let failures = entries
            .iter()
            .filter(|entry| entry["action"] == "instance.ark_creature.failed")
            .collect::<Vec<_>>();
        assert_eq!(
            failures.len(),
            1,
            "each failed attempt needs one terminal receipt"
        );
        let context = &failures[0]["context"];
        assert_eq!(context["phase"], phase);
        assert_eq!(context["outcome"], outcome);
        assert_eq!(context["native_code"], json!(native_code));
        for (key, expected) in [
            ("instance_id", json!(input.instance_id)),
            ("process_key", json!(input.process_key)),
            ("run_id", json!(input.run_id)),
            ("request_id", json!(input.request_id)),
            ("creature", json!(input.creature)),
            ("level", json!(input.level)),
            ("tamed", json!(input.tamed)),
            ("player_id", json!(input.player_id)),
        ] {
            assert_eq!(context[key], expected, "missing correlation field {key}");
        }
        assert!(
            !serde_json::to_string(&entries)
                .unwrap()
                .contains("fixture-password")
        );
        if native_code == Some("tame_failed") {
            assert_eq!(context["native_diagnostics"]["stage"], "tame_verify");
            assert_eq!(context["native_diagnostics"]["id1"], 123);
            assert_eq!(
                context["native_diagnostics"]["cleanup"],
                "destroy_requested"
            );
            assert!(message.contains("尚未确认实体已移除") && message.contains("勿重复生成"));
            assert!(!message.contains("本次请求未执行"));
        }
        if outcome == "spawned_unverified" {
            assert_eq!(context["entity_id"], json!([123, 456]));
            assert!(message.contains("123:456") && message.contains("勿重复生成"));
        }
        if outcome == "outcome_unknown" {
            assert!(message.contains("勿重复生成"));
        }
        if native_code == Some("owner_not_online") {
            assert!(message.contains("角色尚未就绪") && message.contains("本次请求未执行"));
        }
        assert!(message.contains(&input.request_id));
        fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn ark_spawn_success_is_logged_only_after_independent_readback() {
    let root = temp_test_dir("ark-spawn-verified");
    let path = root.join("app.log");
    let input = spawn_input();
    let mut replies = VecDeque::from([
        envelope("status", true, json!({})),
        entity("spawn"),
        entity("inspect"),
    ]);
    let mut commands = Vec::new();
    let mut audit = ark::SpawnAudit::new(Some(path.clone()), &input);
    let creature = ark::execute_spawn(
        &input,
        "arksurvivalascended",
        protocol::spawn_command(&input).unwrap(),
        &mut audit,
        |command| {
            commands.push(command);
            std::future::ready(Ok(replies.pop_front().unwrap()))
        },
    )
    .await
    .unwrap();
    assert!(
        !read_entries(&path)
            .iter()
            .any(|entry| entry["action"] == "instance.ark_creature.spawned")
    );
    audit.succeed(&creature);
    drop(audit);
    assert_eq!(commands.len(), 3);
    assert_eq!(
        commands[2],
        format!("LgsmArkTools.Inspect {} 123 456", input.request_id)
    );
    let entries = read_entries(&path);
    assert!(
        !entries
            .iter()
            .any(|entry| entry["action"] == "instance.ark_creature.failed")
    );
    let receipt = entries.last().unwrap();
    assert_eq!(receipt["action"], "instance.ark_creature.spawned");
    assert_eq!(receipt["context"]["phase"], "verified");
    assert_eq!(receipt["context"]["observed"]["id1"], 123);
    assert_eq!(receipt["context"]["observed"]["level"], input.level);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn ark_spawn_resolves_short_names_before_dispatch_and_verifies_the_canonical_class() {
    let root = temp_test_dir("ark-spawn-blueprint-resolution");
    let path = root.join("app.log");
    fs::write(
        root.join("Manifest_UFSFiles_Win64.txt"),
        "ShooterGame/Content/PrimalEarth/Dinos/Megalodon/MEgaMegalodon_Character_BP.uasset\t1601-01-01T00:00:00.000Z\n",
    )
    .unwrap();
    let original = "MegaMegalodon_Character_BP_C";
    let resolved =
        "/Game/PrimalEarth/Dinos/Megalodon/MEgaMegalodon_Character_BP.MEgaMegalodon_Character_BP_C";
    let mut input = spawn_input();
    input.creature = original.into();
    let mut observed = observed();
    observed.class_name = "MEgaMegalodon_Character_BP_C".into();
    let mut replies = VecDeque::from([
        envelope("status", true, json!({})),
        envelope("spawn", true, serde_json::to_value(&observed).unwrap()),
        envelope("inspect", true, serde_json::to_value(&observed).unwrap()),
    ]);
    let mut commands = Vec::new();
    let mut audit = ark::SpawnAudit::new(Some(path.clone()), &input);
    let creature = ark::resolve_and_execute_spawn(
        &mut input,
        root.clone(),
        "arksurvivalascended",
        &mut audit,
        |command| {
            commands.push(command);
            std::future::ready(Ok(replies.pop_front().unwrap()))
        },
    )
    .await
    .unwrap();
    audit.succeed(&creature);
    drop(audit);
    assert_eq!(input.creature, resolved);
    assert_eq!(creature.class_name, "MEgaMegalodon_Character_BP_C");
    assert_eq!(
        commands,
        vec![
            format!("LgsmArkTools.Status {}", input.request_id),
            format!(
                "LgsmArkTools.Spawn {} {resolved} 150 100 200 300 wild 0",
                input.request_id
            ),
            format!("LgsmArkTools.Inspect {} 123 456", input.request_id),
        ]
    );
    let entries = read_entries(&path);
    let receipt = entries.last().unwrap();
    assert_eq!(receipt["action"], "instance.ark_creature.spawned");
    assert_eq!(receipt["context"]["creature"], original);
    assert_eq!(receipt["context"]["resolved_creature"], resolved);
    assert_eq!(receipt["context"]["phase"], "verified");
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn ark_spawn_ambiguous_blueprints_are_logged_without_dispatch() {
    let root = temp_test_dir("ark-spawn-blueprint-ambiguity");
    let path = root.join("app.log");
    fs::write(
        root.join("Manifest_UFSFiles_Win64.txt"),
        "ShooterGame/Content/PrimalEarth/Dinos/Rex/Rex_Character_BP.uasset\t1601-01-01T00:00:00.000Z\nShooterGame/Content/Fixture/Dinos/Rex/Rex_Character_BP.uasset\t1601-01-01T00:00:00.000Z\n",
    )
    .unwrap();
    let mut input = spawn_input();
    let mut audit = ark::SpawnAudit::new(Some(path.clone()), &input);
    let error = ark::resolve_and_execute_spawn(
        &mut input,
        root.clone(),
        "arksurvivalascended",
        &mut audit,
        |_| -> std::future::Ready<Result<String, String>> {
            panic!("an ambiguous blueprint must not reach the transport");
        },
    )
    .await
    .unwrap_err();
    drop(audit);
    assert!(error.contains("未发送"));
    let entries = read_entries(&path);
    let failures = entries
        .iter()
        .filter(|entry| entry["action"] == "instance.ark_creature.failed")
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1);
    let context = &failures[0]["context"];
    assert_eq!(context["phase"], "blueprint_resolution");
    assert_eq!(context["outcome"], "not_sent");
    assert_eq!(context["error_code"], "blueprint_resolution_failed");
    assert_eq!(context["mutation_dispatch_attempted"], false);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn ark_spawn_preflight_failures_use_the_real_command_log()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = command_smoke_lock().lock().await;
    let fixture = ArkFixture::new("arksurvivalascended").await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let mut input = spawn_input();
    input.instance_id = fixture.id.clone();
    input.tamed = true;
    input.player_id = 42;
    input.creature = "invalid\npassword=fixture-password".into();
    let error = ark::spawn_ark_creature(app.state::<DesktopState>(), input)
        .await
        .err()
        .ok_or("invalid class was accepted")?;
    assert!(error.contains("未发送"));
    let mut input = spawn_input();
    input.instance_id = fixture.id.clone();
    let error = ark::spawn_ark_creature(app.state::<DesktopState>(), input)
        .await
        .err()
        .ok_or("stopped target was accepted")?;
    assert!(error.contains("未发送"));
    let entries = read_entries(&fixture.storage.paths.app_log_path());
    let failures = entries
        .iter()
        .filter(|entry| entry["action"] == "instance.ark_creature.failed")
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 2);
    assert_eq!(failures[0]["context"]["phase"], "validate");
    assert_eq!(failures[0]["context"]["creature"], "<invalid>");
    assert_eq!(failures[0]["context"]["tamed"], true);
    assert_eq!(failures[0]["context"]["player_id"], 42);
    assert_eq!(failures[1]["context"]["phase"], "target");
    for failure in failures {
        assert_eq!(failure["context"]["outcome"], "not_sent");
        assert_eq!(failure["context"]["mutation_dispatch_attempted"], false);
    }
    assert!(!serde_json::to_string(&entries)?.contains("fixture-password"));
    fixture.untouched();
    drop(app);
    fixture.finish()?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn ark_spawn_interrupted_wait_records_the_last_owned_stage() {
    let root = temp_test_dir("ark-spawn-interrupted");
    let path = root.join("app.log");
    let input = spawn_input();
    let mut audit = ark::SpawnAudit::new(Some(path.clone()), &input);
    let mut calls = 0;
    let operation = ark::execute_spawn(
        &input,
        "arksurvivalascended",
        protocol::spawn_command(&input).unwrap(),
        &mut audit,
        |_| {
            calls += 1;
            let call = calls;
            async move {
                if call == 1 {
                    Ok(envelope("status", true, json!({})))
                } else {
                    std::future::pending::<Result<String, String>>().await
                }
            }
        },
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), operation)
            .await
            .is_err()
    );
    drop(audit);
    assert_eq!(calls, 2);
    let entries = read_entries(&path);
    let failure = entries.last().unwrap();
    assert_eq!(failure["context"]["phase"], "spawn_dispatch");
    assert_eq!(failure["context"]["outcome"], "outcome_unknown");
    assert_eq!(failure["context"]["error_code"], "task_interrupted");
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn ark_spawn_refusal_codes_cannot_override_creation_or_cleanup_evidence() {
    for code in [
        "invalid_arguments",
        "owner_not_online",
        "invalid_dino_class",
        "world_not_ready",
        "request_capacity_reached",
    ] {
        for diagnostics in [
            json!({"stage":"spawn"}),
            json!({"stage":"taming_setup"}),
            json!({"stage":"tame"}),
            json!({"stage":"tame_verify"}),
            json!({"stage":"identity"}),
            json!({"stage":"inspect"}),
            json!({"id1":123}),
            json!({"id2":456}),
            json!({"cleanup":"destroy_requested"}),
            json!({"cleanup":"destroy_failed"}),
        ] {
            let root = temp_test_dir("ark-spawn-ambiguous-refusal");
            let path = root.join("app.log");
            let input = spawn_input();
            let mut replies = VecDeque::from([
                envelope("status", true, json!({})),
                envelope(
                    "spawn",
                    false,
                    json!({"error":code,"diagnostics":diagnostics}),
                ),
            ]);
            let mut calls = 0;
            let mut audit = ark::SpawnAudit::new(Some(path.clone()), &input);
            let failure = ark::execute_spawn(
                &input,
                "arksurvivalascended",
                protocol::spawn_command(&input).unwrap(),
                &mut audit,
                |_| {
                    calls += 1;
                    std::future::ready(Ok(replies
                        .pop_front()
                        .expect("uncertain mutations must not be retried")))
                },
            )
            .await
            .unwrap_err();
            let message = audit.fail(failure);
            drop(audit);
            assert_eq!(calls, 2);
            assert!(
                message.contains("结果未确认") && message.contains("勿重复生成"),
                "{code} {diagnostics}: {message}"
            );
            assert!(!message.contains("本次请求未执行"));
            let entries = read_entries(&path);
            let context = &entries.last().unwrap()["context"];
            assert_eq!(context["outcome"], "outcome_unknown");
            assert_eq!(context["native_code"], code);
            assert_eq!(context["native_diagnostics"], diagnostics);
            fs::remove_dir_all(root).unwrap();
        }
    }
}
