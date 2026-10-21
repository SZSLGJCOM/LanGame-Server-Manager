use super::map_save_checks::write_evidence as write_fresh;
use super::*;
use crate::commands::commands_dst_map::{DstMapInput, DstMapSnapshot, read_dontstarve_world_map};

type ProbeResult<T> = Result<T, Box<dyn std::error::Error>>;

struct Phase<'a> {
    index: usize,
    process: &'a StartedProcess,
    map: DstMapSnapshot,
    native_shard_id: String,
}

fn timeout_seconds(name: &str, default: u64, maximum: u64) -> ProbeResult<u64> {
    let seconds = std::env::var(name)
        .unwrap_or_else(|_| default.to_string())
        .parse()?;
    if !(30..=maximum).contains(&seconds) {
        return Err(format!("{name} must be 30..{maximum} seconds").into());
    }
    Ok(seconds)
}

async fn read_map(
    state: tauri::State<'_, DesktopState>,
    id: &str,
    started: &StartInstanceResult,
    phase: &Phase<'_>,
    terrain: bool,
) -> ProbeResult<DstMapSnapshot> {
    let map = command_result(
        read_dontstarve_world_map(
            state,
            DstMapInput {
                instance_id: id.into(),
                shard: phase.process.process_key.clone(),
                include_terrain: terrain,
            },
        )
        .await,
    )
    .map_err(|error| {
        format!(
            "Player phase {} {} map: {error}",
            phase.index, phase.process.process_key
        )
    })?;
    if map.instance_id != id
        || map.run_id != started.run_id
        || map.shard != phase.process.process_key
        || map.session != phase.map.session
        || (map.width, map.height, map.tile_scale)
            != (phase.map.width, phase.map.height, phase.map.tile_scale)
        || (!terrain
            && (map.tiles.is_some()
                || map.landmarks.is_some()
                || map.roads.is_some()
                || map.art.is_some()))
    {
        return Err(format!(
            "Player phase {} {} identity, geometry or position-only contract changed",
            phase.index, phase.process.process_key
        )
        .into());
    }
    Ok(map)
}

fn publish_phase(
    root: &Path,
    id: &str,
    started: &StartInstanceResult,
    phase: &Phase<'_>,
    action: &str,
    user_id: Option<&str>,
    seconds: u64,
) -> ProbeResult<()> {
    let index = match action {
        "walk" => phase.index,
        "disconnect" => phase.index + 1,
        "complete" => phase.index + 2,
        _ => return Err("Invalid native player phase action".into()),
    };
    write_fresh(
        root,
        &format!("native-player-phase-{index}-{action}.json"),
        &json!({
            "schema":"lgsm-dst-native-player-phase-v1", "phaseId":uuid::Uuid::new_v4().to_string(),
            "action":action, "index":index, "instanceId":id, "runId":started.run_id,
            "shard":phase.process.process_key, "expectedSession":phase.map.session,
            "nativeShardId":phase.native_shard_id, "expectedUserId":user_id,
            "width":phase.map.width, "height":phase.map.height, "tileScale":phase.map.tile_scale,
            "timeoutSeconds":seconds, "minimumDistanceMetres":2,
        }),
    )?;
    println!(
        "NATIVE_PLAYER_PHASE index={index}; action={action}; shard={}; session={}",
        phase.process.process_key, phase.map.session
    );
    Ok(())
}

async fn local_counts(
    state: &DesktopState,
    id: &str,
    started: &StartInstanceResult,
    phase: &Phase<'_>,
) -> ProbeResult<(usize, usize)> {
    let raw = native_probe(
        state,
        id,
        phase.process,
        started.run_id,
        "",
        "tostring(#AllPlayers)..' '..tostring(#GetPlayerClientTable())",
    )
    .await?;
    let counts = raw
        .split_whitespace()
        .map(str::parse::<usize>)
        .collect::<Result<Vec<_>, _>>()?;
    if counts.len() != 2 {
        return Err(format!(
            "Invalid native presence counts for {}",
            phase.process.process_key
        )
        .into());
    }
    Ok((counts[0], counts[1]))
}

async fn verify_native_coordinates(
    state: &DesktopState,
    id: &str,
    started: &StartInstanceResult,
    phase: &Phase<'_>,
    map: &DstMapSnapshot,
    user_id: &str,
) -> ProbeResult<Value> {
    let user = serde_json::to_string(user_id)?;
    let prelude = format!(
        "local p;for _,v in ipairs(AllPlayers) do if v.userid=={user} then assert(not p);p=v end end;assert(p and p.Transform);local connected=false;for _,v in ipairs(GetPlayerClientTable()) do if v.userid=={user} then connected=true end end;assert(connected);local x,_,z=p.Transform:GetWorldPosition();local tx,tz=TheWorld.Map:GetTileCoordsAtPoint(x,0,z);local cx,_,cz=TheWorld.Map:GetTileCenterPoint(x,0,z);"
    );
    let raw = native_probe(state, id, phase.process, started.run_id, &prelude,
        "require('json').encode_compliant({x=x,z=z,mapX=tx+.5+(x-cx)/TILE_SCALE,mapZ=tz+.5+(z-cz)/TILE_SCALE,session=TheNet:GetSessionIdentifier()})").await?;
    let native: Value = serde_json::from_str(&raw)?;
    let player = map
        .players
        .iter()
        .find(|player| player.id == user_id)
        .ok_or("Missing observed player")?;
    let number = |key: &str| {
        native[key]
            .as_f64()
            .filter(|value| value.is_finite())
            .ok_or("Invalid native coordinate")
    };
    // The native sample is asynchronous. Comparing the affine offset, rather
    // than equal world positions, permits real movement between both queries.
    if native["session"] != phase.map.session
        || (player.map_x - number("mapX")? - (player.x - number("x")?) / map.tile_scale).abs()
            > 1e-5
        || (player.map_z - number("mapZ")? - (player.z - number("z")?) / map.tile_scale).abs()
            > 1e-5
    {
        return Err(format!(
            "Native fractional coordinate mismatch on {}",
            phase.process.process_key
        )
        .into());
    }
    Ok(native)
}

async fn wait_for_movement(
    state: tauri::State<'_, DesktopState>,
    id: &str,
    started: &StartInstanceResult,
    root: &Path,
    phase: &Phase<'_>,
    user_id: Option<&str>,
    seconds: u64,
) -> ProbeResult<String> {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut first = None;
    let mut samples = Vec::new();
    let mut native_samples = Vec::new();
    loop {
        let map = read_map(state.clone(), id, started, phase, false).await?;
        if map.players.len() > 1 {
            return Err(format!(
                "Unexpected additional client on {}",
                phase.process.process_key
            )
            .into());
        }
        if let Some(player) = map.players.first() {
            if user_id.is_some_and(|expected| expected != player.id) {
                return Err(format!(
                    "Real client identity changed on {}",
                    phase.process.process_key
                )
                .into());
            }
            let initial = first.get_or_insert_with(|| player.clone());
            if player.id != initial.id {
                return Err("Real client identity changed during movement".into());
            }
            let moved = (player.x - initial.x).hypot(player.z - initial.z) >= 2.0;
            if samples.is_empty() {
                native_samples.push(
                    verify_native_coordinates(&state, id, started, phase, &map, &player.id).await?,
                );
                // The first position is observed before authorizing client
                // walking, so fast arrival cannot hide the entire movement.
                publish_phase(root, id, started, phase, "walk", user_id, seconds)?;
            }
            samples.push(map.clone());
            if moved && samples.len() >= 2 {
                let rendered = read_map(state.clone(), id, started, phase, true).await?;
                if !rendered.players.iter().any(|p| p.id == player.id) {
                    return Err(format!(
                        "Connected full map lost client on {}",
                        phase.process.process_key
                    )
                    .into());
                }
                native_samples.push(
                    verify_native_coordinates(&state, id, started, phase, &rendered, &player.id)
                        .await?,
                );
                write_fresh(
                    root,
                    &format!("native-player-movement-{}.json", phase.process.process_key),
                    &samples,
                )?;
                write_fresh(
                    root,
                    &format!(
                        "native-player-coordinates-{}.json",
                        phase.process.process_key
                    ),
                    &native_samples,
                )?;
                write_fresh(
                    root,
                    &format!("world-map-connected-{}.json", phase.process.process_key),
                    &rendered,
                )?;
                println!(
                    "NATIVE_PLAYER_MOVEMENT shard={}; connected_client=true; distance_over_2_metres=true; samples={}",
                    phase.process.process_key,
                    samples.len()
                );
                return Ok(player.id.clone());
            }
        }
        if Instant::now() >= deadline {
            write_fresh(
                root,
                &format!(
                    "native-player-movement-{}-timeout.json",
                    phase.process.process_key
                ),
                &samples,
            )?;
            return Err(format!("Player phase {} {} session {}: no same-user movement >=2m before {seconds}s deadline; samples={}", phase.index, phase.process.process_key, phase.map.session, samples.len()).into());
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn migrate(
    state: tauri::State<'_, DesktopState>,
    id: &str,
    started: &StartInstanceResult,
    root: &Path,
    route: (&Phase<'_>, &Phase<'_>),
    user_id: &str,
    seconds: u64,
) -> ProbeResult<()> {
    let (source, target) = route;
    let user = serde_json::to_string(user_id)?;
    let target_id = serde_json::to_string(&target.native_shard_id)?;
    let source_session = serde_json::to_string(&source.map.session)?;
    // Native 756039 consolecommands.lua:c_migrateto uses this same event;
    // playerspawner.lua persists/removes the real player then StartMigration.
    // Never manufacture an entity or mutate Transform to satisfy movement.
    let prelude = format!(
        "assert(TheNet:GetSessionIdentifier()=={source_session});assert(Shard_IsWorldAvailable({target_id}));local p;for _,v in ipairs(AllPlayers) do if v.userid=={user} then assert(not p);p=v end end;assert(p and not p._despawning);TheWorld:PushEvent('ms_playerdespawnandmigrate',{{player=p,portalid=1,worldid={target_id}}});"
    );
    native_probe(
        &state,
        id,
        source.process,
        started.run_id,
        &prelude,
        "'native_migration_requested'",
    )
    .await?;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        let map = read_map(state.clone(), id, started, source, false).await?;
        let (local_players, clients) = local_counts(&state, id, started, source).await?;
        if map.players.is_empty() && local_players == 0 {
            write_fresh(
                root,
                &format!(
                    "native-player-migration-{}-to-{}.json",
                    source.process.process_key, target.process.process_key
                ),
                &json!({"source":source.process.process_key,"target":target.process.process_key,
                    "userId":user_id,"sourcePositions":map,"sourceLocalPlayers":local_players,
                    "sourceClientTableCount":clients,"targetNativeShardId":target.native_shard_id}),
            )?;
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "Migration {} -> {}: source retained local player after {seconds}s",
                source.process.process_key, target.process.process_key
            )
            .into());
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn wait_for_disconnect(
    state: tauri::State<'_, DesktopState>,
    id: &str,
    started: &StartInstanceResult,
    root: &Path,
    phases: &[Phase<'_>],
    seconds: u64,
) -> ProbeResult<()> {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        let mut observations = Vec::new();
        for phase in phases {
            let map = read_map(state.clone(), id, started, phase, false).await?;
            let (local_players, clients) = local_counts(&state, id, started, phase).await?;
            observations.push((phase, map, local_players, clients));
        }
        if observations.iter().all(|(_, map, players, clients)| {
            map.players.is_empty() && *players == 0 && *clients == 0
        }) {
            for (phase, map, players, clients) in observations {
                write_fresh(
                    root,
                    &format!("world-map-disconnected-{}.json", phase.process.process_key),
                    &map,
                )?;
                write_fresh(
                    root,
                    &format!("native-player-presence-{}.json", phase.process.process_key),
                    &json!({"shard":phase.process.process_key,"session":phase.map.session,
                        "runId":started.run_id,"mapPlayers":map.players.len(),"localPlayers":players,"nativeClients":clients}),
                )?;
            }
            println!(
                "NATIVE_PLAYER_DISCONNECTED all_{}_shards_empty=true; native_clients=0",
                phases.len()
            );
            return Ok(());
        }
        if Instant::now() >= deadline {
            let counts: Vec<_> = observations.iter().map(|(phase,map,players,clients)| json!({
                "shard":phase.process.process_key,"mapPlayers":map.players.len(),"localPlayers":players,"nativeClients":clients})).collect();
            write_fresh(root, "native-player-disconnect-timeout.json", &counts)?;
            return Err(format!(
                "Real client did not disconnect from all shards before {seconds}s: {counts:?}"
            )
            .into());
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

pub(super) async fn verify_all(
    state: tauri::State<'_, DesktopState>,
    id: &str,
    started: &StartInstanceResult,
    root: &Path,
) -> ProbeResult<()> {
    let seconds = timeout_seconds("LANGAME_DST_SMOKE_PLAYER_TIMEOUT_SECONDS", 300, 900)?;
    let disconnect_seconds =
        timeout_seconds("LANGAME_DST_SMOKE_DISCONNECT_TIMEOUT_SECONDS", 90, 180)?;
    if started.processes.first().map(|p| p.process_key.as_str()) != Some("master")
        || started.processes.len() > 4
    {
        return Err("Expected 1..4 actually started DST shards with Master first".into());
    }
    let mut phases = Vec::new();
    for (index, process) in started.processes.iter().enumerate() {
        if !app_core::dst_shards::DST_SHARDS
            .iter()
            .any(|spec| spec.process_key == process.process_key)
            || phases
                .iter()
                .any(|p: &Phase<'_>| p.process.process_key == process.process_key)
        {
            return Err("Unexpected or duplicate started shard".into());
        }
        let map: DstMapSnapshot = serde_json::from_reader(fs::File::open(
            root.join(format!("world-map-0-{}.json", process.process_key)),
        )?)?;
        if map.instance_id != id
            || map.shard != process.process_key
            || map.run_id != started.run_id
            || map.session.is_empty()
        {
            return Err(format!("Initial map identity differs for {}", process.process_key).into());
        }
        let native_shard_id = native_probe(
            &state,
            id,
            process,
            started.run_id,
            "",
            "tostring(TheShard:GetShardId())",
        )
        .await?;
        if native_shard_id.is_empty()
            || native_shard_id.len() > 128
            || phases.iter().any(|p| p.native_shard_id == native_shard_id)
        {
            return Err("Invalid or duplicate actual native shard ID".into());
        }
        phases.push(Phase {
            index,
            process,
            map,
            native_shard_id,
        });
    }
    let plan: Vec<_> = phases
        .iter()
        .map(|p| {
            json!({"index":p.index,"shard":p.process.process_key,
        "expectedSession":p.map.session,"nativeShardId":p.native_shard_id,"width":p.map.width,
        "height":p.map.height,"tileScale":p.map.tile_scale,
        "phaseFile":format!("native-player-phase-{}-walk.json",p.index)})
        })
        .collect();
    write_fresh(
        root,
        "native-player-plan.json",
        &json!({"schema":"lgsm-dst-native-player-plan-v1",
        "instanceId":id,"runId":started.run_id,"phases":plan}),
    )?;
    let mut user_id: Option<String> = None;
    for (index, phase) in phases.iter().enumerate() {
        if index > 0 {
            migrate(
                state.clone(),
                id,
                started,
                root,
                (&phases[index - 1], phase),
                user_id.as_deref().ok_or("Missing initial real client")?,
                seconds,
            )
            .await?;
        }
        user_id = Some(
            wait_for_movement(
                state.clone(),
                id,
                started,
                root,
                phase,
                user_id.as_deref(),
                seconds,
            )
            .await?,
        );
    }
    let last = phases.last().ok_or("Missing final player phase")?;
    publish_phase(
        root,
        id,
        started,
        last,
        "disconnect",
        user_id.as_deref(),
        disconnect_seconds,
    )?;
    wait_for_disconnect(state, id, started, root, &phases, disconnect_seconds).await?;
    publish_phase(
        root,
        id,
        started,
        last,
        "complete",
        user_id.as_deref(),
        disconnect_seconds,
    )?;
    Ok(())
}
