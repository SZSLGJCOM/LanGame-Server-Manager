use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpawnInput {
    pub instance_id: String,
    pub process_key: String,
    pub run_id: i64,
    pub request_id: String,
    pub creature: String,
    pub level: u32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub tamed: bool,
    pub player_id: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Creature {
    pub id1: u32,
    pub id2: u32,
    pub class_name: String,
    pub level: i32,
    pub team: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub tamed: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reply {
    version: u32,
    edition: String,
    action: String,
    request_id: String,
    ok: bool,
    error: Option<String>,
    diagnostics: Option<serde_json::Value>,
    #[serde(flatten)]
    entity: serde_json::Value,
}

/// Only fixed, non-secret native observations may enter the application log.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeDiagnostics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id1: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id2: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_team: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_team: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_tamed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_failure: Option<&'static str>,
}

fn allowed_value(
    value: Option<&serde_json::Value>,
    allowed: &[&'static str],
) -> Option<&'static str> {
    let value = value?.as_str()?;
    allowed
        .iter()
        .copied()
        .find(|candidate| *candidate == value)
}

impl NativeDiagnostics {
    fn parse(value: Option<&serde_json::Value>) -> Self {
        let Some(value) = value.and_then(serde_json::Value::as_object) else {
            return Self::default();
        };
        Self {
            stage: allowed_value(
                value.get("stage"),
                &[
                    "validate",
                    "owner",
                    "class",
                    "spawn",
                    "taming_setup",
                    "tame",
                    "tame_verify",
                    "identity",
                    "inspect",
                ],
            ),
            id1: value
                .get("id1")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| value.try_into().ok()),
            id2: value
                .get("id2")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| value.try_into().ok()),
            owner_team: value
                .get("ownerTeam")
                .and_then(serde_json::Value::as_i64)
                .and_then(|value| value.try_into().ok()),
            actual_team: value
                .get("actualTeam")
                .and_then(serde_json::Value::as_i64)
                .and_then(|value| value.try_into().ok()),
            actual_tamed: value
                .get("actualTamed")
                .and_then(serde_json::Value::as_bool),
            cleanup: allowed_value(
                value.get("cleanup"),
                &["not_needed", "destroy_requested", "destroy_failed"],
            ),
            owner_failure: allowed_value(
                value.get("ownerFailure"),
                &[
                    "not_online",
                    "character_unavailable",
                    "player_state_unavailable",
                    "team_unavailable",
                ],
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ReplyFailure {
    Unconfirmed {
        code: &'static str,
        message: &'static str,
    },
    Native {
        code: &'static str,
        diagnostics: NativeDiagnostics,
    },
}

impl ReplyFailure {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unconfirmed { code, .. } | Self::Native { code, .. } => code,
        }
    }

    pub fn native_code(&self) -> Option<&'static str> {
        match self {
            Self::Native { code, .. } => Some(code),
            Self::Unconfirmed { .. } => None,
        }
    }

    pub fn diagnostics(&self) -> Option<&NativeDiagnostics> {
        match self {
            Self::Native { diagnostics, .. } => Some(diagnostics),
            Self::Unconfirmed { .. } => None,
        }
    }
}

impl std::fmt::Display for ReplyFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unconfirmed { message, .. } => formatter.write_str(message),
            Self::Native { code, .. } => formatter.write_str(code),
        }
    }
}

fn unconfirmed(code: &'static str, message: &'static str) -> ReplyFailure {
    ReplyFailure::Unconfirmed { code, message }
}

pub fn edition(module: &str) -> Result<&'static str, String> {
    match module {
        "arksurvivalevolved" => Ok("ase"),
        "arksurvivalascended" => Ok("asa"),
        _ => Err("This instance does not support ARK creature tools.".into()),
    }
}

pub fn validate_request_id(value: &str) -> Result<(), String> {
    if value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("ARK tool request ID must contain 32 hexadecimal characters.".into())
    }
}

pub fn spawn_command(input: &SpawnInput) -> Result<String, String> {
    validate_request_id(&input.request_id)?;
    let class = &input.creature;
    let identifier = class.ends_with("_C")
        && class.len() <= 300
        && class
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_');
    let path = class.starts_with("/Game/")
        && class.len() <= 300
        && class.ends_with("_C")
        && !class.contains("..")
        && !class.contains("//")
        && class
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_/.-".contains(&b));
    if !identifier && !path {
        return Err("Creature must be a class identifier or a complete /Game/ class path.".into());
    }
    if !(1..=5000).contains(&input.level) {
        return Err("Creature level must be between 1 and 5000.".into());
    }
    if [input.x, input.y, input.z]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 10_000_000.0)
    {
        return Err("World coordinates must be finite and within ±10000000.".into());
    }
    if input.tamed != (input.player_id > 0) {
        return Err(
            "Tamed creatures require an online in-game Player ID; wild creatures use 0.".into(),
        );
    }
    let mode = if input.tamed { "tamed" } else { "wild" };
    let command = format!(
        "LgsmArkTools.Spawn {} {} {} {} {} {} {mode} {}",
        input.request_id, class, input.level, input.x, input.y, input.z, input.player_id
    );
    if command.len() > 512 {
        return Err("ARK tool command exceeds 512 bytes.".into());
    }
    Ok(command)
}

fn reply(
    text: &str,
    module: &str,
    action: &str,
    request_id: &str,
    maximum_bytes: usize,
) -> Result<Reply, ReplyFailure> {
    if text.len() > maximum_bytes {
        return Err(unconfirmed(
            "response_too_large",
            "ARK tool response exceeds the limit.",
        ));
    }
    // ARK may surround the plugin's response with its generic RCON acknowledgement.
    // Exactly one bounded envelope must belong to this operation.
    let mut lines = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("LGSM_ARK_TOOLS "));
    let payload = lines.next().ok_or_else(|| {
        unconfirmed(
            "missing_reply",
            "ARK creature extension did not respond. Check its server startup log.",
        )
    })?;
    if lines.next().is_some() {
        return Err(unconfirmed(
            "ambiguous_reply",
            "Ambiguous ARK tool response.",
        ));
    }
    let reply: Reply = serde_json::from_str(payload)
        .map_err(|_| unconfirmed("invalid_reply", "Invalid ARK tool response."))?;
    if reply.version != 1
        || Some(reply.edition.as_str()) != edition(module).ok()
        || reply.action != action
        || reply.request_id != request_id
    {
        return Err(unconfirmed(
            "mismatched_reply",
            "ARK tool response does not match this request, game edition, or protocol.",
        ));
    }
    if !reply.ok {
        // Never promote an uncorrelated or arbitrary error string to a native
        // refusal, or copy a response payload (which may contain secrets) to logs.
        let code = [
            "invalid_arguments",
            "owner_not_online",
            "invalid_dino_class",
            "world_not_ready",
            "request_id_conflict",
            "request_capacity_reached",
            "spawn_failed",
            "tame_failed",
            "entity_readback_failed",
            "entity_not_found",
            "entity_not_initialized",
            "outcome_unknown",
            "native_operation_failed",
            "response_limit_exceeded",
            "map_not_ready",
            "player_limit_exceeded",
        ]
        .into_iter()
        .find(|code| Some(*code) == reply.error.as_deref())
        .unwrap_or("unknown_native_error");
        return Err(ReplyFailure::Native {
            code,
            diagnostics: NativeDiagnostics::parse(reply.diagnostics.as_ref()),
        });
    }
    Ok(reply)
}

pub fn status(text: &str, module: &str, request_id: &str) -> Result<(), String> {
    status_reply(text, module, request_id).map_err(|error| error.to_string())
}

pub fn status_reply(text: &str, module: &str, request_id: &str) -> Result<(), ReplyFailure> {
    reply(text, module, "status", request_id, 8192).map(|_| ())
}

pub(crate) fn player_snapshot_payload(
    text: &str,
    module: &str,
    request_id: &str,
) -> Result<serde_json::Value, String> {
    reply(text, module, "players", request_id, 512 * 1024)
        .map(|value| value.entity)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
pub fn creature(
    text: &str,
    module: &str,
    action: &str,
    request_id: &str,
) -> Result<Creature, String> {
    creature_reply(text, module, action, request_id).map_err(|error| error.to_string())
}

pub fn creature_reply(
    text: &str,
    module: &str,
    action: &str,
    request_id: &str,
) -> Result<Creature, ReplyFailure> {
    let value: Creature =
        serde_json::from_value(reply(text, module, action, request_id, 8192)?.entity)
            .map_err(|_| unconfirmed("invalid_entity", "Invalid ARK creature record."))?;
    if (value.id1 == 0 && value.id2 == 0)
        || value.class_name.is_empty()
        || value.class_name.len() > 300
        || !(1..=20_000).contains(&value.level)
        || [value.x, value.y, value.z].iter().any(|v| !v.is_finite())
    {
        return Err(unconfirmed(
            "invalid_entity",
            "ARK extension returned an invalid creature identity or state.",
        ));
    }
    Ok(value)
}

pub fn verify_spawn(
    input: &SpawnInput,
    spawned: &Creature,
    inspected: &Creature,
) -> Result<(), String> {
    let class_name = input.creature.rsplit('.').next().unwrap_or(&input.creature);
    if spawned.id1 != inspected.id1
        || spawned.id2 != inspected.id2
        || spawned.class_name != inspected.class_name
        || inspected.class_name != class_name
        || inspected.tamed != input.tamed
        || spawned.team != inspected.team
        || spawned.level != inspected.level
        || spawned.level != input.level as i32
        || [
            (spawned.x, input.x),
            (spawned.y, input.y),
            (spawned.z, input.z),
        ]
        .iter()
        .any(|(actual, requested)| (actual - requested).abs() > 1.0)
    {
        return Err("ARK creature read-back did not match the generated entity. Do not repeat the spawn before checking the server.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> SpawnInput {
        SpawnInput {
            instance_id: "fixture".into(),
            process_key: "main".into(),
            run_id: 1,
            request_id: "0123456789abcdef0123456789abcdef".into(),
            creature: "Rex_Character_BP_C".into(),
            level: 150,
            x: 0.0,
            y: 0.0,
            z: 1000.0,
            tamed: false,
            player_id: 0,
        }
    }
    #[test]
    fn rejects_injection_invalid_coordinates_and_owner() {
        let mut value = input();
        assert!(spawn_command(&value).is_ok());
        value.creature = "Rex_Character_BP_C\nDestroyWildDinos".into();
        assert!(spawn_command(&value).is_err());
        value.creature = "/Game/PrimalEarth/Dinos/Rex/Rex_Character_BP.Rex_Character_BP_C".into();
        assert!(spawn_command(&value).is_ok());
        value.x = f64::NAN;
        assert!(spawn_command(&value).is_err());
        value.x = 0.0;
        value.tamed = true;
        assert!(spawn_command(&value).is_err());
        value.player_id = 42;
        assert!(spawn_command(&value).is_ok());
    }

    #[test]
    fn complete_paths_allow_directory_hyphens_without_accepting_command_injection() {
        let mut value = input();
        value.creature = "/Game/PrimalEarth/Dinos/Spider-Small/SpiderS_Character_BP_Aberrant.SpiderS_Character_BP_Aberrant_C".into();
        let command = spawn_command(&value).unwrap();
        assert_eq!(
            command.split_whitespace().nth(2),
            Some(value.creature.as_str())
        );
        for invalid in [
            "Spider-Small_C",
            "/Game/Spider-Small/../Rex.Rex_C",
            "/Game//Spider-Small/Spider.Spider_C",
            "/Game/Spider-Small/Spider.Spider_C\nDestroyWildDinos_C",
            "/Game/Spider-Small/Spider.Spider_C;SaveWorld_C",
            "/Game/Spider-Small/Spider.Spider_C SaveWorld_C",
        ] {
            value.creature = invalid.into();
            assert!(spawn_command(&value).is_err(), "accepted {invalid:?}");
        }
    }

    #[test]
    fn generic_ack_and_wrong_nonce_are_not_success() {
        let id = input().request_id;
        assert!(
            status(
                "Server received, But no response!!",
                "arksurvivalascended",
                &id
            )
            .is_err()
        );
        let valid = format!(
            "LGSM_ARK_TOOLS {{\"version\":1,\"edition\":\"asa\",\"action\":\"status\",\"requestId\":\"{id}\",\"ok\":true}}"
        );
        assert!(status(&valid, "arksurvivalascended", &id).is_ok());
        assert!(status(&valid, "arksurvivalevolved", &id).is_err());
        assert!(status(&valid, "arksurvivalascended", "wrong").is_err());
        for version in [0, 2] {
            assert!(
                status(
                    &valid.replace("\"version\":1", &format!("\"version\":{version}")),
                    "arksurvivalascended",
                    &id
                )
                .is_err()
            );
        }
        assert!(
            status(
                &valid.replace("\"action\":\"status\"", "\"action\":\"spawn\""),
                "arksurvivalascended",
                &id
            )
            .is_err()
        );
        assert!(status(&format!("{valid}\n{valid}"), "arksurvivalascended", &id).is_err());
    }
    #[test]
    fn readback_must_match_the_spawned_entity() {
        let creature = Creature {
            id1: 123,
            id2: 456,
            class_name: "Rex_Character_BP_C".into(),
            level: 150,
            team: 0,
            x: 0.0,
            y: 0.0,
            z: 1000.0,
            tamed: false,
        };
        let mut wrong = creature.clone();
        wrong.id2 = 457;
        assert!(verify_spawn(&input(), &creature, &wrong).is_err());
        assert!(verify_spawn(&input(), &creature, &creature).is_ok());
    }

    #[test]
    fn native_failures_require_a_matching_envelope_before_classification() {
        let id = input().request_id;
        let value = serde_json::json!({"version":1,"edition":"asa","action":"spawn",
            "requestId":id,"ok":false,"error":"owner_not_online"});
        let parse = |value: &serde_json::Value| {
            creature_reply(
                &format!("LGSM_ARK_TOOLS {value}"),
                "arksurvivalascended",
                "spawn",
                &id,
            )
            .unwrap_err()
        };
        assert_eq!(parse(&value).native_code(), Some("owner_not_online"));
        for (key, wrong) in [
            ("version", serde_json::json!(2)),
            ("edition", serde_json::json!("ase")),
            ("action", serde_json::json!("inspect")),
            (
                "requestId",
                serde_json::json!("fedcba9876543210fedcba9876543210"),
            ),
        ] {
            let mut mismatched = value.clone();
            mismatched[key] = wrong;
            let error = parse(&mismatched);
            assert_eq!(error.code(), "mismatched_reply");
            assert_eq!(
                error.native_code(),
                None,
                "a stale refusal cannot establish this mutation's result"
            );
            assert!(error.diagnostics().is_none());
        }
        let text = format!("LGSM_ARK_TOOLS {value}\nLGSM_ARK_TOOLS {value}");
        assert_eq!(
            creature_reply(&text, "arksurvivalascended", "spawn", &id)
                .unwrap_err()
                .code(),
            "ambiguous_reply"
        );
    }

    #[test]
    fn native_diagnostics_are_typed_and_unknown_fields_or_errors_are_never_copied() {
        let id = input().request_id;
        let value = serde_json::json!({"version":1,"edition":"asa","action":"spawn",
            "requestId":id,"ok":false,"error":"tame_failed","diagnostics":{
                "stage":"tame_verify","id1":123,"id2":456,"ownerTeam":10001,"actualTeam":7,
                "actualTamed":false,"cleanup":"destroy_requested","ownerFailure":"team_unavailable",
                "password":"fixture-password","command":"fixture-password"}});
        let parse = |value: &serde_json::Value| {
            creature_reply(
                &format!("LGSM_ARK_TOOLS {value}"),
                "arksurvivalascended",
                "spawn",
                &id,
            )
            .unwrap_err()
        };
        let error = parse(&value);
        assert_eq!(error.native_code(), Some("tame_failed"));
        let diagnostics = error.diagnostics().unwrap();
        assert_eq!(diagnostics.id1, Some(123));
        assert_eq!(diagnostics.actual_tamed, Some(false));
        assert_eq!(diagnostics.cleanup, Some("destroy_requested"));
        assert_eq!(diagnostics.owner_failure, Some("team_unavailable"));
        let safe = serde_json::to_string(diagnostics).unwrap();
        assert!(
            !safe.contains("fixture-password")
                && !safe.contains("password")
                && !safe.contains("command")
        );

        let mut invalid = value.clone();
        invalid["error"] = serde_json::json!("password=fixture-password");
        invalid["diagnostics"] = serde_json::json!({"stage":"fixture-password","id1":4294967296u64,
            "id2":-1,"ownerTeam":2147483648i64,"actualTeam":"7","actualTamed":"false",
            "cleanup":"deleted_and_verified","ownerFailure":"fixture-password"});
        let error = parse(&invalid);
        assert_eq!(error.native_code(), Some("unknown_native_error"));
        assert_eq!(error.diagnostics(), Some(&NativeDiagnostics::default()));
        assert!(!error.to_string().contains("fixture-password"));
    }
}
