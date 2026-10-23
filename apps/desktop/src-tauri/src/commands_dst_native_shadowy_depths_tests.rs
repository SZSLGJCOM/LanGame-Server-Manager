use super::*;
use crate::commands::commands_dst_map::{DstMapInput, DstMapSnapshot, read_dontstarve_world_map};

type ProbeResult<T> = Result<T, Box<dyn std::error::Error>>;

// Native 759671 registrations, including dynamically registered hats. Merely
// checking these does not create the bosses or start their encounters.
const PREFABS: &[&str] = &[
    "armor_rocky",
    "bat_bosscorpsehat",
    "queen_torch",
    "king_cane",
    "mask_charlie_bosshat",
    "atrium_ritual_organ_rocky",
    "atrium_ritual_organ_bat",
    "atrium_ritual_organ_worm",
    "bat_boss",
    "bat_boss_shadow",
    "rocky_boss",
    "rocky_boss_shadow",
    "worm_boss_shadow",
    "charlie_boss",
    "shrouden",
    "charlie_boss_runner",
    "batbosscave",
];

// The harness owns a disposable, offline world. Keep all arranged entities
// outside the atrium's save radius so a room switch cannot replace their GUIDs.
const ARRANGE_RITUAL: &str = r#"
assert(#AllPlayers==0,'shadowy_depths requires an empty disposable shard')
assert(_LGSM_SHADOWY_DEPTHS==nil,'fixture already exists')
local room=assert(TheWorld.components.virtualroommanager:GetVirtualRoomSet(VIRTUALROOMSETS.ATRIUM),'generated atrium roomset missing')
assert(room.currentroomindex==0 and room.rooms.boss1,'atrium must start in its lobby')
local rx,_,rz=room:GetOrigin()
local radius=room:GetSaveRadius()+16
local s={room=room,owned={},loose={},placed={},markings={},dropped={}}
_LGSM_SHADOWY_DEPTHS=s
local points={};local w,h=TheWorld.Map:GetSize()
for ty=2,h-3 do
 for tx=2,w-3 do
  if #points<6 and TileGroupManager:IsLandTile(TheWorld.Map:GetTile(tx,ty)) then
   local x,_,z=TheWorld.Map:GetTileCenterPoint(tx,ty)
   local free=(x-rx)^2+(z-rz)^2>radius^2 and TheWorld.Map:IsPassableAtPoint(x,0,z)
   for _,p in ipairs(points) do if (x-p.x)^2+(z-p.z)^2<256 then free=false end end
   if free then points[#points+1]={x=x,z=z} end
  end
 end
 if #points==6 then break end
end
assert(#points==6,'six separated native land sites required')
local function spawn(name,p)
 local e=assert(SpawnPrefab(name),'spawn failed: '..name)
 s.owned[#s.owned+1]=e;e.Transform:SetPosition(p.x,0,p.z)
 return e
end
for i,name in ipairs({'rocky','bat','worm'}) do
 s.loose[i]=spawn('atrium_ritual_organ_'..name,points[i])
 s.placed[i]=spawn('atrium_ritual_organ_'..name,points[i+3])
 local marking=spawn('atrium_ritual_marking',points[i+3])
 marking.entity:SetCanSleep(false)
 marking:Enable(true)
 s.markings[i]=marking
end
for i,name in ipairs({'log','twigs'}) do s.dropped[i]=spawn(name,points[i]) end
s.chest=spawn('treasurechest',points[1])
result.chest=tostring(s.chest.GUID)
local tx,ty=room:GetOriginInTiles();result.origin={tx,ty}
result.classification='test_arranged'
"#;

const RITUAL_STATE: &str = r#"
local s=assert(_LGSM_SHADOWY_DEPTHS)
result.loose=0;result.placed=0;result.dropped=0
for _,e in ipairs(s.loose) do
 if e:IsValid() and e:HasTag('ritualitem') and e.marking==nil and e.components.inventoryitem.owner==nil and e.entity:GetParent()==nil then result.loose=result.loose+1 end
end
for i,e in ipairs(s.placed) do
 local m=s.markings[i]
 if e:IsValid() and m:IsValid() and m.item==e and e.marking==m and e.components.inventoryitem.owner==nil and e.entity:GetParent()==nil then result.placed=result.placed+1 end
end
for _,e in ipairs(s.dropped) do if e:IsValid() then result.dropped=result.dropped+1 end end
"#;

async fn probe(
    state: &DesktopState,
    id: &str,
    process: &StartedProcess,
    run_id: i64,
    script: &str,
) -> ProbeResult<Value> {
    // Use the existing native console and log receipt, returning Lua failures
    // as evidence instead of waiting for a marker that an assertion prevented.
    let script = script.lines().map(str::trim).collect::<Vec<_>>().join(" ");
    let value = native_probe(
        state,
        id,
        process,
        run_id,
        &format!(
            "local result={{}};local ok,err=pcall(function() {script} end);if not ok then result.error=tostring(err) end;"
        ),
        "require('json').encode_compliant(result)",
    )
    .await?;
    let value: Value = serde_json::from_str(&value)?;
    if let Some(error) = value.get("error") {
        return Err(format!("Native Shadowy Depths probe: {error}").into());
    }
    Ok(value)
}

fn production_cleanup_command() -> ProbeResult<String> {
    // Read the exact frontend command; do not maintain a second cleanup rule.
    let expression = include_str!("../../src/views/servers/gm-tools-dst.ts")
        .split_once("const DST_CLEAR_GROUND_ITEMS = ")
        .ok_or("Missing production ground cleanup command")?
        .1
        .trim()
        .strip_suffix(';')
        .ok_or("Ground cleanup must remain the final constant")?
        .lines()
        .map(|line| match line.trim_start().strip_prefix("+ ") {
            Some(literal) => format!(".. {literal}"),
            None => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    let command: String = mlua::Lua::new()
        .load(format!("return {expression}"))
        .eval()?;
    assert!(command.len() <= 512, "production GM command byte limit");
    Ok(command)
}

async fn map(
    state: tauri::State<'_, DesktopState>,
    id: &str,
    process: &StartedProcess,
    run_id: i64,
    include_terrain: bool,
) -> ProbeResult<DstMapSnapshot> {
    let map = command_result(
        read_dontstarve_world_map(
            state,
            DstMapInput {
                instance_id: id.into(),
                shard: process.process_key.clone(),
                include_terrain,
            },
        )
        .await,
    )?;
    assert_eq!(map.run_id, run_id);
    if include_terrain {
        assert_eq!(map.tiles.as_ref().unwrap().len(), map.width * map.height);
    } else {
        assert!(map.tiles.is_none());
    }
    Ok(map)
}

fn verify_third_atlas_landmark(map: &DstMapSnapshot, prefab: &str, guid: Option<&str>) {
    let landmark = map
        .landmarks
        .as_ref()
        .expect("native landmarks")
        .iter()
        .find(|item| item.prefab == prefab && guid.is_none_or(|id| item.id == id))
        .expect("native landmark from minimap_data3");
    let expected = match prefab {
        "multiplayer_portal" => "portal_dst.png",
        "treasurechest" => "treasurechest.png",
        _ => panic!("unexpected acceptance landmark"),
    };
    assert_eq!(landmark.icon, expected);
    let art = map.art.as_ref().expect("native map artwork");
    assert!(
        art.icons
            .get(expected)
            .is_some_and(|image| image.starts_with("data:image/png;base64,")),
        "Third native atlas landmark {expected}: {:?}",
        art.issues
    );
    assert!(
        art.icon_fallbacks.iter().all(|item| item.icon != expected),
        "The native atlas must supply {expected} directly"
    );
}

pub(super) async fn verify(
    state: tauri::State<'_, DesktopState>,
    id: &str,
    process: &StartedProcess,
    run_id: i64,
) -> ProbeResult<Value> {
    let names = PREFABS
        .iter()
        .map(|name| format!("'{name}'"))
        .collect::<Vec<_>>()
        .join(",");
    let registered = probe(
        &state, id, process, run_id,
        &format!("result.prefabs={{}};for _,name in ipairs({{{names}}}) do assert(PrefabExists(name),'missing prefab: '..name);result.prefabs[#result.prefabs+1]=name end"),
    ).await?;
    assert_eq!(
        registered["prefabs"].as_array().unwrap().len(),
        PREFABS.len()
    );
    if process.process_key == "master" {
        let snapshot = map(state, id, process, run_id, true).await?;
        verify_third_atlas_landmark(&snapshot, "multiplayer_portal", None);
        return Ok(
            json!({"shard":"master", "registered":registered, "nativeAtlas3":"portal_dst.png"}),
        );
    }
    assert_eq!(process.process_key, "caves", "two native shards only");
    let settings = probe(
        &state,
        id,
        process,
        run_id,
        r#"
assert(TheWorld.topology.overrides.bat_boss=='rare')
assert(TheWorld.topology.overrides.rocky_boss=='often')
assert(TUNING.BATBOSSCAVE_REGEN_PERIOD==TUNING.TOTAL_DAY_TIME*20)
assert(TUNING.BATBOSSCAVE_SPAWN_PERIOD==40 and TUNING.BATBOSSCAVE_MAX_CHILDREN==1)
assert(TUNING.ROCKY_BOSS_SPAWNDELAY_BASE==TUNING.TOTAL_DAY_TIME*7.5)
assert(TUNING.ROCKY_BOSS_SPAWNDELAY_RETRY_BASE==TUNING.TOTAL_DAY_TIME*0.75)
result.bat_boss=TheWorld.topology.overrides.bat_boss
result.rocky_boss=TheWorld.topology.overrides.rocky_boss
result.batSpawnSeconds=TUNING.BATBOSSCAVE_SPAWN_PERIOD
result.rockySpawnDays=TUNING.ROCKY_BOSS_SPAWNDELAY_BASE/TUNING.TOTAL_DAY_TIME
"#,
    )
    .await?;
    let fixture = probe(&state, id, process, run_id, ARRANGE_RITUAL).await?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let ready = probe(&state, id, process, run_id, RITUAL_STATE).await?;
        if ready["placed"] == 3 {
            assert_eq!(ready["loose"], 3);
            assert_eq!(ready["dropped"], 2);
            break;
        }
        if Instant::now() >= deadline {
            return Err(
                format!("Native ritual markings did not select their offerings: {ready}").into(),
            );
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let input = serde_json::from_value(json!({
        "instanceId":id, "processKey":process.process_key,
        "transport":"stdin", "command":production_cleanup_command()?,
    }))?;
    let receipt =
        command_result(crate::commands::send_instance_gm_command(state.clone(), input).await)?;
    assert_eq!(receipt.instance_id, id);
    let cleanup = probe(&state, id, process, run_id, RITUAL_STATE).await?;
    assert_eq!(cleanup, json!({"loose":3,"placed":3,"dropped":0}));

    let before = map(state.clone(), id, process, run_id, true).await?;
    assert!(
        !before.geometry_revision.is_empty(),
        "native atrium identity"
    );
    verify_third_atlas_landmark(&before, "treasurechest", fixture["chest"].as_str());
    let room = probe(
        &state,
        id,
        process,
        run_id,
        r#"
local r=_LGSM_SHADOWY_DEPTHS.room
r:SetRoom('boss1')
assert(r.currentroomindex==r.rooms.boss1.roomindex,'native room switch failed')
local tx,ty=r:GetOriginInTiles()
result.origin={tx,ty};result.tile=TheWorld.Map:GetTile(tx,ty)
assert(result.tile==WORLD_TILES.BRICK_GLOW,'native arena floor missing')
"#,
    )
    .await?;
    let positions = map(state.clone(), id, process, run_id, false).await?;
    assert_ne!(positions.geometry_revision, before.geometry_revision);
    let after = map(state.clone(), id, process, run_id, true).await?;
    assert_eq!(after.geometry_revision, positions.geometry_revision);
    assert_eq!(
        (&after.session, after.width, after.height),
        (&before.session, before.width, before.height)
    );
    let tx = room["origin"][0].as_u64().expect("native tile x") as usize;
    let ty = room["origin"][1].as_u64().expect("native tile y") as usize;
    assert_eq!(room["origin"], fixture["origin"]);
    assert!(tx >= 10 && ty >= 10 && tx + 10 < after.width && ty + 10 < after.height);
    let before_tiles = before.tiles.as_ref().unwrap();
    let after_tiles = after.tiles.as_ref().unwrap();
    assert_eq!(
        u64::from(after_tiles[ty * after.width + tx]),
        room["tile"].as_u64().unwrap()
    );
    let width = after.width;
    let region: Vec<_> = (ty - 10..=ty + 10)
        .flat_map(|y| (tx - 10..=tx + 10).map(move |x| y * width + x))
        .collect();
    let changed = region
        .iter()
        .filter(|&&offset| before_tiles[offset] != after_tiles[offset])
        .count();
    assert!(
        changed > 0,
        "room switch must recollect the changed native terrain"
    );

    // This native layout creates exactly one Charlie encounter. Return through
    // the native room API, which unloads the encounter and restores its lobby.
    probe(
        &state,
        id,
        process,
        run_id,
        r#"
local s=_LGSM_SHADOWY_DEPTHS;s.room:SetRoom(0)
assert(s.room:IsCurrentRoomLobby(),'atrium lobby did not restore')
for _,e in ipairs(s.owned) do if e:IsValid() then e:Remove() end end
_LGSM_SHADOWY_DEPTHS=nil;result.restored=true
"#,
    )
    .await?;
    let restored = map(state, id, process, run_id, true).await?;
    assert_ne!(restored.geometry_revision, after.geometry_revision);
    assert_eq!(
        (&restored.session, restored.width, restored.height),
        (&before.session, before.width, before.height)
    );
    let restored_tiles = restored.tiles.as_ref().unwrap();
    for offset in region {
        assert_eq!(
            restored_tiles[offset], before_tiles[offset],
            "restored atrium tile offset {offset}"
        );
    }
    Ok(json!({
        "shard":"caves", "registered":registered, "worldSettings":settings, "fixture":fixture, "cleanup":cleanup,
        "nativeAtlas3":"treasurechest.png", "changedTiles":changed,
        "geometryRevisions":{"before":before.geometry_revision,"positions":positions.geometry_revision,
            "after":after.geometry_revision,"restored":restored.geometry_revision},
        "nativeArenaTile":room, "lobbyRestored":true,
    }))
}
