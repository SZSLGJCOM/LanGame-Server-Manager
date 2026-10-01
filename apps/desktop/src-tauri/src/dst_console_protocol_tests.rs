use super::*;

#[test]
fn generation_progress_ignores_commands_and_periodic_server_noise() {
    assert!(world_generation_progress(
        "[00:01:03]: An error occured during world gen we will retry! [was 1 of 5]"
    ));
    assert!(world_generation_progress("[00:01:04]: Creating story..."));
    for line in [
        "[00:01:04]: RemoteCommandInput: print('Creating story...')",
        "[00:01:04]: [LGSM-DST-READY:abc]",
        "[00:01:04]: SendWorldState",
        "[00:01:04]: ModManager: Loading workshop-1",
    ] {
        assert!(!world_generation_progress(line));
    }
}

#[test]
fn native_configuration_failures_cannot_be_reported_ready() {
    for message in [
        "ERROR: Failed to load modoverrides.lua",
        "ERROR: Failed to run code from modoverrides.lua",
        "ERROR: Failed to load ../worldgenoverride.lua",
        "Worldgenoverride specified a nonexistent worldgen preset: MISSING. If this is a custom worldgen preset, it may not exist in this save location. Ignoring it and applying overrides.",
        "Worldgenoverride specified a nonexistent settings preset: MISSING. If this is a custom settings preset, it may not exist in this save location. Ignoring it and applying overrides.",
    ] {
        assert_eq!(
            startup_failure(&format!("[00:00:42]: {message}"), "probe").as_deref(),
            Some(message)
        );
        assert!(
            startup_failure(
                &format!("[00:00:42]: RemoteCommandInput: print('{message}')"),
                "probe"
            )
            .is_none()
        );
    }
    for message in [
        "Could not find modoverrides.lua",
        "Failed to load ../worldgenoverride.lua",
        "SUCCESS: Loaded modoverrides.lua",
        "Retrying world generation...",
        "Worldgenoverride specified a nonexistent custom user message",
    ] {
        assert!(startup_failure(message, "probe").is_none());
    }
}

#[test]
fn console_echo_old_nonce_and_listener_do_not_confirm_ready_or_saved() {
    for kind in ["READY", "SAVED"] {
        assert!(!is_ack(
            "[00:00:58]: Lan Server Started on port: 10999",
            kind,
            "abc"
        ));
        assert!(!is_ack(
            &format!("[00:00:59]: RemoteCommandInput: print('[LGSM-DST-{kind}:abc]')"),
            kind,
            "abc"
        ));
        assert!(!is_ack(
            &format!("[00:00:59]: [LGSM-DST-{kind}:old]"),
            kind,
            "abc"
        ));
        assert!(is_ack(
            &format!("[00:01:10]: [LGSM-DST-{kind}:abc]\t"),
            kind,
            "abc"
        ));
    }
}

#[test]
fn ready_requires_gameplay_and_the_enabled_caves_connection() {
    let lua = mlua::Lua::new();
    lua.load("out={};function print(s) table.insert(out,s) end;playing=false;function InGamePlay() return playing end;TheWorld={ismastersim=true};ModManager={mods={},failedmods={}};peers={};function Shard_GetConnectedShards() return peers end").exec().unwrap();
    lua.load(ready_command("probe", &["Caves"])).exec().unwrap();
    assert_eq!(lua.load("return #out").eval::<usize>().unwrap(), 0);
    lua.load("playing=true").exec().unwrap();
    lua.load(ready_command("probe", &["Caves"])).exec().unwrap();
    assert_eq!(lua.load("return #out").eval::<usize>().unwrap(), 0);
    lua.load("peers={['2']={ready=true,shard_name='Caves'}}")
        .exec()
        .unwrap();
    lua.load(ready_command("probe", &["Caves"])).exec().unwrap();
    assert_eq!(
        lua.load("return out[1]").eval::<String>().unwrap(),
        "[LGSM-DST-READY:probe]"
    );
    lua.load("out={};peers={}").exec().unwrap();
    lua.load(ready_command("single", &[])).exec().unwrap();
    assert_eq!(lua.load("return #out").eval::<usize>().unwrap(), 1);
}

#[test]
fn island_adventures_ready_requires_every_secondary_and_uses_native_names() {
    let lua = mlua::Lua::new();
    lua.load("out={};function print(s) table.insert(out,s) end;function InGamePlay() return true end;TheWorld={ismastersim=true};ModManager={mods={},failedmods={}};peers={};function Shard_GetConnectedShards() return peers end").exec().unwrap();
    let command = ready_command("four", &["Caves", "Islands", "Volcano"]);
    for peers in [
        "{}",
        "{['native-17']={ready=true,shard_name='Caves'}}",
        "{['native-17']={ready=true,shard_name='Caves'},['native-29']={ready=true,shard_name='Islands'}}",
        "{['native-17']={ready=true,shard_name='Caves'},['native-29']={ready=true,shard_name='Islands'},['native-31']={ready=false,shard_name='Volcano'}}",
        "{['native-17']={ready=true,shard_name='Caves'},['native-29']={ready=true,shard_name='Islands'},['native-31']={ready=true,shard_name='Moon'}}",
    ] {
        lua.load(format!("out={{}};peers={peers}")).exec().unwrap();
        lua.load(&command).exec().unwrap();
        assert_eq!(
            lua.load("return #out").eval::<usize>().unwrap(),
            0,
            "{peers}"
        );
    }
    lua.load("peers={['native-17']={ready=true,shard_name='Caves'},['native-29']={ready=true,shard_name='Islands'},['native-31']={ready=true,shard_name='Volcano'}}").exec().unwrap();
    lua.load(&command).exec().unwrap();
    assert_eq!(
        lua.load("return out").eval::<Vec<String>>().unwrap(),
        ["[LGSM-DST-READY:four]"]
    );
}

fn probe_mod_state(mod_state: &str) -> Vec<String> {
    let lua = mlua::Lua::new();
    lua.load("out={};function print(s) table.insert(out,s) end;function InGamePlay() return true end;TheWorld={ismastersim=true};ModManager={mods={},failedmods={}}").exec().unwrap();
    lua.load(mod_state).exec().unwrap();
    lua.load(ready_command("mods", &[])).exec().unwrap();
    lua.load("return out").eval().unwrap()
}

#[test]
fn ready_rejects_native_failed_mods_with_a_diagnostic() {
    let lines = probe_mod_state(
        "ModManager.failedmods={{name='workshop-2039181790',error='modmain.lua:42: missing dependency'}}",
    );
    assert!(!lines.iter().any(|line| is_ack(line, "READY", "mods")));
    assert!(lines.iter().any(|line| {
        line.starts_with("[LGSM-DST-FAILED:mods]")
            && line.contains("workshop-2039181790")
            && line.contains("missing dependency")
    }));
}

#[test]
fn ready_rejects_modinfo_failure_after_failedmods_was_cleared() {
    let lines =
        probe_mod_state("ModManager.mods={{modname='workshop-1392778117',modinfo={failed=true}}}");
    assert!(!lines.iter().any(|line| is_ack(line, "READY", "mods")));
    assert!(lines.iter().any(|line| {
        line.starts_with("[LGSM-DST-FAILED:mods]") && line.contains("workshop-1392778117")
    }));
}

#[test]
fn ready_accepts_no_mods_and_successfully_loaded_mods() {
    for state in [
        "",
        "ModManager.mods={{modname='workshop-2039181790',modinfo={failed=false}},{modname='workshop-1392778117',modinfo={}}}",
    ] {
        assert_eq!(probe_mod_state(state), ["[LGSM-DST-READY:mods]"]);
    }
}

#[test]
fn ready_mod_failure_diagnostic_is_a_single_bounded_line() {
    let lines = probe_mod_state(
        "ModManager.failedmods={{name='workshop-2039181790',error='first\\nsecond\\r'..string.rep('x',4000)}}",
    );
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("[LGSM-DST-FAILED:mods]"));
    assert!(lines[0].contains("first second"));
    assert!(!lines[0].contains(['\r', '\n']));
    assert!(lines[0].len() < 1024);
}

#[test]
fn shutdown_ack_is_emitted_only_from_the_native_save_completion_callback() {
    let lua = mlua::Lua::new();
    lua.load("out={};function print(s) table.insert(out,s) end;function InGamePlay() return true end;TheWorld={ismastersim=true};quit=false;function Shutdown() quit=true end;function c_shutdown(save) assert(save==true);pending=Shutdown end").exec().unwrap();
    lua.load(shutdown_command("stop")).exec().unwrap();
    assert_eq!(lua.load("return #out").eval::<usize>().unwrap(), 0);
    assert!(!lua.globals().get::<bool>("quit").unwrap());
    lua.load("pending()").exec().unwrap();
    assert_eq!(
        lua.load("return out[1]").eval::<String>().unwrap(),
        "[LGSM-DST-SAVED:stop]"
    );
    assert!(lua.globals().get::<bool>("quit").unwrap());
}
