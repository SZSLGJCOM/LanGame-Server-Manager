use super::*;
use crate::commands::commands_dst_map::{DstMapInput, read_dontstarve_world_map};
use std::io::Write;

pub(super) fn write_evidence(
    root: &Path,
    name: &str,
    value: &impl serde::Serialize,
) -> Result<(), Box<dyn std::error::Error>> {
    // Complete immutable JSON is linked into place without replacing existing
    // evidence. The staging file shares the destination's filesystem.
    let staged = root.join(format!(
        ".native-player-{}.json",
        uuid::Uuid::new_v4().simple()
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged)?;
    serde_json::to_writer(&mut file, value)?;
    file.flush()?;
    drop(file);
    let published = fs::hard_link(&staged, root.join(name));
    let cleanup = fs::remove_file(&staged);
    published?;
    cleanup?;
    Ok(())
}

#[test]
fn native_player_phase_publication_preserves_existing_evidence() {
    let root = std::env::temp_dir().join(format!("dst-player-phase-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let original = json!({"phaseId":"original","index":0});
    write_evidence(&root, "phase.json", &original).unwrap();
    let bytes = fs::read(root.join("phase.json")).unwrap();
    assert!(write_evidence(&root, "phase.json", &json!({"phaseId":"replacement"})).is_err());
    assert_eq!(fs::read(root.join("phase.json")).unwrap(), bytes);
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), original);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    fs::remove_file(root.join("phase.json")).unwrap();
    fs::remove_dir(root).unwrap();
}

pub(super) async fn verify_connected_player(
    state: tauri::State<'_, DesktopState>,
    id: &str,
    started: &StartInstanceResult,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    match std::env::var("LANGAME_DST_SMOKE_PLAYER_SHARDS").as_deref() {
        Ok("all") => return super::player_checks::verify_all(state, id, started, root).await,
        Ok("master") | Err(std::env::VarError::NotPresent) => {}
        _ => return Err("LANGAME_DST_SMOKE_PLAYER_SHARDS must be master or all".into()),
    }
    let Ok(timeout) = std::env::var("LANGAME_DST_SMOKE_PLAYER_TIMEOUT_SECONDS") else {
        return Ok(());
    };
    let seconds = timeout.parse::<u64>()?;
    if !(30..=900).contains(&seconds) {
        return Err("Player verification timeout must be 30..900 seconds".into());
    }
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut first = None;
    let mut samples = Vec::new();
    println!(
        "NATIVE_PLAYER_WAIT root={}; run={}; instance={id}",
        root.display(),
        started.run_id
    );
    loop {
        let map = command_result(
            read_dontstarve_world_map(
                state.clone(),
                DstMapInput {
                    instance_id: id.into(),
                    shard: "master".into(),
                    include_terrain: false,
                },
            )
            .await,
        )?;
        assert_eq!(map.run_id, started.run_id);
        if let Some(player) = map.players.first() {
            // A real native client must connect and move. This harness never
            // spawns or teleports a stand-in entity to satisfy the assertion.
            let initial = first.get_or_insert_with(|| player.clone());
            let moved = player.id == initial.id
                && (player.x - initial.x).hypot(player.z - initial.z) >= 2.0;
            samples.push(map.clone());
            if moved {
                let rendered = command_result(
                    read_dontstarve_world_map(
                        state.clone(),
                        DstMapInput {
                            instance_id: id.into(),
                            shard: "master".into(),
                            include_terrain: true,
                        },
                    )
                    .await,
                )?;
                fs::write(
                    root.join("world-map-connected-master.json"),
                    serde_json::to_vec(&rendered)?,
                )?;
                fs::write(
                    root.join("native-player-movement.json"),
                    serde_json::to_vec(&samples)?,
                )?;
                println!(
                    "NATIVE_PLAYER_MOVEMENT connected_client=true; distance_over_2_metres=true; samples={}",
                    samples.len()
                );
                verify_disconnected_player(state.clone(), id, started, root).await?;
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err("No connected native player movement before deadline".into());
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn verify_disconnected_player(
    state: tauri::State<'_, DesktopState>,
    id: &str,
    started: &StartInstanceResult,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let Ok(timeout) = std::env::var("LANGAME_DST_SMOKE_DISCONNECT_TIMEOUT_SECONDS") else {
        return Ok(());
    };
    let seconds = timeout.parse::<u64>()?;
    if !(30..=180).contains(&seconds) {
        return Err("Player disconnection timeout must be 30..180 seconds".into());
    }
    println!("NATIVE_PLAYER_DISCONNECT_WAIT root={}", root.display());
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        let snapshot = command_result(
            read_dontstarve_world_map(
                state.clone(),
                DstMapInput {
                    instance_id: id.into(),
                    shard: "master".into(),
                    include_terrain: false,
                },
            )
            .await,
        )?;
        if snapshot.players.is_empty() {
            let master = started
                .processes
                .iter()
                .find(|process| process.process_key == "master")
                .ok_or("Missing native Master process")?;
            let counts = native_probe(
                &state,
                id,
                master,
                started.run_id,
                "",
                "tostring(#AllPlayers)..' '..tostring(#GetPlayerClientTable())",
            )
            .await?;
            let values = counts
                .split_whitespace()
                .map(str::parse::<usize>)
                .collect::<Result<Vec<_>, _>>()?;
            if values.len() != 2 {
                return Err("Invalid native player presence counts".into());
            }
            if values[1] == 0 {
                fs::write(root.join("native-player-presence-counts.txt"), counts)?;
                fs::write(
                    root.join("world-map-disconnected-master.json"),
                    serde_json::to_vec(&snapshot)?,
                )?;
                println!("NATIVE_PLAYER_DISCONNECTED online_map_players=0; native_clients=0");
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err(
                "Connected native player did not leave before the disconnection deadline".into(),
            );
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

pub(super) async fn verify(
    state: tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    id: &str,
    started: &StartInstanceResult,
    root: &Path,
    generation: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    for process in &started.processes {
        let materials = native_probe(
            &state, id, process, started.run_id,
            "local o=rawget(_G,'__lgsm_world_map_observer');local r,e;if o and o.render and TheWorld.minimap then local w,h=TheWorld.Map:GetSize();r,e=o.render:report(TheWorld.minimap.MiniMap,{gameVersion=APP_VERSION,session=TheNet:GetSessionIdentifier(),width=w,height=h,mods={}}) end;",
            "r and (tostring(#r.layers)..' '..tostring(#r.atlases)) or ('unavailable '..tostring(e))",
        ).await?;
        let counts = materials
            .split_whitespace()
            .map(str::parse::<usize>)
            .collect::<Result<Vec<_>, _>>()?;
        assert!(
            counts.len() == 2 && counts[0] > 0 && counts[1] > 0,
            "Automatic native MiniMap material observation: {materials}"
        );
        fs::write(
            root.join(format!(
                "native-materials-{generation}-{}.txt",
                process.process_key
            )),
            &materials,
        )?;
        println!(
            "NATIVE_AUTOMATIC_MATERIALS shard={}; layers={}; atlases={}; client_import=false",
            process.process_key, counts[0], counts[1]
        );
        let map = command_result(
            read_dontstarve_world_map(
                state.clone(),
                DstMapInput {
                    instance_id: id.into(),
                    shard: process.process_key.clone(),
                    include_terrain: true,
                },
            )
            .await,
        )?;
        assert_eq!(map.run_id, started.run_id);
        let tiles = map.tiles.as_ref().expect("native terrain");
        assert_eq!(tiles.len(), map.width * map.height);
        // Retain the real response even if a Mod-content assertion fails.
        fs::write(
            root.join(format!(
                "world-map-{generation}-{}.json",
                process.process_key
            )),
            serde_json::to_vec(&map)?,
        )?;
        if generation == 1
            && std::env::var("LANGAME_DST_SMOKE_PLAYER_SHARDS").as_deref() == Ok("all")
        {
            verify_reload_against_pre_stop(root, &map)?;
        }
        let art = map.art.as_ref().expect("installed native map artwork");
        assert!(
            art.issues
                .iter()
                .all(|issue| !issue.contains("Native map material observation is unavailable")),
            "The product query must use automatic material observation: {:?}",
            art.issues
        );
        if process.process_key == "master" {
            // Independent acceptance markers come from the downloaded Mods'
            // required rooms and prefab MiniMapEntity calls, not our renderer.
            let scenario = std::env::var("LANGAME_DST_SMOKE_SCENARIO").unwrap_or_default();
            if matches!(
                scenario.as_str(),
                "cherry_forest" | "tropical_cherry" | "adventures_cherry"
            ) {
                for name in ["CHERRY2", "CHERRY3", "CHERRY4"] {
                    assert!(
                        map.tile_names.values().any(|value| value == name),
                        "Missing generated {name}"
                    );
                }
                let statue = map
                    .landmarks
                    .as_ref()
                    .unwrap()
                    .iter()
                    .find(|item| item.prefab == "cherry_foreststatue")
                    .expect("Cherry Forest's required native statue");
                assert_eq!(statue.icon, "cherry_stonetree.png");
                assert!(
                    art.icons.contains_key(&statue.icon),
                    "Cherry statue artwork: {:?}",
                    art.issues
                );
            }
            if matches!(
                scenario.as_str(),
                "tropical" | "tropical_cherry" | "tropical_adventures" | "adventures_cherry"
            ) {
                assert!(
                    map.tile_names.values().any(|value| value == "BEACH"),
                    "Missing generated tropical beach"
                );
                let machine = map
                    .landmarks
                    .as_ref()
                    .unwrap()
                    .iter()
                    .find(|item| item.prefab == "slotmachine")
                    .expect("Tropical world's required native casino");
                let expected =
                    if scenario.starts_with("adventures") || scenario == "tropical_adventures" {
                        "slot_machine.tex"
                    } else {
                        "slot_machine.png"
                    };
                assert_eq!(machine.icon, expected);
                assert!(
                    art.icons.contains_key(&machine.icon),
                    "Casino artwork: {:?}",
                    art.issues
                );
            }
        }
        assert!(
            art.textures.len() > 1,
            "native terrain textures: {:?}",
            art.issues
        );
        if process.process_key == "master" {
            assert!(
                !map.landmarks.as_ref().unwrap().is_empty(),
                "native portal and landmarks must be collected"
            );
        }
        let expected = native_probe(
            &state,
            id,
            process,
            started.run_id,
            "local w,h=TheWorld.Map:GetSize();",
            "tostring(w)..','..tostring(h)..','..tostring(TheWorld.Map:GetTile(0,0))",
        )
        .await?;
        assert_eq!(
            expected,
            format!("{},{},{}", map.width, map.height, tiles[0])
        );
        let positions = command_result(
            read_dontstarve_world_map(
                state.clone(),
                DstMapInput {
                    instance_id: id.into(),
                    shard: process.process_key.clone(),
                    include_terrain: false,
                },
            )
            .await,
        )?;
        assert_eq!(positions.session, map.session);
        assert_eq!((positions.width, positions.height), (map.width, map.height));
        assert!(positions.tiles.is_none());
        println!(
            "NATIVE_MAP shard={}; dimensions={}x{}; terrain_types={}; players={}; textures={}; icons={}; landmarks={}; roads={}",
            process.process_key,
            map.width,
            map.height,
            map.tile_names.len(),
            positions.players.len(),
            art.textures.len(),
            art.icons.len(),
            map.landmarks.as_ref().unwrap().len(),
            map.roads.as_ref().unwrap().len()
        );
    }
    save_started_world(state, storage, id, started).await
}

pub(super) async fn verify_pre_stop_if_all_shards(
    state: tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    id: &str,
    started: &StartInstanceResult,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("LANGAME_DST_SMOKE_PLAYER_SHARDS").as_deref() != Ok("all") {
        return Ok(());
    }
    // Gameplay can change tiles. Retain the final same-run full observation
    // immediately before requesting the native cluster save, rather than initial terrain.
    let mut captures = Vec::new();
    for process in &started.processes {
        let map = command_result(
            read_dontstarve_world_map(
                state.clone(),
                DstMapInput {
                    instance_id: id.into(),
                    shard: process.process_key.clone(),
                    include_terrain: true,
                },
            )
            .await,
        )?;
        assert_eq!(map.run_id, started.run_id);
        write_evidence(
            root,
            &format!("world-map-prestop-{}.json", process.process_key),
            &map,
        )?;
        captures.push(json!({"shard":process.process_key,"session":map.session,
            "capturedAtUnixMs":map.captured_at_unix_ms}));
    }
    let request_started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis();
    save_started_world(state, storage, id, started).await?;
    let request_submitted = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis();
    write_evidence(
        root,
        "native-prestop-save.json",
        &json!({"instanceId":id,"runId":started.run_id,
        "captures":captures,"nativeRequestStartedAtUnixMs":request_started,
        "nativeRequestSubmittedAtUnixMs":request_submitted,"nativeRequestSubmitted":true}),
    )
}

fn verify_reload_against_pre_stop(
    root: &Path,
    map: &crate::commands::commands_dst_map::DstMapSnapshot,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = root.join(format!("world-map-prestop-{}.json", map.shard));
    let before: crate::commands::commands_dst_map::DstMapSnapshot =
        serde_json::from_reader(fs::File::open(&path)?)?;
    let same_geometry =
        (before.width, before.height, before.tile_scale) == (map.width, map.height, map.tile_scale);
    let changed_cells = before
        .tiles
        .as_ref()
        .zip(map.tiles.as_ref())
        .filter(|(left, right)| {
            same_geometry && left.len() == right.len() && left.len() == map.width * map.height
        })
        .map(|(left, right)| left.iter().zip(right).filter(|(a, b)| a != b).count());
    let checks = json!({"sameInstance":before.instance_id == map.instance_id,"sameShard":before.shard == map.shard,
        "sameSession":before.session == map.session,"sameGeometry":same_geometry,
        "laterRunId":map.run_id > before.run_id,"laterCapture":map.captured_at_unix_ms > before.captured_at_unix_ms,
        "sameNativeTiles":changed_cells == Some(0)});
    let passed = checks
        .as_object()
        .ok_or("Invalid persistence checks")?
        .values()
        .all(|value| value == &Value::Bool(true));
    write_evidence(
        root,
        &format!("native-prestop-reload-{}.json", map.shard),
        &json!({
        "baseline":path,"shard":map.shard,"checks":checks,"changedCells":changed_cells,"passed":passed}),
    )?;
    if !passed {
        return Err(format!("Pre-stop/reload mismatch on {}: changedCells={changed_cells:?}; inspect retained full snapshots and capture/save interval",map.shard).into());
    }
    Ok(())
}

async fn save_started_world(
    state: tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    id: &str,
    started: &StartInstanceResult,
) -> Result<(), Box<dyn std::error::Error>> {
    let details = read_instance_details(&storage.paths, id).await?;
    let receipt = command_result(
        crate::commands::commands_runtime_lifecycle::dst_lifecycle::immediate_save::save_world(
            &state, storage, &details, 1,
        )
        .await,
    )?;
    assert!(
        !receipt.write_confirmation_pending,
        "Native save request delivery is still pending; no submission evidence can be recorded."
    );
    assert_eq!(
        receipt.save_completion,
        Some(WorldSaveCompletion::Requested),
        "Native save request did not return a submitted-request receipt."
    );
    assert!(
        receipt
            .response_text
            .as_deref()
            .is_some_and(|text| !text.is_empty())
    );
    for process in &started.processes {
        assert!(process_is_running(process.pid)?);
    }
    println!(
        "NATIVE_IMMEDIATE_SAVE native_request_submitted=true; enabled_shards={}; processes_still_running=true",
        started.process_count
    );
    Ok(())
}
