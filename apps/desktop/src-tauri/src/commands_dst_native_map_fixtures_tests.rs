use super::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

type ProbeResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FixtureInput {
    raw_json: String,
    input_sha256: String,
    prefabs: Vec<String>,
}

pub(super) fn parse(scenario: &str, raw: Option<&str>) -> Result<Option<FixtureInput>, String> {
    let Some(raw) = raw else { return Ok(None) };
    if scenario != "custom_mods" {
        return Err("Map fixtures are accepted only by the custom_mods scenario".into());
    }
    if raw.len() > 4096 {
        return Err("Map fixture input exceeds 4096 UTF-8 bytes".into());
    }
    let prefabs: Vec<String> = serde_json::from_str(raw)
        .map_err(|error| format!("Map fixtures must be a JSON string array: {error}"))?;
    if prefabs.is_empty() || prefabs.len() > 8 {
        return Err("Map fixtures must contain 1..8 building prefab names".into());
    }
    let mut unique = HashSet::new();
    for name in &prefabs {
        if name.is_empty()
            || name.len() > 128
            || !name.as_bytes()[0].is_ascii_lowercase()
            || !name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            || !unique.insert(name)
        {
            return Err(
                "Map fixture names must be unique ASCII [a-z][a-z0-9_]*, at most 128 bytes".into(),
            );
        }
    }
    Ok(Some(FixtureInput {
        raw_json: raw.into(),
        input_sha256: super::mod_input::input_sha256(raw.as_bytes()),
        prefabs,
    }))
}

// This is a fixed, test-only native operation. Only validated names are inserted
// as string data. It does not modify author packages or move/create players.
// No RNG is consumed to find a site, and no generation-1 path invokes it again.
const PLACE_BUILDINGS: &str = r#"local result={session=TheNet:GetSessionIdentifier(),fixtures={},preexistingCounts={}}
local ok,err=pcall(function()
 assert(TheWorld.ismastersim and type(result.session)=='string' and #result.session>0,'native world not ready')
 local banned={}
 for _,key in ipairs({'DST_CHARACTERLIST','MODCHARACTERLIST'}) do
  for _,name in ipairs(rawget(_G,key) or {}) do banned[name]=true end
 end
 for _,name in ipairs(fixturePrefabs) do
  assert(not banned[name] and type(Prefabs[name])=='table','unknown or player prefab: '..name)
  result.preexistingCounts[name]=0
 end
 local anchor=nil;local rank=3;local scanned=0
 for _,entity in pairs(Ents) do
  scanned=scanned+1;assert(scanned<=200000,'entity scan limit')
  if entity:IsValid() then
   local name=entity.prefab
   if result.preexistingCounts[name]~=nil then result.preexistingCounts[name]=result.preexistingCounts[name]+1 end
   local r=name=='multiplayer_portal' and 1 or name=='cave_exit' and 2 or 3
   if r<3 and (r<rank or r==rank and entity.GUID<anchor.GUID) then anchor=entity;rank=r end
  end
 end
 assert(anchor and anchor.Transform,'no native portal or cave-exit anchor')
 local ax,ay,az=anchor.Transform:GetWorldPosition()
 result.anchor={prefab=anchor.prefab,guid=string.format('%.0f',anchor.GUID),x=ax,z=az}
 local map=TheWorld.Map
 local function land(x,z)
  for dx=-2,2,2 do for dz=-2,2,2 do
   if not TileGroupManager:IsLandTile(map:GetTileAtPoint(x+dx,0,z+dz)) or not map:IsPassableAtPoint(x+dx,0,z+dz) then return false end
  end end
  return true
 end
 for _,name in ipairs(fixturePrefabs) do
  local px,pz=nil,nil;local attempts=0
  for radius=2,16 do
   for dx=-radius,radius do for dz=-radius,radius do
    if math.abs(dx)==radius or math.abs(dz)==radius then
     attempts=attempts+1
     if attempts<=1024 and not px then
      local x,z=ax+dx*4,az+dz*4
      if land(x,z) and #TheSim:FindEntities(x,0,z,6)==0 then px=x;pz=z end
     end
    end
   end end
   if px or attempts>=1024 then break end
  end
  assert(px,'no safe land site for '..name)
  local entity=SpawnPrefab(name)
  assert(entity and entity:IsValid() and entity.Transform,'prefab failed: '..name)
  assert(not entity:HasTag('player') and not entity:HasTag('playerghost') and entity:HasTag('structure'),'prefab is not a building: '..name)
  assert(entity.prefab==name and entity.persists==true,'nonpersistent or renamed building: '..name)
  entity.Transform:SetPosition(px,0,pz)
  local x,y,z=entity.Transform:GetWorldPosition()
  assert(land(x,z) and math.abs(x-px)<0.01 and math.abs(z-pz)<0.01,'building position changed')
  assert(type(entity.GUID)=='number' and entity.GUID>0 and entity.GUID<=9007199254740991 and entity.GUID%1==0,'invalid native GUID')
  table.insert(result.fixtures,{prefab=entity.prefab,guid=string.format('%.0f',entity.GUID),x=x,z=z,tile=map:GetTileAtPoint(x,0,z),persistent=true,classification='test_arranged'})
 end
end)
if not ok then result.error=tostring(err) end
"#;

fn prelude(input: &FixtureInput) -> String {
    // The parser permits no quotes, slashes or escape sequences in names.
    let names = input
        .prefabs
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(",");
    // The dedicated server console executes one command per input line.
    format!("local fixturePrefabs={{{names}}};{PLACE_BUILDINGS}").replace('\n', " ")
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NativeAnchor {
    prefab: String,
    guid: String,
    x: f64,
    z: f64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NativeBuilding {
    prefab: String,
    guid: String,
    x: f64,
    z: f64,
    tile: u16,
    persistent: bool,
    classification: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativePlacement {
    session: String,
    anchor: Option<NativeAnchor>,
    fixtures: Vec<NativeBuilding>,
    preexisting_counts: BTreeMap<String, u32>,
    error: Option<String>,
}

fn validate(input: &FixtureInput, placed: &NativePlacement) -> Result<(), String> {
    if let Some(error) = &placed.error {
        return Err(format!("Native map fixture placement failed: {error}"));
    }
    let anchor = placed
        .anchor
        .as_ref()
        .ok_or("Native fixture anchor missing")?;
    let valid_guid = |guid: &str| {
        guid.parse::<u64>()
            .ok()
            .is_some_and(|n| n > 0 && n <= 9_007_199_254_740_991 && n.to_string() == guid)
    };
    if placed.session.is_empty()
        || placed.session.len() > 128
        || !matches!(anchor.prefab.as_str(), "multiplayer_portal" | "cave_exit")
        || !valid_guid(&anchor.guid)
        || !anchor.x.is_finite()
        || !anchor.z.is_finite()
        || placed.fixtures.len() != input.prefabs.len()
        || placed.preexisting_counts.len() != input.prefabs.len()
        || input
            .prefabs
            .iter()
            .any(|name| !placed.preexisting_counts.contains_key(name))
    {
        return Err("Native map fixture identity or count is invalid".into());
    }
    let mut guids = HashSet::new();
    for (index, (name, entity)) in input.prefabs.iter().zip(&placed.fixtures).enumerate() {
        if &entity.prefab != name
            || !valid_guid(&entity.guid)
            || !guids.insert(&entity.guid)
            || !entity.x.is_finite()
            || !entity.z.is_finite()
            || !entity.persistent
            || entity.classification != "test_arranged"
            || entity.guid == anchor.guid
            || (entity.x - anchor.x).abs() > 64.01
            || (entity.z - anchor.z).abs() > 64.01
            || (entity.x - anchor.x).hypot(entity.z - anchor.z) < 6.0
            || placed.fixtures[..index]
                .iter()
                .any(|prior| (entity.x - prior.x).hypot(entity.z - prior.z) < 6.0)
        {
            return Err("Native map fixture building evidence is invalid".into());
        }
    }
    Ok(())
}

pub(super) async fn arrange(
    state: &DesktopState,
    id: &str,
    process: &StartedProcess,
    run_id: i64,
    root: &Path,
    input: &FixtureInput,
) -> ProbeResult<Value> {
    if !matches!(process.process_key.as_str(), "master" | "caves") {
        return Err("Unexpected custom Mod fixture shard".into());
    }
    let raw = native_probe(
        state,
        id,
        process,
        run_id,
        &prelude(input),
        "(function() local encoded=require('json').encode_compliant(result); assert(#encoded<=3800,'native fixture frame limit'); return encoded end)()",
    )
    .await?;
    if raw.len() > 4096 {
        return Err("Native fixture response exceeds 4096 bytes".into());
    }
    let native: Value = serde_json::from_str(&raw)?;
    let name = format!("native-map-fixtures-{}.json", process.process_key);
    let document = json!({"schema":"lgsm-dst-native-map-fixtures-v1","classification":"test_arranged",
        "generation":0,"instanceId":id,"runId":run_id,"shard":process.process_key,
        "input":input,"session":native["session"],"anchor":native["anchor"],"fixtures":native["fixtures"],
        "preexistingCounts":native["preexistingCounts"],"error":native["error"]});
    super::map_save_checks::write_evidence(root, &name, &document)?;
    // Preserve native partial/error evidence before failing the entire fixture.
    if let Some(error) = native["error"].as_str() {
        return Err(format!("Native map fixture placement failed: {error}").into());
    }
    let placed: NativePlacement = serde_json::from_value(native)?;
    validate(input, &placed)?;
    let path = root.join(name);
    Ok(
        json!({"path":path,"sha256":super::mod_input::input_sha256(&fs::read(&path)?),"shard":process.process_key}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_input_is_optional_only_custom_scenarios_may_supply_it() {
        assert!(parse("vanilla", None).unwrap().is_none());
        assert!(parse("custom_mods", None).unwrap().is_none());
        for scenario in ["vanilla", "myth_words", "unknown"] {
            for raw in ["", "null", "[]", r#"["sample_building"]"#] {
                assert!(parse(scenario, Some(raw)).is_err());
            }
        }
    }

    #[test]
    fn prefab_syntax_count_duplicates_and_complete_json_are_enforced() {
        for raw in [
            "",
            "null",
            "{}",
            "[]",
            "[1]",
            "[null]",
            r#"["a"] []"#,
            r#"["a","a"]"#,
        ] {
            assert!(parse("custom_mods", Some(raw)).is_err(), "{raw}");
        }
        for name in [
            "", "A", "1a", "_a", "a-b", "a/b", "a\\b", "a.lua", "a\n", "é", "a'", "a\0",
        ] {
            assert!(
                parse(
                    "custom_mods",
                    Some(&serde_json::to_string(&[name]).unwrap())
                )
                .is_err(),
                "{name:?}"
            );
        }
        assert!(
            parse(
                "custom_mods",
                Some(&serde_json::to_string(&["a".repeat(128)]).unwrap())
            )
            .is_ok()
        );
        assert!(
            parse(
                "custom_mods",
                Some(&serde_json::to_string(&["a".repeat(129)]).unwrap())
            )
            .is_err()
        );
        let names = (0..8).map(|n| format!("building_{n}")).collect::<Vec<_>>();
        assert!(parse("custom_mods", Some(&serde_json::to_string(&names).unwrap())).is_ok());
        assert!(
            parse(
                "custom_mods",
                Some(
                    &serde_json::to_string(&(0..9).map(|n| format!("b{n}")).collect::<Vec<_>>())
                        .unwrap()
                )
            )
            .is_err()
        );
    }

    #[test]
    fn exact_byte_limit_and_original_input_hash_are_preserved() {
        let raw = format!("{}[\"a\"]", " ".repeat(4091));
        let parsed = parse("custom_mods", Some(&raw)).unwrap().unwrap();
        assert_eq!(parsed.raw_json, raw);
        assert_eq!(
            parsed.input_sha256,
            super::super::mod_input::input_sha256(raw.as_bytes())
        );
        assert!(parse("custom_mods", Some(&(raw + " "))).is_err());
        assert_ne!(
            parsed.input_sha256,
            parse("custom_mods", Some(r#"["a"]"#))
                .unwrap()
                .unwrap()
                .input_sha256
        );
    }

    fn native_fixture() -> mlua::Lua {
        let lua = mlua::Lua::new();
        lua.load(r#"
            calls=0;scanCalls=0;blocked=false;passable=true;island=true;spawned={};nextGuid=20
            DST_CHARACTERLIST={'wilson'};MODCHARACTERLIST={'custom_player'}
            Prefabs={building_a={},building_b={},wilson={},custom_player={}}
            function entity(name,guid,x,z,structure)
                local e={prefab=name,GUID=guid,persists=true,Transform={},x=x,z=z}
                function e:IsValid() return true end
                function e:HasTag(tag) return tag=='structure' and structure or false end
                function e.Transform:GetWorldPosition() return e.x,0,e.z end
                function e.Transform:SetPosition(x,y,z) e.x=x;e.z=z end
                return e
            end
            Ents={[1]=entity('multiplayer_portal',1,100,-100,false)}
            TheWorld={ismastersim=true,Map={}}
            function TheWorld.Map:GetTileAtPoint(x,y,z) return 42 end
            function TheWorld.Map:IsPassableAtPoint(x,y,z) return passable end
            TileGroupManager={};function TileGroupManager:IsLandTile(tile) return island end
            TheNet={};function TheNet:GetSessionIdentifier() return 'native-session' end
            TheSim={};function TheSim:FindEntities(x,y,z,r)
                scanCalls=scanCalls+1
                if blocked then return {Ents[1]} end
                local found={}
                for _,e in pairs(Ents) do if (e.x-x)^2+(e.z-z)^2<r*r then table.insert(found,e) end end
                return found
            end
            function SpawnPrefab(name)
                calls=calls+1;nextGuid=nextGuid+1
                local e=entity(name,nextGuid,0,0,true);Ents[nextGuid]=e;table.insert(spawned,e);return e
            end
        "#).exec().unwrap();
        lua
    }

    fn run(lua: &mlua::Lua, raw: &str) -> mlua::Table {
        let input = parse("custom_mods", Some(raw)).unwrap().unwrap();
        let source = format!("{};return result", prelude(&input));
        assert!(source.len() < 15360);
        lua.load(source).eval().unwrap()
    }

    #[test]
    fn fixed_native_operation_places_real_buildings_without_moving_anchor_or_using_rng() {
        let lua = native_fixture();
        lua.load("math.random=function() error('unexpected RNG') end;math.randomseed=math.random")
            .exec()
            .unwrap();
        let result = run(&lua, r#"["building_a","building_b"]"#);
        assert!(result.get::<Option<String>>("error").unwrap().is_none());
        assert_eq!(lua.globals().get::<u32>("calls").unwrap(), 2);
        let items = result.get::<mlua::Table>("fixtures").unwrap();
        assert_eq!(items.raw_len(), 2);
        for i in 1..=2 {
            let item = items.get::<mlua::Table>(i).unwrap();
            assert_eq!(
                item.get::<String>("classification").unwrap(),
                "test_arranged"
            );
            assert!(item.get::<bool>("persistent").unwrap());
            assert_eq!(item.get::<u16>("tile").unwrap(), 42);
        }
        let positions: (f64, f64) = lua.load("return Ents[1].x,Ents[1].z").eval().unwrap();
        assert_eq!(positions, (100.0, -100.0));
        let count = result.get::<mlua::Table>("preexistingCounts").unwrap();
        assert_eq!(count.get::<u32>("building_a").unwrap(), 0);
        let a = items.get::<mlua::Table>(1).unwrap();
        let b = items.get::<mlua::Table>(2).unwrap();
        let dx = a.get::<f64>("x").unwrap() - b.get::<f64>("x").unwrap();
        let dz = a.get::<f64>("z").unwrap() - b.get::<f64>("z").unwrap();
        assert!(dx * dx + dz * dz >= 36.0);
    }

    #[test]
    fn native_preflight_rejects_missing_and_player_prefabs_before_any_spawn() {
        for raw in [
            r#"["building_a","missing"]"#,
            r#"["wilson"]"#,
            r#"["custom_player"]"#,
        ] {
            let lua = native_fixture();
            let result = run(&lua, raw);
            assert!(result.get::<Option<String>>("error").unwrap().is_some());
            assert_eq!(lua.globals().get::<u32>("calls").unwrap(), 0);
        }
    }

    #[test]
    fn bounded_native_scan_rejects_crowded_ocean_impassable_and_missing_anchor() {
        for setup in ["blocked=true", "island=false", "passable=false", "Ents={}"] {
            let lua = native_fixture();
            lua.load(setup).exec().unwrap();
            let result = run(&lua, r#"["building_a"]"#);
            assert!(
                result.get::<Option<String>>("error").unwrap().is_some(),
                "{setup}"
            );
            assert_eq!(lua.globals().get::<u32>("calls").unwrap(), 0);
            assert!(lua.globals().get::<u32>("scanCalls").unwrap() <= 1024);
        }
    }

    #[test]
    fn native_spawn_requires_structure_identity_persistence_and_a_real_guid() {
        for mutation in [
            "e.persists=false",
            "e.prefab='other'",
            "e.GUID=0",
            "e.HasTag=function(self,tag) return tag=='player' end",
        ] {
            let lua = native_fixture();
            lua.load(format!("local original=SpawnPrefab;SpawnPrefab=function(name) local e=original(name);{mutation};return e end")).exec().unwrap();
            let result = run(&lua, r#"["building_a"]"#);
            assert!(
                result.get::<Option<String>>("error").unwrap().is_some(),
                "{mutation}"
            );
            assert_eq!(lua.globals().get::<u32>("calls").unwrap(), 1);
        }
    }

    #[test]
    fn host_validation_rejects_partial_reordered_duplicate_or_nonpersistent_evidence() {
        let input = parse("custom_mods", Some(r#"["building_a"]"#))
            .unwrap()
            .unwrap();
        let value = json!({"session":"native-session","anchor":{"prefab":"cave_exit","guid":"1","x":0,"z":0},"fixtures":[{"prefab":"building_a","guid":"2","x":8,"z":8,"tile":42,"persistent":true,"classification":"test_arranged"}],"preexistingCounts":{"building_a":0}});
        let placed: NativePlacement = serde_json::from_value(value.clone()).unwrap();
        assert!(validate(&input, &placed).is_ok());
        for (path, bad) in [
            ("prefab", json!("other")),
            ("guid", json!("0")),
            ("persistent", json!(false)),
            ("classification", json!("natural")),
            ("x", json!(1000)),
        ] {
            let mut changed = value.clone();
            changed["fixtures"][0][path] = bad;
            assert!(
                validate(&input, &serde_json::from_value(changed).unwrap()).is_err(),
                "{path}"
            );
        }
        let mut partial = value.clone();
        partial["fixtures"] = json!([]);
        assert!(validate(&input, &serde_json::from_value(partial).unwrap()).is_err());
        let mut failed = value;
        failed["error"] = json!("no safe land site");
        assert!(validate(&input, &serde_json::from_value(failed).unwrap()).is_err());
        let input = parse("custom_mods", Some(r#"["building_a","building_b"]"#))
            .unwrap()
            .unwrap();
        let mut pair = serde_json::to_value(placed).unwrap();
        pair["preexistingCounts"]["building_b"] = json!(0);
        let second = pair["fixtures"][0].clone();
        pair["fixtures"].as_array_mut().unwrap().push(second);
        pair["fixtures"][1]["prefab"] = json!("building_b");
        pair["fixtures"][1]["x"] = json!(16);
        assert!(validate(&input, &serde_json::from_value(pair.clone()).unwrap()).is_err());
        pair["fixtures"][1]["guid"] = json!("3");
        let both: NativePlacement = serde_json::from_value(pair.clone()).unwrap();
        assert!(validate(&input, &both).is_ok());
        pair["fixtures"].as_array_mut().unwrap().swap(0, 1);
        assert!(validate(&input, &serde_json::from_value(pair).unwrap()).is_err());
    }
}
