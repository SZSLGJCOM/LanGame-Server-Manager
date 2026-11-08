//! Opt-in native entity persistence: never replay Spawn after the server restarts.
use super::*;
use crate::commands::commands_ark_tools::{self as tools, protocol::Creature};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Deserialize, Serialize)]
struct Receipt {
    instance_id: String,
    run_id: i64,
    creatures: Vec<Creature>,
    world: WorldFile,
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
struct WorldFile {
    relative: PathBuf,
    bytes: u64,
    sha256: String,
}

fn enabled() -> bool {
    std::env::var("LANGAME_NATIVE_ARK_PERSISTENCE").as_deref() == Ok("true")
}

fn run_id(running: &InstanceDetails) -> Result<i64, Box<dyn std::error::Error>> {
    running
        .active_run
        .as_ref()
        .map(|run| run.run_id)
        .ok_or_else(|| "ARK persistence has no active managed run".into())
}

fn world_file(running: &InstanceDetails) -> Result<Option<WorldFile>, Box<dyn std::error::Error>> {
    let root = Path::new(&running.saves_path);
    let worlds = save_inventory(root)?
        .into_iter()
        .filter(|(path, (bytes, _))| {
            path.extension().is_some_and(|value| value == "ark") && *bytes > 0
        })
        .collect::<Vec<_>>();
    if worlds.len() > 1 {
        return Err("ARK persistence requires exactly one isolated primary world".into());
    }
    let Some((relative, (bytes, _))) = worlds.into_iter().next() else {
        return Ok(None);
    };
    if bytes > 256 * 1024 * 1024 {
        return Err("Fresh ARK persistence world exceeds its fixture size budget".into());
    }
    let contents = fs::read(root.join(&relative))?;
    Ok(Some(WorldFile {
        relative,
        bytes: contents.len() as u64,
        sha256: Sha256::digest(contents)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    }))
}

async fn inspect(
    running: &InstanceDetails,
    expected: &Creature,
) -> Result<Creature, Box<dyn std::error::Error>> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let response = tools::dispatch(
        running,
        &format!(
            "LgsmArkTools.Inspect {id} {} {}",
            expected.id1, expected.id2
        ),
    )
    .await?;
    let found = tools::protocol::creature(&response, &running.summary.module_id, "inspect", &id)?;
    if (
        found.id1,
        found.id2,
        &found.class_name,
        found.level,
        found.tamed,
        found.team,
    ) != (
        expected.id1,
        expected.id2,
        &expected.class_name,
        expected.level,
        expected.tamed,
        expected.team,
    ) {
        return Err("ARK persisted entity identity, class, level or ownership changed".into());
    }
    Ok(found)
}

pub(super) async fn save<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    creatures: Vec<Creature>,
) -> Result<(), Box<dyn std::error::Error>> {
    if !enabled() {
        return Ok(());
    }
    require_fixture(runtime, running)?;
    tools::ensure_endpoint(running).await?;
    if running.summary.module_id != "arksurvivalascended" || creatures.len() != 3 {
        return Err("ARK persistence requires the three ASA native creature probes".into());
    }
    for creature in &creatures {
        inspect(running, creature).await?;
    }
    let before = world_file(running)?;
    let action = runtime
        .descriptor
        .runtime
        .player_actions
        .iter()
        .find(|action| action.id == "save_world")
        .ok_or("ARK persistence requires the declared save_world action")?;
    let result = crate::commands::send_instance_runtime_command(
        runtime.state.clone(),
        serde_json::from_value(json!({
            "instanceId": running.summary.id,
            "command": action.command_template,
            "runtimeActionId": "save_world",
        }))?,
    )
    .await?;
    let deadline = Instant::now() + Duration::from_secs(90);
    let world = loop {
        let current = world_file(running)?;
        if current.is_some() && current != before {
            break current.unwrap();
        }
        if Instant::now() >= deadline {
            return Err("ARK save request did not change its primary .ark world".into());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    };
    let receipt = Receipt {
        instance_id: running.summary.id.clone(),
        run_id: run_id(running)?,
        creatures,
        world,
    };
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(runtime.package.root.join("ark-creature-persistence.json"))?;
    serde_json::to_writer_pretty(file, &receipt)?;
    println!(
        "NATIVE_ARK_PERSISTENCE module={} phase=primary_world_written creatures={} bytes={} sha256={} save_receipt={:?} content_readback=pending_restart",
        running.summary.module_id,
        receipt.creatures.len(),
        receipt.world.bytes,
        receipt.world.sha256,
        result.save_completion
    );
    Ok(())
}

pub(super) async fn verify_restart<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    cycle: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if !enabled() || cycle != "restart" {
        return Ok(());
    }
    require_fixture(runtime, running)?;
    tools::ensure_endpoint(running).await?;
    let path = runtime.package.root.join("ark-creature-persistence.json");
    if fs::metadata(&path)?.len() > 16 * 1024 {
        return Err("ARK persistence receipt exceeds its fixture budget".into());
    }
    let receipt: Receipt = serde_json::from_slice(&fs::read(path)?)?;
    if receipt.instance_id != running.summary.id
        || receipt.run_id == run_id(running)?
        || receipt.creatures.len() != 3
        || world_file(running)?.is_none()
    {
        return Err("ARK persistence did not reload the same instance in a fresh run".into());
    }
    // A fresh Inspect nonce reads the original native ID. Spawn's process-local
    // receipt table is intentionally neither replayed nor treated as durability.
    for creature in receipt.creatures {
        let found = inspect(running, &creature).await?;
        println!(
            "NATIVE_ARK_PERSISTENCE module={} phase=restart_readback class={} level={} tamed={} team={} id1={} id2={} evidence=native_entity spawn_replayed=false",
            running.summary.module_id,
            found.class_name,
            found.level,
            found.tamed,
            found.team,
            found.id1,
            found.id2
        );
    }
    Ok(())
}
