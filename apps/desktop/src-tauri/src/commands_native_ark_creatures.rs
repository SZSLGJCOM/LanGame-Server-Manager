use super::*;
use crate::commands::commands_ark_tools::{self as tools, InstanceInput, SpawnInput};

pub(super) async fn verify<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
) -> Result<(), Box<dyn std::error::Error>> {
    require_fixture(runtime, running)?;
    let status = tools::read_ark_tools_status(
        runtime.state.clone(),
        InstanceInput {
            instance_id: running.summary.id.clone(),
        },
    )
    .await?;
    if !status.connected {
        return Err(format!("Native ARK extension handshake failed: {:?}", status.issue).into());
    }
    let request = |class: &str, level: u32, tamed: bool, player_id: u32| SpawnInput {
        instance_id: running.summary.id.clone(),
        request_id: uuid::Uuid::new_v4().simple().to_string(),
        creature: class.into(),
        level,
        x: 0.0,
        y: 0.0,
        z: 10000.0,
        tamed,
        player_id,
    };
    let class = "/Game/PrimalEarth/Dinos/Dodo/Dodo_Character_BP.Dodo_Character_BP_C";
    let input = request(class, 37, false, 0);
    let id = input.request_id.clone();
    let result = tools::spawn_ark_creature(runtime.state.clone(), input).await?;
    if result.creature.level != 37
        || result.creature.class_name != "Dodo_Character_BP_C"
        || result.creature.tamed
    {
        return Err(
            "Native spawned Dodo does not match the requested class, level and wild state".into(),
        );
    }
    println!(
        "NATIVE_ARK_CREATURE module={} phase=spawn_readback class={} level={} id1={} id2={} x={} y={} z={} evidence=native_entity",
        running.summary.module_id,
        result.creature.class_name,
        result.creature.level,
        result.creature.id1,
        result.creature.id2,
        result.creature.x,
        result.creature.y,
        result.creature.z
    );
    // Exercise production idempotency with the exact same nonce. The second
    // request must inspect the first entity rather than creating a second one.
    let mut duplicate = request(class, 37, false, 0);
    duplicate.request_id = id;
    let again = tools::spawn_ark_creature(runtime.state.clone(), duplicate).await?;
    if (again.creature.id1, again.creature.id2) != (result.creature.id1, result.creature.id2) {
        return Err("Duplicate ARK request generated a different entity".into());
    }
    let invalid = request("/Game/LgsmMissing/Dino.Dino_C", 37, false, 0);
    let invalid_error = tools::spawn_ark_creature(runtime.state.clone(), invalid)
        .await
        .err()
        .ok_or("Native ARK accepted a nonexistent creature class")?;
    if !invalid_error.contains("invalid_dino_class") {
        return Err(
            format!("Unknown class did not receive a native rejection: {invalid_error}").into(),
        );
    }
    let absent_player = request(class, 37, true, u32::MAX);
    let owner_error = tools::spawn_ark_creature(runtime.state.clone(), absent_player)
        .await
        .err()
        .ok_or("Native ARK assigned a tamed creature to a nonexistent online player")?;
    if !owner_error.contains("owner_not_online") {
        return Err(
            format!("Missing owner did not receive a native rejection: {owner_error}").into(),
        );
    }
    let rex = tools::spawn_ark_creature(
        runtime.state.clone(),
        request(
            "/Game/PrimalEarth/Dinos/Rex/Rex_Character_BP.Rex_Character_BP_C",
            150,
            false,
            0,
        ),
    )
    .await?;
    println!(
        "NATIVE_ARK_CREATURE module={} phase=default_creature class={} level={} id1={} id2={} evidence=native_entity",
        running.summary.module_id,
        rex.creature.class_name,
        rex.creature.level,
        rex.creature.id1,
        rex.creature.id2
    );
    println!(
        "NATIVE_ARK_CREATURE module={} phase=complete duplicate=one_entity unknown_class=rejected missing_owner=rejected tamed_live_player=not_available",
        running.summary.module_id
    );
    Ok(())
}
