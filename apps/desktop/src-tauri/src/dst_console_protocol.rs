pub(super) fn ready_command(nonce: &str, required_peers: &[&str]) -> String {
    // Names come from the fixed native role specifications, never user Lua.
    // Native shard IDs may change on imported worlds; match their shard names.
    let required = required_peers
        .iter()
        .map(|name| format!("[{name:?}]=true"))
        .collect::<Vec<_>>()
        .join(",");
    let peer_check = if required_peers.is_empty() {
        String::from("local peer=true;")
    } else {
        format!(
            "local required={{{required}}};for _,v in pairs(Shard_GetConnectedShards()) do if v.ready then required[v.shard_name]=nil end end;local peer=next(required)==nil;"
        )
    };
    // Native ModManager retains environments after a modmain failure. Inspect
    // its errors, not just the presence of a world or a mod environment. This
    // uses the engine's actual loaded set, including raw modoverrides.lua.
    format!(
        "if InGamePlay() and TheWorld and TheWorld.ismastersim and ModManager then local failure=nil;for _,m in ipairs(ModManager.failedmods) do failure=tostring(m.name)..': '..tostring(m.error);break end;if not failure then for _,m in ipairs(ModManager.mods) do if m.modinfo and m.modinfo.failed then failure=tostring(m.modname)..': native Mod metadata reports a load failure';break end end end;if failure then print('[LGSM-DST-FAILED:{nonce}] '..string.sub(string.gsub(failure,'%c',' '),1,768)) else {peer_check}if peer then print('[LGSM-DST-READY:{nonce}]') end end end"
    )
}

pub(super) fn shutdown_command(nonce: &str) -> String {
    // c_shutdown(true) supplies Shutdown as its save-completion callback. Wrap
    // that callback only in this exiting process; a stdin echo is not a save ACK.
    format!(
        "if InGamePlay() and TheWorld and TheWorld.ismastersim then local quit=Shutdown;Shutdown=function() print('[LGSM-DST-SAVED:{nonce}]');quit() end;c_shutdown(true) end"
    )
}

pub(super) fn is_ack(line: &str, kind: &str, nonce: &str) -> bool {
    log_body(line) == format!("[LGSM-DST-{kind}:{nonce}]")
}

pub(super) fn startup_failure(line: &str, nonce: &str) -> Option<String> {
    let body = log_body(line);
    if matches!(
        body,
        "DownloadServerMods timed out with no response from Workshop..."
            | "ERROR: Failed to load modoverrides.lua"
            | "ERROR: Failed to run code from modoverrides.lua"
            | "ERROR: Failed to load ../worldgenoverride.lua"
    ) || body.starts_with("Worldgenoverride specified a nonexistent worldgen preset: ")
        || body.starts_with("Worldgenoverride specified a nonexistent settings preset: ")
    {
        return Some(body.to_owned());
    }
    body.strip_prefix(&format!("[LGSM-DST-FAILED:{nonce}] "))
        .filter(|message| !message.is_empty())
        .map(str::to_owned)
}

fn log_body(line: &str) -> &str {
    let line = line.trim();
    if line.starts_with('[') {
        line.split_once("]: ").map_or(line, |(_, body)| body.trim())
    } else {
        line
    }
}

/// Only native world-generation milestones extend startup, never console echoes,
/// periodic network messages or repeated readiness probes.
pub(super) fn world_generation_progress(line: &str) -> bool {
    let body = log_body(line);
    [
        "Generating world with these parameters:",
        "Creating story...",
        "[Story Gen] Generate nodes.",
        "GenerateVoronoiMap [",
        "[GenerateLandmasses]",
        "Checking Required Prefab ",
        "Generation complete, injecting world entities.",
        "An error occured during world gen we will retry!",
        "World generated on build ",
    ]
    .iter()
    .any(|prefix| body.starts_with(prefix))
}

#[cfg(test)]
#[path = "dst_console_protocol_tests.rs"]
mod tests;
