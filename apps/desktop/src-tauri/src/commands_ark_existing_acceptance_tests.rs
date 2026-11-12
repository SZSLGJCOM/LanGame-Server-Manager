//! Explicit existing-backend ARK acceptance. No fixture, start/stop, installation,
//! arbitrary command, automatic resend, or player/account-name evidence.
use crate::commands::commands_ark_tools::protocol::Creature;
use crate::runtime_service::ExistingClient;
use app_core::InstanceDetails;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::windows::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const CONTRACT_ENV: &str = "LANGAME_ARK_EXISTING_ACCEPTANCE_CONTRACT";
const DODO: &str = "/Game/PrimalEarth/Dinos/Dodo/Dodo_Character_BP.Dodo_Character_BP_C";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Contract {
    schema_version: u32,
    backend: Backend,
    instance_id: String,
    #[serde(default = "main_process")]
    process_key: String,
    expected_run_id: i64,
    evidence_directory: PathBuf,
    #[serde(default)]
    mode: Mode,
    mutation: Option<Mutation>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Backend {
    executable: PathBuf,
    pid: u32,
    creation_time: u64,
}

#[derive(Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Mode {
    #[default]
    ReadOnly,
    SpawnTamedDodoOnce,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Mutation {
    request_id: String,
    player_id: Option<u32>,
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Scope {
    instance_id: String,
    process_key: String,
    run_id: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolsStatus {
    installed: bool,
    update_available: bool,
    connected: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MapSnapshot {
    #[serde(flatten)]
    scope: Scope,
    sampled_at_unix_ms: u64,
    players: Vec<Player>,
}

// Deliberately do not deserialize account IDs or names, or serialize raw maps.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Player {
    player_id: u32,
    team: i32,
    x: Option<f64>,
    y: Option<f64>,
    z: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Owner {
    player_id: u32,
    team: i32,
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpawnReply {
    #[serde(flatten)]
    scope: Scope,
    request_id: String,
    creature: Creature,
}

fn main_process() -> String {
    "main".into()
}

fn unix_ms() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .map_err(|_| "system clock is unavailable".into())
}

fn identifier(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

impl Contract {
    fn load() -> Result<Self, String> {
        let path = std::env::var_os(CONTRACT_ENV)
            .map(PathBuf::from)
            .ok_or("explicit private ARK acceptance contract is required")?;
        let metadata = fs::symlink_metadata(&path).map_err(|_| "contract is unavailable")?;
        if !path.is_absolute()
            || !metadata.is_file()
            || metadata.file_attributes() & 0x400 != 0
            || metadata.len() > 65536
        {
            return Err("contract must be a small absolute plain JSON file".into());
        }
        let mut contract: Self =
            serde_json::from_slice(&fs::read(path).map_err(|_| "cannot read private contract")?)
                .map_err(|_| "invalid private contract schema")?;
        if let Some(mutation) = &mut contract.mutation {
            mutation.request_id.make_ascii_lowercase();
        }
        contract.validate()?;
        Ok(contract)
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || !self.backend.executable.is_absolute()
            || self.backend.pid == 0
            || self.backend.creation_time == 0
            || !identifier(&self.instance_id, 128)
            || !identifier(&self.process_key, 36)
            || self.expected_run_id <= 0
        {
            return Err("contract requires an exact backend identity and current ARK run".into());
        }
        match (&self.mode, &self.mutation) {
            (Mode::ReadOnly, None) => Ok(()),
            (Mode::SpawnTamedDodoOnce, Some(mutation))
                if mutation.request_id.len() == 32
                    && mutation
                        .request_id
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit())
                    && mutation.player_id != Some(0) =>
            {
                Ok(())
            }
            _ => Err("mutation requires explicit mode and a unique 32-hex requestId".into()),
        }
    }

    fn scope(&self) -> Scope {
        Scope {
            instance_id: self.instance_id.clone(),
            process_key: self.process_key.clone(),
            run_id: self.expected_run_id,
        }
    }

    fn evidence_directory(&self) -> Result<PathBuf, String> {
        let path = &self.evidence_directory;
        if !path.is_absolute()
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err("evidence directory must be absolute without parent traversal".into());
        }
        // Require an already prepared private task directory; never create a
        // directory in a package/save tree based on a mistyped contract.
        for ancestor in path.ancestors() {
            let metadata = fs::symlink_metadata(ancestor)
                .map_err(|_| "evidence directory must already exist")?;
            if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
                return Err("evidence directory must not traverse a reparse point".into());
            }
        }
        let directory =
            dunce::canonicalize(path).map_err(|_| "cannot resolve evidence directory")?;
        let workspace = dunce::canonicalize(super::workspace_root())
            .map_err(|_| "cannot resolve source root")?;
        if directory.starts_with(workspace) {
            return Err("evidence directory must be outside the source checkout".into());
        }
        Ok(directory)
    }
}

async fn read<T: DeserializeOwned>(
    client: &ExistingClient,
    command: &str,
    args: Value,
) -> Result<T, String> {
    // ExistingClient redacts backend errors before they cross this boundary.
    serde_json::from_value(client.request(command, args).await?)
        .map_err(|_| format!("invalid {command} response"))
}

async fn verify_current_run(client: &ExistingClient, contract: &Contract) -> Result<(), String> {
    let details: InstanceDetails = read(
        client,
        "read_instance_details_from_storage",
        json!({"instanceId":contract.instance_id}),
    )
    .await?;
    if details.summary.id != contract.instance_id {
        return Err("instance response identity does not match the contract".into());
    }
    crate::commands::commands_ark_map::selected_target(
        &details,
        &contract.process_key,
        contract.expected_run_id,
    )
    .map(|_| ())
    .map_err(|_| "selected ARK process is not running with the contracted run".into())
}

fn coordinate(value: f64) -> bool {
    value.is_finite() && value.abs() <= 10_000_000.0
}

fn select_owner(map: &MapSnapshot, player_id: Option<u32>) -> Result<Owner, String> {
    let player = if let Some(id) = player_id {
        let mut matching = map.players.iter().filter(|player| player.player_id == id);
        let player = matching.next().ok_or("selected player is not online")?;
        if matching.next().is_some() {
            return Err("online player identity is ambiguous".into());
        }
        player
    } else if map.players.len() == 1 {
        &map.players[0]
    } else {
        return Err("mutation requires exactly one online player or an explicit playerId".into());
    };
    let (Some(x), Some(y), Some(z)) = (player.x, player.y, player.z) else {
        return Err("online player has no complete position".into());
    };
    if player.player_id == 0 || player.team <= 0 || ![x, y, z].into_iter().all(coordinate) {
        return Err("online player has no valid taming team or position".into());
    }
    Ok(Owner {
        player_id: player.player_id,
        team: player.team,
        x,
        y,
        z,
    })
}

fn verify_spawn(
    reply: &SpawnReply,
    contract: &Contract,
    mutation: &Mutation,
    owner: &Owner,
) -> Result<(), String> {
    let creature = &reply.creature;
    if reply.scope != contract.scope() || reply.request_id != mutation.request_id {
        return Err("spawn response instance/process/run/request identity mismatch".into());
    }
    if (creature.id1 == 0 && creature.id2 == 0)
        || creature.class_name != "Dodo_Character_BP_C"
        || creature.level != 37
        || !creature.tamed
        || creature.team != owner.team
        || ![creature.x, creature.y, creature.z]
            .into_iter()
            .all(coordinate)
    {
        return Err(
            "spawn read-back does not confirm level-37 tamed Dodo with the mapped owner team"
                .into(),
        );
    }
    Ok(())
}

fn write_new(path: &Path, value: &Value) -> Result<(), String> {
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|_| "cannot encode acceptance evidence")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "evidence already exists or cannot be exclusively created; request refused")?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| "evidence could not be durably written; request refused".into())
}

fn reserve_mutation(
    directory: &Path,
    contract: &Contract,
    mutation: &Mutation,
    intent: &Value,
) -> Result<(), String> {
    // Any record, including a truncated write or failed/uncertain attempt,
    // blocks reuse. A second requestId cannot bypass the same-run one-shot guard.
    write_new(
        &directory.join(format!("mutation-{}.json", mutation.request_id)),
        intent,
    )?;
    write_new(
        &directory.join(format!(
            "dodo-once-{}-{}-{}.json",
            contract.instance_id, contract.process_key, contract.expected_run_id
        )),
        intent,
    )
}

async fn execute(contract: &Contract, directory: &Path, report: &mut Value) -> Result<(), String> {
    let client = ExistingClient::new(
        &contract.backend.executable,
        contract.backend.pid,
        contract.backend.creation_time,
    )?;
    let status = client.request("runtime_service_status", json!({})).await?;
    if status["pid"].as_u64() != Some(u64::from(contract.backend.pid))
        || status["protocol"].as_u64() != Some(u64::from(crate::runtime_service::PROTOCOL))
        || status["shutdown_in_progress"].as_bool() != Some(false)
        || status["final_exit_requested"].as_bool() != Some(false)
    {
        return Err("selected backend is not ready for ARK acceptance".into());
    }
    report["backendIdentityVerified"] = json!(true);
    verify_current_run(&client, contract).await?;
    report["stage"] = json!("tools_status");
    let tools: ToolsStatus = read(
        &client,
        "read_ark_tools_status",
        json!({"input":{
            "instanceId":contract.instance_id,"processKey":contract.process_key
        }}),
    )
    .await?;
    report["tools"] = json!({"installed":tools.installed,
        "connected":tools.connected,"updateAvailable":tools.update_available});
    if !tools.installed || !tools.connected || tools.update_available {
        return Err("ARK tools must be installed, connected, and match this backend bundle".into());
    }
    report["stage"] = json!("player_map");
    let map: MapSnapshot = read(&client, "read_ark_map", json!({"input":contract.scope()})).await?;
    if map.scope != contract.scope() {
        return Err("player map instance/process/run identity mismatch".into());
    }
    report["players"] = json!(
        map.players
            .iter()
            .map(|player| json!({
                "playerId":player.player_id,"team":player.team,
                "hasPosition":matches!((player.x,player.y,player.z), (Some(x),Some(y),Some(z))
                    if [x,y,z].into_iter().all(coordinate))
            }))
            .collect::<Vec<_>>()
    );
    verify_current_run(&client, contract).await?;
    let Some(mutation) = &contract.mutation else {
        report["outcome"] = json!("read_only_verified");
        return Ok(());
    };
    report["stage"] = json!("mutation_preflight");
    let now = unix_ms()?;
    if map.sampled_at_unix_ms > now || now - map.sampled_at_unix_ms > 10_000 {
        return Err("online player map is stale; no mutation sent".into());
    }
    let owner = select_owner(&map, mutation.player_id)?;
    report["owner"] = json!(owner);
    let intent = json!({"schemaVersion":1,"scope":contract.scope(),
        "requestId":mutation.request_id,"owner":owner,"createdAtUnixMs":now,
        "creature":DODO,"level":37,"tamed":true,
        "state":"reserved_may_be_sent","automaticResendAllowed":false});
    reserve_mutation(directory, contract, mutation, &intent)?;
    report["stage"] = json!("spawn_once");
    report["mutationReserved"] = json!(true);
    // One request only. The backend also checks the current run under its
    // mutation guard and independently inspects the returned entity ID pair.
    let reply: SpawnReply = read(
        &client,
        "spawn_ark_creature",
        json!({"input":{
            "instanceId":contract.instance_id,"processKey":contract.process_key,
            "runId":contract.expected_run_id,"requestId":mutation.request_id,
            "creature":DODO,"level":37,"tamed":true,"playerId":owner.player_id,
            "x":owner.x,"y":owner.y,"z":owner.z
        }}),
    )
    .await?;
    report["stage"] = json!("spawn_readback");
    verify_spawn(&reply, contract, mutation, &owner)?;
    report["creature"] = json!(reply.creature);
    verify_current_run(&client, contract).await?;
    report["outcome"] = json!("tamed_spawn_verified");
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in private contract: reads an exact existing ARK run; explicit mutation mode spawns one level-37 tamed Dodo for a real online player, never resends"]
async fn ark_existing_acceptance() -> Result<(), String> {
    let contract = Contract::load()?;
    let directory = contract.evidence_directory()?;
    let mut report = json!({"schemaVersion":1,"scope":contract.scope(),"mode":contract.mode,
        "backendPid":contract.backend.pid,"backendCreationTime":contract.backend.creation_time,
        "requestId":contract.mutation.as_ref().map(|mutation| &mutation.request_id),
        "startedAtUnixMs":unix_ms()?,"stage":"backend_preflight","passed":false,
        "automaticResendAllowed":false});
    let result = execute(&contract, &directory, &mut report).await;
    report["passed"] = json!(result.is_ok());
    if let Err(reason) = &result {
        report["error"] = json!(reason);
        report["outcome"] = json!(if report["mutationReserved"] == true {
            if reason.starts_with("backend rejected spawn_ark_creature;") {
                "backend_rejected_inspect_request_log"
            } else {
                "mutation_outcome_not_fully_verified"
            }
        } else {
            "not_sent"
        });
    }
    write_new(
        &directory.join(format!("result-{}.json", uuid::Uuid::new_v4().simple())),
        &report,
    )?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract() -> Contract {
        serde_json::from_value(json!({"schemaVersion":1,
            "backend":{"executable":"C:\\private\\backend.exe","pid":123,"creationTime":456},
            "instanceId":"fixture","expectedRunId":7,"evidenceDirectory":"C:\\private\\evidence"
        }))
        .unwrap()
    }

    fn mutation() -> Mutation {
        Mutation {
            request_id: "0123456789abcdef0123456789abcdef".into(),
            player_id: None,
        }
    }

    fn map(players: Value) -> MapSnapshot {
        serde_json::from_value(json!({"instanceId":"fixture","processKey":"main",
            "runId":7,"sampledAtUnixMs":1,"players":players}))
        .unwrap()
    }

    fn player(id: u32) -> Value {
        json!({"playerId":id,"team":500,"x":1.0,"y":2.0,"z":3.0,
            "accountId":"must-not-escape","name":"private-name","accountName":"private-account"})
    }

    #[test]
    fn defaults_are_read_only_and_mutation_needs_explicit_mode() {
        let mut contract = contract();
        assert!(contract.mode == Mode::ReadOnly);
        assert_eq!(contract.process_key, "main");
        assert!(contract.validate().is_ok());
        contract.mutation = Some(mutation());
        assert!(contract.validate().is_err());
        contract.mode = Mode::SpawnTamedDodoOnce;
        assert!(contract.validate().is_ok());
        contract.mutation.as_mut().unwrap().request_id = "../../unsafe".into();
        assert!(contract.validate().is_err());
        contract.mutation = None;
        assert!(contract.validate().is_err());
    }

    #[test]
    fn owner_must_be_online_unambiguous_positioned_and_have_a_team() {
        assert!(select_owner(&map(json!([])), None).is_err());
        assert!(select_owner(&map(json!([player(1), player(2)])), None).is_err());
        assert!(select_owner(&map(json!([player(1)])), Some(2)).is_err());
        assert!(select_owner(&map(json!([player(1), player(1)])), Some(1)).is_err());
        for (field, value) in [
            ("x", Value::Null),
            ("team", json!(0)),
            ("playerId", json!(0)),
            ("z", json!(10_000_001.0)),
        ] {
            let mut invalid = player(1);
            invalid[field] = value;
            assert!(select_owner(&map(json!([invalid])), None).is_err());
        }
        let owner = select_owner(&map(json!([player(1), player(2)])), Some(2)).unwrap();
        let evidence = serde_json::to_value(owner).unwrap();
        assert_eq!(
            evidence,
            json!({"playerId":2,"team":500,"x":1.0,"y":2.0,"z":3.0})
        );
    }

    #[test]
    fn readback_must_match_every_scope_field_request_entity_and_owner_team() {
        let contract = contract();
        let mutation = mutation();
        let owner = select_owner(&map(json!([player(1)])), None).unwrap();
        let valid = json!({"instanceId":"fixture","processKey":"main","runId":7,
            "requestId":mutation.request_id,"creature":{"id1":11,"id2":12,
            "className":"Dodo_Character_BP_C","level":37,"team":500,
            "x":1.0,"y":2.0,"z":3.0,"tamed":true}});
        let reply: SpawnReply = serde_json::from_value(valid.clone()).unwrap();
        assert!(verify_spawn(&reply, &contract, &mutation, &owner).is_ok());
        for (pointer, value) in [
            ("/instanceId", json!("other")),
            ("/processKey", json!("map-other")),
            ("/runId", json!(8)),
            ("/requestId", json!("other")),
            ("/creature/className", json!("Rex_Character_BP_C")),
            ("/creature/level", json!(38)),
            ("/creature/team", json!(501)),
            ("/creature/tamed", json!(false)),
            ("/creature/z", json!(10_000_001.0)),
        ] {
            let mut invalid = valid.clone();
            *invalid.pointer_mut(pointer).unwrap() = value;
            let reply = serde_json::from_value(invalid).unwrap();
            assert!(
                verify_spawn(&reply, &contract, &mutation, &owner).is_err(),
                "{pointer}"
            );
        }
        let mut reply = reply;
        reply.creature.id1 = 0;
        reply.creature.id2 = 0;
        assert!(verify_spawn(&reply, &contract, &mutation, &owner).is_err());
    }

    #[test]
    fn mutation_journal_blocks_existing_unknown_records_and_new_ids_for_same_run() {
        let directory =
            std::env::temp_dir().join(format!("lgsm-ark-once-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let contract = contract();
        let mut mutation = mutation();
        let intent = json!({"state":"reserved_may_be_sent","requestId":mutation.request_id});
        reserve_mutation(&directory, &contract, &mutation, &intent).unwrap();
        assert!(
            reserve_mutation(&directory, &contract, &mutation, &json!({"overwrite":true})).is_err()
        );
        let retained: Value = serde_json::from_slice(
            &fs::read(directory.join(format!("mutation-{}.json", mutation.request_id))).unwrap(),
        )
        .unwrap();
        assert_eq!(retained, intent);
        mutation.request_id = "1123456789abcdef0123456789abcdef".into();
        assert!(reserve_mutation(&directory, &contract, &mutation, &intent).is_err());
        fs::remove_dir_all(&directory).unwrap();
    }
}
