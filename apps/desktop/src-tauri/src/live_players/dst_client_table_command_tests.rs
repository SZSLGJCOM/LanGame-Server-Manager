use super::dst_client_table::{CollectedLivePlayers, DstCaptureOutcome, parse_dst_client_table};

const REQUEST_ID: &str = "0123456789abcdef0123456789abcdef";

fn collect(clients: &str, client_hosted: bool) -> CollectedLivePlayers {
    let manifest: toml::Value = toml::from_str(include_str!(
        "../../../../../modules/dontstarve/module.toml"
    ))
    .unwrap();
    let action = manifest["runtime"]["player_actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["id"].as_str() == Some("list_online_players"))
        .unwrap();
    let command = action["command_template"]
        .as_str()
        .unwrap()
        .replace("{{request_id}}", REQUEST_ID);
    let lua = mlua::Lua::new();
    // The external boundary models the engine's GetPlayerClientTable helper
    // from scripts/networking.lua, including its dedicated-host exclusion.
    lua.load(format!(
        r#"
        clients={clients}
        TheNet={{
            GetClientTable=function() return clients end,
            GetServerIsClientHosted=function() return {client_hosted} end
        }}
        function GetPlayerClientTable()
            local t=TheNet:GetClientTable()
            if t==nil then return {{}} end
            if TheNet:GetServerIsClientHosted() then return t end
            for i,v in ipairs(t) do
                if v.performance~=nil then table.remove(t,i);break end
            end
            return t
        end
        out={{}}
        function print(s) table.insert(out, tostring(s).."\t") end
        "#
    ))
    .exec()
    .unwrap();
    lua.load(command).exec().unwrap();
    let lines = lua
        .globals()
        .get::<mlua::Table>("out")
        .unwrap()
        .sequence_values::<String>()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let DstCaptureOutcome::Complete(capture) =
        parse_dst_client_table(&lines, REQUEST_ID, &["kick_userid"])
    else {
        panic!("the module command must emit a complete correlated snapshot");
    };
    capture
}

#[test]
fn dst_client_table_command_excludes_host_before_same_account_player() {
    let capture = collect(
        r#"{
            {userid="KU_shared",name="[Host]",prefab="",admin=true,performance={}},
            {userid="KU_shared",name="Player",prefab="walter",admin=true}
        }"#,
        false,
    );
    assert_eq!(capture.current_players, Some(1));
    assert_eq!(capture.raw_valid_count, 1);
    assert_eq!(capture.players.len(), 1);
    let player = &capture.players[0];
    assert_eq!(player.entry.display_name, "Player");
    assert_eq!(player.entry.attributes[0].value, "walter");
    assert_eq!(player.entry.role.as_deref(), Some("admin"));
    assert_eq!(player.entry.available_action_ids, ["kick_userid"]);
    assert_eq!(player.action_bindings["kick_userid"], "KU_shared");
}

#[test]
fn dst_client_table_command_keeps_unselected_and_same_name_players() {
    let capture = collect(
        r#"{
            {userid="KU_server",name="[Host]",performance=false},
            {userid="KU_first",name="[Host]",prefab=""},
            {userid="KU_second",name="[Host]",prefab="walter"}
        }"#,
        false,
    );
    assert_eq!(capture.current_players, Some(2));
    assert_eq!(capture.players.len(), 2);
    assert_eq!(capture.players[0].entry.identifiers[0].value, "KU_first");
    assert_eq!(capture.players[0].entry.attributes[0].value, "");
    assert_eq!(capture.players[1].entry.identifiers[0].value, "KU_second");
    assert_eq!(capture.players[1].entry.attributes[0].value, "walter");
}

#[test]
fn dst_client_table_command_reports_zero_for_only_dedicated_host_or_no_clients() {
    for clients in [
        r#"{{userid="KU_server",name="[Host]",performance={}}}"#,
        "nil",
    ] {
        let capture = collect(clients, false);
        assert_eq!(capture.current_players, Some(0));
        assert!(capture.players.is_empty());
    }
}

#[test]
fn dst_client_table_command_preserves_client_hosted_player() {
    let capture = collect(
        r#"{{userid="KU_hosted",name="Host player",prefab="walter",performance={}}}"#,
        true,
    );
    assert_eq!(capture.current_players, Some(1));
    assert_eq!(capture.players[0].entry.display_name, "Host player");
    assert_eq!(capture.players[0].entry.attributes[0].value, "walter");
}

#[test]
fn dst_client_table_command_emits_one_correlated_row_per_unique_player() {
    let capture = collect(
        r#"{
            {userid="KU_player",name="Player",prefab="walter"},
            {userid="KU_player",name="Player",prefab="walter"}
        }"#,
        false,
    );
    assert_eq!(capture.raw_valid_count, 1);
    assert_eq!(capture.current_players, Some(1));
    assert_eq!(capture.players.len(), 1);
}
