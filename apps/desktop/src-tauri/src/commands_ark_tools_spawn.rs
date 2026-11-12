use super::{SpawnInput, blueprints, protocol};
use serde_json::json;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub(in crate::commands) struct SpawnFailure {
    outcome: &'static str,
    code: &'static str,
    native_code: Option<&'static str>,
    diagnostics: Option<Box<protocol::NativeDiagnostics>>,
    message: String,
}

impl SpawnFailure {
    fn host(outcome: &'static str, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            outcome,
            code,
            native_code: None,
            diagnostics: None,
            message: message.into(),
        }
    }

    fn reply(
        error: protocol::ReplyFailure,
        mutation_sent: bool,
        entity: Option<(u32, u32)>,
    ) -> Self {
        let native_code = error.native_code();
        let diagnostics = error.diagnostics().cloned().map(Box::new);
        let code = error.code();
        // A native argument exception can occur inside SpawnDino before its
        // pointer is assigned. A refusal code alone cannot override evidence
        // that creation or cleanup may already have started.
        let creation_may_have_started = diagnostics.as_ref().is_some_and(|value| {
            value.id1.is_some_and(|id| id != 0)
                || value.id2.is_some_and(|id| id != 0)
                || matches!(value.cleanup, Some("destroy_requested" | "destroy_failed"))
                || matches!(
                    value.stage,
                    Some(
                        "spawn" | "taming_setup" | "tame" | "tame_verify" | "identity" | "inspect"
                    )
                )
        });
        let (outcome, message) = if !mutation_sent {
            (
                "not_sent",
                "扩展握手未通过，生成请求未发送。请检查扩展启动日志和世界加载状态。".to_owned(),
            )
        } else if let Some((id1, id2)) = entity {
            (
                "spawned_unverified",
                format!("生物 {id1}:{id2} 已返回，但独立读回未通过；请检查服务器，勿重复生成。"),
            )
        } else {
            match native_code {
                Some("invalid_arguments" | "owner_not_online" | "invalid_dino_class" | "world_not_ready" | "request_capacity_reached") if creation_may_have_started =>
                    ("outcome_unknown", "扩展返回了拒绝码，但回执同时显示可能已经进入生物生成或清理阶段；结果未确认。请检查服务器，勿重复生成。".into()),
                Some("invalid_arguments") => ("rejected", "服务器拒绝了生成参数，本次请求未执行。请检查生物类、等级、坐标和目标玩家。".into()),
                Some("owner_not_online") => {
                    let reason = match diagnostics.as_ref().and_then(|value| value.owner_failure) {
                        Some("character_unavailable") => "目标玩家的角色尚未就绪",
                        Some("player_state_unavailable") => "目标玩家的原生玩家状态尚未就绪",
                        Some("team_unavailable") => "目标玩家的所属队伍尚未就绪",
                        _ => "服务器未找到可用于驯服的在线玩家",
                    };
                    ("rejected", format!("{reason}，本次请求未执行。请确认玩家已进入当前地图，刷新玩家列表后重试。"))
                }
                Some("invalid_dino_class") => ("rejected", "服务器未能加载指定生物类，本次请求未执行。请检查完整蓝图路径及相关模组是否已加载。".into()),
                Some("world_not_ready") => ("rejected", "世界尚未就绪，本次请求未执行。请等待服务器完成加载。".into()),
                Some("request_capacity_reached") => ("rejected", "扩展的生成请求记录已达上限，本次请求未执行。请检查扩展日志。".into()),
                Some("spawn_failed") => ("native_failed", "原生生成函数未返回生物，生成失败；请检查服务器日志后再决定是否重试。".into()),
                Some("tame_failed" | "entity_readback_failed" | "entity_not_found" | "entity_not_initialized") => {
                    let reason = if native_code == Some("tame_failed") { "生物生成后的原生驯服校验失败" } else { "生物生成后的原生实体读回失败" };
                    let cleanup = match diagnostics.as_ref().and_then(|value| value.cleanup) {
                        Some("destroy_requested") => "扩展已请求删除，但尚未确认实体已移除",
                        Some("destroy_failed") => "扩展删除调用未成功，实体可能仍在",
                        _ => "回执未确认清理结果，实体可能仍在",
                    };
                    ("native_failed", format!("{reason}；{cleanup}。请检查服务器，勿重复生成。"))
                }
                Some("request_id_conflict") => ("outcome_unknown", "生成请求 ID 与已有记录冲突，本次命令未执行，但先前请求可能已生成生物。请检查服务器，勿重复生成。".into()),
                _ => ("outcome_unknown", "生成请求已发出，但结果未确认，可能已经生成生物。请检查服务器，勿重复生成。".into()),
            }
        };
        Self {
            outcome,
            code,
            native_code,
            diagnostics,
            message,
        }
    }
}

/// The audit survives a disappearing IPC waiter with the owned mutation task.
/// Its drop receipt also covers a panic/cancellation without claiming a rollback.
pub(in crate::commands) struct SpawnAudit {
    pub(super) path: Option<PathBuf>,
    pub(super) context: serde_json::Value,
    phase: &'static str,
    mutation_sent: bool,
    entity: Option<(u32, u32)>,
    finished: bool,
}

fn spawn_log_identifier(value: &str, maximum: usize, class: bool) -> &str {
    if !value.is_empty()
        && value.len() <= maximum
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || b"_-".contains(&byte) || class && b"/.".contains(&byte)
        })
    {
        value
    } else {
        "<invalid>"
    }
}

impl SpawnAudit {
    pub(in crate::commands) fn new(path: Option<PathBuf>, input: &SpawnInput) -> Self {
        let audit = Self {
            path,
            context: json!({"instance_id":spawn_log_identifier(&input.instance_id, 128, false),
                "process_key":spawn_log_identifier(&input.process_key, 36, false), "run_id":input.run_id,
                "request_id": if protocol::validate_request_id(&input.request_id).is_ok() { input.request_id.as_str() } else { "<invalid>" },
                "creature":spawn_log_identifier(&input.creature, 300, true), "level":input.level,
                "tamed":input.tamed, "player_id":input.player_id, "x":input.x, "y":input.y, "z":input.z}),
            phase: "validate",
            mutation_sent: false,
            entity: None,
            finished: false,
        };
        audit.record("info", "instance.ark_creature.started", None);
        audit
    }

    pub(super) fn advance(&mut self, phase: &'static str) {
        self.phase = phase;
    }

    fn record(&self, level: &str, action: &str, failure: Option<&SpawnFailure>) -> bool {
        let mut context = self.context.clone();
        context["phase"] = json!(self.phase);
        context["mutation_dispatch_attempted"] = json!(self.mutation_sent);
        context["entity_id"] = json!(self.entity);
        if let Some(failure) = failure {
            context["outcome"] = json!(failure.outcome);
            context["error_code"] = json!(failure.code);
            context["native_code"] = json!(failure.native_code);
            context["native_diagnostics"] = json!(failure.diagnostics);
        }
        let entry = json!({"ts_unix_ms":SystemTime::now().duration_since(UNIX_EPOCH).map(|value| value.as_millis()).unwrap_or(0),
            "level":level, "action":action, "message":"ARK creature generation diagnostic", "context":context});
        if let Some(path) = &self.path
            && crate::desktop_app_log::append(path, &entry).is_ok()
        {
            return true;
        }
        // The selected storage may itself be unavailable. Retain the safe
        // diagnostic on stderr and tell the IPC caller that persistence failed.
        eprintln!("ARK creature diagnostic could not be persisted: {entry}");
        false
    }

    pub(in crate::commands) fn fail(&mut self, failure: SpawnFailure) -> String {
        self.finished = true;
        let persisted = self.record("error", "instance.ark_creature.failed", Some(&failure));
        let native = failure
            .native_code
            .map(|code| format!(" 原生错误：{code}。"))
            .unwrap_or_default();
        format!(
            "{}{} 请求 ID：{}。{}",
            failure.message,
            native,
            self.context["request_id"].as_str().unwrap_or("<invalid>"),
            if persisted {
                ""
            } else {
                "诊断日志未能写入，请同时保留服务器控制台日志。"
            }
        )
    }

    pub(super) fn before_send(
        &mut self,
        code: &'static str,
        error: impl std::fmt::Display,
    ) -> String {
        self.fail(SpawnFailure::host(
            "not_sent",
            code,
            format!("生成请求未发送：{error}"),
        ))
    }

    fn observe(&mut self, key: &str, creature: &protocol::Creature) {
        self.context[key] = json!({"id1":creature.id1,"id2":creature.id2,
            "class":spawn_log_identifier(&creature.class_name, 300, true),"level":creature.level,
            "team":creature.team,"tamed":creature.tamed,"x":creature.x,"y":creature.y,"z":creature.z});
    }

    pub(in crate::commands) fn succeed(&mut self, creature: &protocol::Creature) {
        self.phase = "verified";
        self.context["outcome"] = json!("verified");
        self.observe("observed", creature);
        self.finished = true;
        self.record("info", "instance.ark_creature.spawned", None);
    }
}

impl Drop for SpawnAudit {
    fn drop(&mut self) {
        if !self.finished {
            let outcome = if self.entity.is_some() {
                "spawned_unverified"
            } else if self.mutation_sent {
                "outcome_unknown"
            } else {
                "not_sent"
            };
            self.record(
                "error",
                "instance.ark_creature.failed",
                Some(&SpawnFailure::host(
                    outcome,
                    "task_interrupted",
                    "生成任务意外中止，请检查服务器后再操作。",
                )),
            );
        }
    }
}

pub(in crate::commands) async fn resolve_and_execute_spawn<F, Fut>(
    input: &mut SpawnInput,
    root: PathBuf,
    module: &str,
    audit: &mut SpawnAudit,
    dispatch: F,
) -> Result<protocol::Creature, String>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    audit.advance("blueprint_resolution");
    let creature = input.creature.clone();
    input.creature = tokio::task::spawn_blocking(move || blueprints::resolve(&root, &creature))
        .await
        .map_err(|_| {
            audit.before_send(
                "blueprint_resolution_failed",
                "生物蓝图解析任务意外中止，请检查服务器程序清单。",
            )
        })?
        .map_err(|error| audit.before_send("blueprint_resolution_failed", error))?;
    audit.context["resolved_creature"] = json!(spawn_log_identifier(&input.creature, 300, true));
    audit.advance("resolved_validate");
    let command = protocol::spawn_command(input)
        .map_err(|error| audit.before_send("invalid_resolved_blueprint", error))?;
    execute_spawn(input, module, command, audit, dispatch)
        .await
        .map_err(|error| audit.fail(error))
}

pub(in crate::commands) async fn execute_spawn<F, Fut>(
    input: &SpawnInput,
    module: &str,
    command: String,
    audit: &mut SpawnAudit,
    mut dispatch: F,
) -> Result<protocol::Creature, SpawnFailure>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    audit.advance("handshake_dispatch");
    let response = dispatch(format!("LgsmArkTools.Status {}", input.request_id))
        .await
        .map_err(|_| {
            SpawnFailure::host(
                "not_sent",
                "handshake_transport_failed",
                "扩展握手连接失败，生成请求未发送。请检查服务器和 RCON 连接。",
            )
        })?;
    audit.advance("handshake_reply");
    protocol::status_reply(&response, module, &input.request_id)
        .map_err(|error| SpawnFailure::reply(error, false, None))?;
    audit.advance("spawn_dispatch");
    audit.mutation_sent = true;
    audit.record("info", "instance.ark_creature.dispatching", None);
    let response = dispatch(command).await.map_err(|_| {
        SpawnFailure::host(
            "outcome_unknown",
            "spawn_transport_failed",
            "生成请求的回执未确认，可能已经生成生物。请检查服务器，勿重复生成。",
        )
    })?;
    audit.advance("spawn_reply");
    let spawned = protocol::creature_reply(&response, module, "spawn", &input.request_id)
        .map_err(|error| SpawnFailure::reply(error, true, None))?;
    audit.entity = Some((spawned.id1, spawned.id2));
    audit.observe("spawn_receipt", &spawned);
    audit.advance("inspect_dispatch");
    let response = dispatch(format!(
        "LgsmArkTools.Inspect {} {} {}",
        input.request_id, spawned.id1, spawned.id2
    ))
    .await
    .map_err(|_| {
        SpawnFailure::host(
            "spawned_unverified",
            "inspect_transport_failed",
            format!(
                "生物 {}:{} 已返回，但独立读回连接失败；请检查服务器，勿重复生成。",
                spawned.id1, spawned.id2
            ),
        )
    })?;
    audit.advance("inspect_reply");
    let inspected = protocol::creature_reply(&response, module, "inspect", &input.request_id)
        .map_err(|error| SpawnFailure::reply(error, true, audit.entity))?;
    audit.observe("inspect_receipt", &inspected);
    audit.advance("verify");
    protocol::verify_spawn(input, &spawned, &inspected).map_err(|_| SpawnFailure::host("spawned_unverified", "spawn_readback_mismatch",
        format!("生物 {}:{} 已返回，但读回的生物类型、等级、位置或驯服归属与请求不一致；请检查服务器，勿重复生成。", spawned.id1, spawned.id2)))?;
    Ok(inspected)
}
