use super::*;

const RUN: &str = "LGSM_UWS_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn records() -> Vec<serde_json::Value> {
    ["inspect", "before", "after"]
        .into_iter()
        .enumerate()
        .map(|(index, stage)| {
            serde_json::json!({
                "run": RUN, "sequence": index + 1, "stage": stage, "valid": true,
                "interval": 120.0, "game_seconds": 20.0 + index as f64,
                "real_seconds": 30.0 + index as f64, "dilation": 1.0
            })
        })
        .collect()
}

fn encode(records: &[serde_json::Value]) -> Vec<u8> {
    records
        .iter()
        .map(|record| format!("{record}\n"))
        .collect::<String>()
        .into_bytes()
}

#[test]
fn moria_timing_requires_current_run_actual_interval_and_one_call() {
    let records = records();
    assert_eq!(
        parse(&encode(&records), RUN, Duration::from_secs(40), true)
            .unwrap()
            .interval,
        120.0
    );
    assert!(parse(&encode(&records[..1]), RUN, Duration::from_secs(40), false).is_ok());
    assert!(parse(&encode(&records[..2]), RUN, Duration::from_secs(40), true).is_err());
    assert!(
        parse(
            &encode(&records),
            "another private run",
            Duration::from_secs(40),
            true
        )
        .is_err()
    );
    assert!(parse(&encode(&records), RUN, Duration::from_secs(120), true).is_err());
    for (index, key, value) in [
        (1, "interval", serde_json::json!(300.0)),
        (1, "game_seconds", serde_json::json!(19.0)),
        (1, "game_seconds", serde_json::json!(32.0)),
        (1, "real_seconds", serde_json::json!(41.0)),
        (1, "dilation", serde_json::json!(2.0)),
        (1, "dilation", serde_json::json!(0.5)),
        (1, "interval", serde_json::json!(0.0)),
        (1, "interval", serde_json::Value::Null),
        (1, "sequence", serde_json::json!(1)),
        (0, "stage", serde_json::json!("before")),
        (2, "stage", serde_json::json!("before")),
    ] {
        let mut changed = records.clone();
        changed[index][key] = value;
        assert!(
            parse(&encode(&changed), RUN, Duration::from_secs(40), true).is_err(),
            "{key}"
        );
    }
}

#[test]
fn moria_timing_refuses_a_copy_that_reaches_the_timer_deadline() {
    before_first_periodic(Duration::from_millis(119_999), 120.0).unwrap();
    assert!(before_first_periodic(Duration::from_secs(120), 120.0).is_err());
    assert!(before_first_periodic(Duration::from_secs(121), 120.0).is_err());
    assert!(before_first_periodic(Duration::ZERO, f64::NAN).is_err());
    assert!(before_first_periodic(Duration::ZERO, f64::INFINITY).is_err());
    let creation = 116_444_736_000_000_000 + 100 * 10_000_000;
    assert_eq!(
        process_age(creation, Duration::from_secs(140)).unwrap(),
        Duration::from_secs(40)
    );
    assert!(process_age(creation, Duration::from_secs(99)).is_err());
}

fn lua_evidence(case: &str) -> String {
    let lua = mlua::Lua::new();
    lua.globals().set("CASE", case).unwrap();
    lua.globals().set("RUN", RUN).unwrap();
    let source = [r#"
local calls, enters, files = 0, 0, {}
local world, instance, state = {}, {}, {}
local data = {AutoSaveTime=120, IsA=function(_, name) return name=="/Script/Moria.MorSaveGameData" end}
local manager = {SaveGameData=data}
local debug = {getinfo=function() return {source="@D:/fixture/Moria/Binaries/Win64/ue4ss/Mods/LgsmWorldSave/Scripts/main.lua"} end}
local os = {
    getenv=function(name) assert(name=="LGSM_UNREAL_WORLD_SAVE_RUN");return RUN end,
    remove=function(path) if files[path] then files[path]=nil;return true end return nil,"missing",2 end,
    rename=function(from,to) files[to]=files[from];files[from]=nil;return true end
}
local io = {open=function(path, mode)
    assert(mode=="wb")
    if CASE=="io_failure" then return nil end
    return {write=function(_, text) files[path]=text;return true end,close=function() return true end}
end}
local api = {
    valid=function(object) return type(object)=="table" end,
    same=function(a,b) return a==b end,
    cdo=function(name) assert(name=="/Script/Engine.GameplayStatics");return {} end,
    signature=function(name, flags, params)
        assert(flags==0x14022401 and #params==2)
        assert(params[1][1]=="WorldContextObject" and params[1][2]=="ObjectProperty")
        assert(params[2][1]=="ReturnValue" and params[2][2]=="FloatProperty")
        if CASE=="reflection_failure" then error("private reflection failure") end
        return function(_, actual)
            assert(actual==world)
            if name=="/Script/Engine.GameplayStatics:GetTimeSeconds" then return CASE=="accelerated" and 50 or 20 end
            if name=="/Script/Engine.GameplayStatics:GetRealTimeSeconds" then return 30 end
            assert(name=="/Script/Engine.GameplayStatics:GetGlobalTimeDilation");return 1
        end
    end
}
local function build_save_target(_)
    return function(actual_world, actual_instance)
        assert(actual_world==world and actual_instance==instance)
        return {world=world,instance=instance,persistence=manager,state=state,invoke=function(argument, enter)
            assert(argument=="original argument");enter();calls=calls+1
            if CASE=="native_failure" then error("original native failure",0) end
            return false,nil,"original result"
        end}
    end
end
"#, include_str!("unreal_moria_save_timing.lua"), r#"
local inspect=build_save_target(api)
local target=inspect(world,instance)
if CASE=="world_changed" then world={};target=inspect(world,instance) end
if CASE=="interval_changed" then data.AutoSaveTime=300 end
if CASE=="interval_invalid" then data.AutoSaveTime=0/0 end
local result=table.pack(pcall(target.invoke,"original argument",function() enters=enters+1 end))
assert(calls==1 and enters==1)
if CASE=="native_failure" then assert(result[1]==false and result[2]=="original native failure")
else assert(result.n==4 and result[1]==true and result[2]==false and result[3]==nil and result[4]=="original result") end
return files["D:/fixture\\langame_world_save\\moria-test-timing.jsonl"] or ""
"#].concat();
    lua.load(source).eval().unwrap()
}

#[test]
fn moria_timing_wrapper_preserves_one_original_call_and_its_result() {
    let evidence = lua_evidence("valid");
    let sample = parse(evidence.as_bytes(), RUN, Duration::from_secs(40), true).unwrap();
    assert_eq!(sample.interval, 120.0);
    for case in [
        "reflection_failure",
        "accelerated",
        "world_changed",
        "interval_changed",
        "interval_invalid",
        "native_failure",
        "io_failure",
    ] {
        let evidence = lua_evidence(case);
        let error = parse(evidence.as_bytes(), RUN, Duration::from_secs(40), true).unwrap_err();
        assert!(!error.contains(RUN));
        assert!(!error.contains("private reflection failure"));
    }
}
