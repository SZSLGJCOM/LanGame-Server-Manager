use super::super::assistant_repair_integration_tests::read_repair_model_request;
use super::super::assistant_tool_fixtures::{assert_native_tool_available, openai_tool_response};
use super::launch_tests::{NativeLaunchDriver, run_native_new_launch};
use super::*;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

const SCRIPTED_PROVIDER: &str = "scripted-local-contract";

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires elevated opt-in native acceptance and owned DST files; uses scripted localhost responses, not a model"]
async fn assistant_live_contract_launches_new_native_dst_server() -> LiveResult {
    let _guard = command_smoke_lock().lock().await;
    let mut provider = stored_openai_compatible_ai_mock_settings();
    provider.model = SCRIPTED_PROVIDER.into();
    let mut environment = LiveEnvironment::from_env_with_provider(provider)?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    environment.provider.base_url = format!("http://{}/v1", listener.local_addr()?);
    let run_root = real_smoke_support::allocate_smoke_run_root("ai")?;
    let mut evidence = Value::Null;
    let mut responses = Vec::new();
    let (stop, stopped) = oneshot::channel();
    let exercise = async {
        let result = Box::pin(run_native_new_launch(
            &environment,
            &run_root,
            &mut evidence,
            NativeLaunchDriver::ScriptedLocalContract,
        ))
        .await;
        let _ = stop.send(());
        result
    };
    // Both futures stay owned by this test; an early failure still lets the
    // shared native runner finish its process cleanup and retain evidence.
    let (outcome, provider_result) = tokio::join!(
        Box::pin(exercise),
        Box::pin(serve_launch_contract(
            listener,
            stopped,
            &run_root,
            &mut responses
        ))
    );
    let actions: Vec<_> = responses
        .iter()
        .filter_map(|response| response["action"].as_str())
        .collect();
    let provider_result = provider_result.and_then(|()| {
        if actions == ["create_server", "customize_config", "start_server"] {
            Ok(())
        } else {
            Err("scripted provider did not complete the three ordered launch stages".into())
        }
    });
    evidence["scriptedProvider"] = json!({"responses":responses,
        "requestCount":responses.len(), "operationCount":actions.len(), "passed":provider_result.is_ok(),
        "scope":"deterministic execution contract; no model decision was evaluated"});
    let mut errors = Vec::new();
    if let Err(error) = outcome {
        errors.push(error.to_string());
    }
    if let Err(error) = provider_result {
        evidence["scriptedProvider"]["failure"] = json!(error.to_string());
        errors.push(error.to_string());
    }
    evidence["passed"] = json!(errors.is_empty());
    if !errors.is_empty() {
        evidence["failure"] = json!(errors.join("\n"));
    }
    write_evidence(&run_root, &evidence)?;
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n").into())
    }
}

async fn serve_launch_contract(
    listener: TcpListener,
    mut stopped: oneshot::Receiver<()>,
    run_root: &Path,
    responses: &mut Vec<Value>,
) -> LiveResult {
    loop {
        let (mut stream, peer) = tokio::select! {
            _ = &mut stopped => return Ok(()),
            connection = listener.accept() => connection?,
        };
        if !peer.ip().is_loopback() || responses.len() >= 5 {
            return Err("scripted launch provider received an out-of-scope request".into());
        }
        let exchange = async {
            let request = read_repair_model_request(&mut stream).await?;
            if request["model"] != SCRIPTED_PROVIDER
                || !request.to_string().contains("Application task contract")
                || !request.to_string().contains("本地开服验收")
            {
                return Err("scripted launch request is missing its task context".into());
            }
            let index = responses.len();
            let (name, plan) = match index {
                0 => ("record_task_requirements", native_launch_requirements()),
                1 => ("finish_task_requirements", json!({})),
                _ => (
                    "propose_operation",
                    scripted_launch_plan(index - 2, &request, run_root).await?,
                ),
            };
            assert_native_tool_available(&request, name);
            if index == 1 || index == 2 {
                let result = request["messages"]
                    .as_array()
                    .ok_or("messages missing")?
                    .iter()
                    .find(|message| {
                        message["role"] == "tool"
                            && message["tool_call_id"] == format!("contract-{}", index - 1)
                    })
                    .ok_or("draft result lost its native call ID")?;
                let result: Value =
                    serde_json::from_str(result["content"].as_str().ok_or("result missing")?)?;
                if result["ok"] != true || (index == 1 && result["data"]["errors"] != json!([])) {
                    return Err("scripted requirement draft did not succeed".into());
                }
            }
            let body =
                openai_tool_response(&format!("contract-{index}"), name, plan.clone()).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await?;
            stream.shutdown().await?;
            responses.push(plan);
            Ok::<_, Box<dyn std::error::Error>>(())
        };
        tokio::time::timeout(Duration::from_secs(15), Box::pin(exchange))
            .await
            .map_err(|_| "scripted launch HTTP exchange exceeded its time limit")??;
    }
}

async fn scripted_launch_plan(index: usize, request: &Value, run_root: &Path) -> LiveResult<Value> {
    // Read the actual isolated database after the preceding confirmation. The
    // provider cannot choose an unrelated target or synthesize a created ID.
    if !app_storage::StoragePaths::default()
        .app_data_root
        .starts_with(run_root)
    {
        return Err("scripted launch storage isolation is no longer active".into());
    }
    let storage = bootstrap_storage()?;
    if storage.paths.instances_root != run_root.join("i") {
        return Err("scripted launch database is not bound to the owned instance directory".into());
    }
    let instances = list_instances(&storage.paths).await?;
    if index == 0 {
        if !instances.is_empty() {
            return Err("scripted creation requires the initially empty database".into());
        }
        return Ok(json!({"action":"create_server", "moduleId":"dontstarve",
            "reason":"Create the requested server before configuring and starting it."}));
    }
    if instances.len() != 1 || instances[0].module_id != "dontstarve" {
        return Err("scripted launch continuation requires exactly its one created server".into());
    }
    let current = read_instance_details(&storage.paths, &instances[0].id).await?;
    if current.active_run.is_some() || !request.to_string().contains(&current.summary.id) {
        return Err("scripted continuation is not bound to the stopped created server".into());
    }
    let requested = json!({"cluster_name":"本地开服验收", "offline_cluster":true,
        "lan_only_cluster":true, "disable_data_collection":true, "enable_caves":false,
        "master_world_size":"small", "max_players":4, "bind_ip":"127.0.0.1"});
    match index {
        1 => Ok(json!({"action":"customize_config", "moduleId":"dontstarve",
            "instanceId":current.summary.id, "settingsPatch":requested,
            "reason":"Apply all explicitly requested server settings before startup."})),
        2 => {
            let saved: Value = serde_json::from_str(&current.settings_json)?;
            for (key, value) in requested.as_object().ok_or("requested settings missing")? {
                if saved.get(key) != Some(value) {
                    return Err(format!("scripted startup requires the saved setting {key}").into());
                }
            }
            if current.summary.bind_ip != "127.0.0.1" {
                return Err("scripted startup requires the saved loopback bind address".into());
            }
            Ok(json!({"action":"start_server", "moduleId":"dontstarve",
                "instanceId":current.summary.id,
                "reason":"The complete requested configuration is saved; start after confirmation."}))
        }
        _ => Err("scripted launch has no additional response stage".into()),
    }
}

fn native_launch_requirements() -> Value {
    let settings: Vec<Value> = [
        ("cluster_name", json!("本地开服验收"), "request_2"),
        ("offline_cluster", json!(true), "request_2"),
        ("lan_only_cluster", json!(true), "request_3"),
        ("disable_data_collection", json!(true), "request_8"),
        ("enable_caves", json!(false), "request_4"),
        ("master_world_size", json!("small"), "request_6"),
        ("max_players", json!(4), "request_7"),
        ("bind_ip", json!("127.0.0.1"), "request_5"),
    ]
    .into_iter()
    .map(|(key, expected, source)| {
        json!({
            "key":key, "expected":expected, "sourceId":source,
        })
    })
    .collect();
    let forbidden: Vec<Value> = [
        ("install_server", "request_10"),
        ("validate_server", "request_11"),
        ("install_fun_mod", "request_9"),
        ("install_site_mod", "request_9"),
    ]
    .into_iter()
    .map(|(action, source)| json!({"action":action, "sourceId":source}))
    .collect();
    json!({"settings":settings,"ports":[],"forbiddenActions":forbidden,"unverified":[]})
}
