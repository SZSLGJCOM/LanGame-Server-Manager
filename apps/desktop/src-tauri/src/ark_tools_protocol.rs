use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpawnInput {
    pub instance_id: String,
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
    #[serde(flatten)]
    entity: serde_json::Value,
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
            .all(|b| b.is_ascii_alphanumeric() || b"_/.".contains(&b));
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

fn reply(text: &str, module: &str, action: &str, request_id: &str) -> Result<Reply, String> {
    if text.len() > 8192 {
        return Err("ARK tool response exceeds the limit.".into());
    }
    // ARK may surround the plugin's response with its generic RCON acknowledgement.
    // Exactly one bounded envelope must belong to this operation.
    let mut lines = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("LGSM_ARK_TOOLS "));
    let payload = lines
        .next()
        .ok_or("ARK creature extension did not respond. Check its server startup log.")?;
    if lines.next().is_some() {
        return Err("Ambiguous ARK tool response.".into());
    }
    let reply: Reply =
        serde_json::from_str(payload).map_err(|e| format!("Invalid ARK tool response: {e}"))?;
    if reply.version != 1
        || reply.edition != edition(module)?
        || reply.action != action
        || reply.request_id != request_id
    {
        return Err(
            "ARK tool response does not match this request, game edition, or protocol.".into(),
        );
    }
    if !reply.ok {
        return Err(reply
            .error
            .filter(|e| !e.is_empty() && e.len() <= 2048)
            .unwrap_or_else(|| "ARK creature extension rejected the request.".into()));
    }
    Ok(reply)
}

pub fn status(text: &str, module: &str, request_id: &str) -> Result<(), String> {
    reply(text, module, "status", request_id).map(|_| ())
}

pub fn creature(
    text: &str,
    module: &str,
    action: &str,
    request_id: &str,
) -> Result<Creature, String> {
    let value: Creature = serde_json::from_value(reply(text, module, action, request_id)?.entity)
        .map_err(|e| format!("Invalid ARK creature record: {e}"))?;
    if (value.id1 == 0 && value.id2 == 0)
        || value.class_name.is_empty()
        || value.class_name.len() > 300
        || !(1..=20_000).contains(&value.level)
        || [value.x, value.y, value.z].iter().any(|v| !v.is_finite())
    {
        return Err("ARK extension returned an invalid creature identity or state.".into());
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
}
