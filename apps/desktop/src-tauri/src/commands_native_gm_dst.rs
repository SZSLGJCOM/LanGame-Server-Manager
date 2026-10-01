use super::*;

pub(super) fn verify_bounded_targets(
    cases: &[&ToolCase],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut verified = 0;
    for case in cases.iter().filter(|case| {
        case.tool_id == "dst_give_item_to_player"
            && case
                .values
                .get("allPlayers")
                .is_some_and(|value| value == "true")
    }) {
        let amount: usize = case
            .values
            .get("amount")
            .ok_or("missing DST amount")?
            .parse()?;
        if !(1..=999).contains(&amount) {
            return Err("invalid bounded DST test amount".into());
        }
        let lua = mlua::Lua::new();
        lua.globals().set("budget", 2 * amount)?;
        // Only native external entity creation is replaced here. The exported
        // Lua executes its real iteration, validation and target selection.
        lua.load(
            r#"
            spawned=0
            function player()
                return {components={inventory={GiveItem=function() end}},
                    HasTag=function() return false end, GetPosition=function() return {} end,
                    Transform={GetWorldPosition=function() return 0,0,0 end,
                        SetPosition=function() end}}
            end
            AllPlayers={player(),player()}
            function SpawnPrefab()
                spawned=spawned+1
                assert(spawned<=budget, 'all-player delivery expanded its own target list')
                local p=player(); AllPlayers[#AllPlayers+1]=p
                return {components={inventoryitem={}}, Transform=p.Transform, Remove=function() end}
            end
            print=function() end
        "#,
        )
        .exec()?;
        for command in &case.commands {
            lua.load(command).exec()?;
        }
        let (spawned, players) = lua
            .load("return spawned,#AllPlayers")
            .eval::<(usize, usize)>()?;
        if spawned != 2 * amount || players != 2 + 2 * amount {
            return Err("DST all-player command did not retain its initial target snapshot".into());
        }
        verified += 1;
    }
    if verified > 0 {
        println!(
            "NATIVE_GM module=dontstarve phase=bounded_target_iteration evidence=exported_lua_with_entity_boundary_fixture cases={verified} passed=true"
        );
    }
    Ok(())
}

pub(super) async fn prepare<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
) -> Result<(), Box<dyn std::error::Error>> {
    for process in &running
        .active_run
        .as_ref()
        .ok_or("DST has no active run")?
        .processes
    {
        let value = probe(runtime, running, &process.process_key,
            "assert(#AllPlayers==0,'native fixture unexpectedly has clients'); _LGSM_GM_PLAYERS={}; for i=1,2 do local p=SpawnPrefab('wilson'); assert(p and p.components.inventory,'native player spawn failed'); p.entity:SetCanSleep(false); p.Transform:SetPosition((i-1)*20,0,0); p.components.health:SetInvincible(true); _LGSM_GM_PLAYERS[i]=p end;",
            "tostring(#AllPlayers)").await?;
        if value != "2" {
            return Err("DST did not create two real native player entities".into());
        }
        println!(
            "NATIVE_GM module=dontstarve phase=fixture shard={} players=2 type=native_entities client_join=false",
            process.process_key
        );
    }
    Ok(())
}

pub(super) async fn cleanup<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
) -> Result<(), Box<dyn std::error::Error>> {
    for process in &running
        .active_run
        .as_ref()
        .ok_or("DST has no active run")?
        .processes
    {
        let value = probe(runtime, running, &process.process_key,
            "for _,p in ipairs(_LGSM_GM_PLAYERS or {}) do if p:IsValid() then p:Remove() end end; _LGSM_GM_PLAYERS=nil;",
            "tostring(#AllPlayers)").await?;
        if value != "0" {
            return Err("DST native player entities were not removed".into());
        }
    }
    Ok(())
}

pub(super) async fn before<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    case: &ToolCase,
) -> Result<String, Box<dyn std::error::Error>> {
    if case.values.contains_key("expectedError") {
        return rejected_effect_snapshot(runtime, running, case).await;
    }
    if case.tool_id == "dst_give_item_to_player" {
        return item_counts(runtime, running, case).await;
    }
    if case.tool_id == "dst_revive_player" {
        let index = player_index(case)?;
        // Use the game's native saved-ghost transition: it configures the
        // ghost stategraph, physics, health, inventory and network state. A
        // clientless entity cannot reliably complete a death animation.
        probe(runtime, running, shard(case),
            &format!("local p=AllPlayers[{index}]; assert(p and p.ghostenabled,'native ghost mode unavailable'); p:PushEvent('makeplayerghost',{{loading=true}});"), "'native_ghost_transition'").await?;
        wait_state(
            runtime,
            running,
            shard(case),
            &format!("tostring(AllPlayers[{index}]:HasTag('playerghost'))"),
            "true",
        )
        .await?;
    }
    Ok(String::new())
}

pub(super) async fn verify_rejected_effect<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    case: &ToolCase,
    before: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let after = rejected_effect_snapshot(runtime, running, case).await?;
    if after != before {
        return Err(
            "rejected DST tool changed native inventory, player state or left a spawned entity"
                .into(),
        );
    }
    Ok(())
}

async fn rejected_effect_snapshot<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    case: &ToolCase,
) -> Result<String, Box<dyn std::error::Error>> {
    let prefab = case.values.get("prefab").map(String::as_str).unwrap_or("");
    if !prefab
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err("invalid DST negative probe prefab".into());
    }
    // Keep unrelated world simulation out of this comparison. Inventory bytes,
    // ghost state and matching loose entities near the two fixture targets are
    // the effects these negative cases must leave unchanged.
    probe(runtime, running, shard(case), &format!(
        "local r={{}}; for _,p in ipairs(_LGSM_GM_PLAYERS) do local items={{}}; for _,e in pairs(p.components.inventory.itemslots) do items[#items+1]=e.prefab..':'..(e.components.stackable and e.components.stackable:StackSize() or 1) end; table.sort(items); r[#r+1]=tostring(p:HasTag('playerghost'))..':'..table.concat(items,',') end; local ids={{}}; for id,e in pairs(Ents) do if e.prefab=='{prefab}' and e.Transform and (not e.components.inventoryitem or not e.components.inventoryitem.owner) then local x,y,z=e.Transform:GetWorldPosition(); if x*x+z*z<1600 then ids[#ids+1]=tostring(id) end end end; table.sort(ids); r[#r+1]=table.concat(ids,',');"),
        "table.concat(r,'|')").await
}

pub(super) async fn after<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    case: &ToolCase,
    before: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    match case.tool_id.as_str() {
        "dst_give_item_to_player" => {
            let old = parse_counts(before)?;
            let new = parse_counts(&item_counts(runtime, running, case).await?)?;
            let amount: i64 = case
                .values
                .get("amount")
                .ok_or("missing DST amount")?
                .parse()?;
            let all_players = case
                .values
                .get("allPlayers")
                .is_some_and(|value| value == "true");
            let inventory = !case
                .values
                .get("placeInInventory")
                .is_some_and(|value| value == "false");
            let selected = if all_players {
                0
            } else {
                player_index(case)? - 1
            };
            for index in 0..2 {
                let wanted = if all_players || index == selected {
                    amount
                } else {
                    0
                };
                let offset = if inventory { 0 } else { 2 };
                if new[index + offset] - old[index + offset] != wanted {
                    return Err(format!("DST native item delivery mismatch: player={} inventory={inventory} expected_delta={wanted} actual_delta={}", index + 1, new[index + offset] - old[index + offset]).into());
                }
            }
            println!(
                "NATIVE_GM module=dontstarve tool=dst_give_item_to_player shard={} inventory={inventory} all_players={all_players} counts_before={before} counts_after={}",
                shard(case),
                new.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
            );
        }
        "dst_set_season" => {
            let expected = case.values.get("season").ok_or("missing DST season")?;
            wait_state(
                runtime,
                running,
                shard(case),
                "tostring(TheWorld.state.season)",
                expected,
            )
            .await?;
        }
        "dst_set_rain" => {
            let expected = case.values.get("enabled").ok_or("missing DST rain state")?;
            wait_state(
                runtime,
                running,
                shard(case),
                "tostring(TheWorld.state.israining or TheWorld.state.issnowing)",
                expected,
            )
            .await?;
        }
        "dst_revive_player" => {
            let index = player_index(case)?;
            wait_state(
                runtime,
                running,
                shard(case),
                &format!("tostring(AllPlayers[{index}]:HasTag('playerghost'))"),
                "false",
            )
            .await?;
        }
        _ => return Err("DST native verifier does not support this tool".into()),
    }
    Ok(())
}

fn shard(case: &ToolCase) -> &str {
    case.process_key.as_deref().unwrap_or("master")
}

fn player_index(case: &ToolCase) -> Result<usize, Box<dyn std::error::Error>> {
    let index: usize = case
        .values
        .get("playerIndex")
        .ok_or("missing DST player index")?
        .parse()?;
    if !(1..=2).contains(&index) {
        return Err("DST native probe requires player index 1 or 2".into());
    }
    Ok(index)
}

fn parse_counts(value: &str) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
    let counts = value
        .split(',')
        .map(str::parse)
        .collect::<Result<Vec<i64>, _>>()?;
    if counts.len() != 4 {
        return Err("DST native counts response is incomplete".into());
    }
    Ok(counts)
}

async fn item_counts<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    case: &ToolCase,
) -> Result<String, Box<dyn std::error::Error>> {
    let prefab = case.values.get("prefab").ok_or("missing DST prefab")?;
    if prefab.is_empty()
        || !prefab
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err("invalid DST native probe prefab".into());
    }
    probe(runtime, running, shard(case), &format!(
        "local r={{0,0,0,0}}; for i,p in ipairs(_LGSM_GM_PLAYERS) do for _,item in pairs(p.components.inventory.itemslots) do if item.prefab=='{prefab}' then r[i]=r[i]+(item.components.stackable and item.components.stackable:StackSize() or 1) end end; local x,y,z=p.Transform:GetWorldPosition(); for _,e in pairs(Ents) do if e.prefab=='{prefab}' and e.Transform and (not e.components.inventoryitem or not e.components.inventoryitem.owner) then local ex,ey,ez=e.Transform:GetWorldPosition(); if (ex-x)^2+(ez-z)^2<4 then r[i+2]=r[i+2]+(e.components.stackable and e.components.stackable:StackSize() or 1) end end end end;"),
        "table.concat(r,',')").await
}

async fn wait_state<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    shard: &str,
    expression: &str,
    expected: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let actual = probe(runtime, running, shard, "", expression).await?;
        if actual == expected {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(
                format!("DST native state mismatch: expected={expected} actual={actual}").into(),
            );
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn probe<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    shard: &str,
    prelude: &str,
    expression: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let run = running
        .active_run
        .as_ref()
        .ok_or("DST fixture has no current run")?;
    let process = run
        .processes
        .iter()
        .find(|process| process.process_key == shard)
        .ok_or("DST native probe shard missing")?;
    let path = process
        .log_path
        .as_ref()
        .ok_or("DST native probe lacks captured log")?;
    let logs = vec![(PathBuf::from(path), fs::metadata(path)?.len())];
    let marker = format!("LGSM-GM-{}", uuid::Uuid::new_v4().simple());
    let command = format!("{prelude} print('{marker} '..({expression}))");
    dispatch_managed_stdin_command(
        runtime.state,
        &running.summary.id,
        Some(shard),
        &command,
        Some(run.run_id),
    )
    .await?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let text = new_logs(&logs)?;
        if let Some(line) = text
            .lines()
            .find(|line| line.contains(&marker) && !line.contains("print("))
        {
            return Ok(line
                .split_once(&marker)
                .ok_or("DST native marker missing")?
                .1
                .trim()
                .to_owned());
        }
        if Instant::now() >= deadline {
            runtime::print_native_failure(
                "gm_dst_probe",
                &text,
                &[],
                runtime.settings,
                &runtime.package.root,
            );
            return Err("DST native state query did not produce a fresh marker".into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
