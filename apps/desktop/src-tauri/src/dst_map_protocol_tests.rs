use super::*;
use serde_json::json;

#[path = "dst_map_geometry_tests.rs"]
mod geometry_tests;

fn push(capture: &mut Capture, value: serde_json::Value) -> Result<Option<DstMapSnapshot>, String> {
    capture.push(&format!("[00:01:02]: [LGSM-DST-MAP:nonce] {value}\t"))
}
fn begin(capture: &mut Capture) {
    push(
        capture,
        json!({"k":"begin","width":3,"height":2,"tileScale":4,"session":"native-world",
        "day":12,"season":"autumn","phase":"day"}),
    )
    .unwrap();
}

#[test]
fn native_error_classification_uses_only_an_explicit_known_code() {
    let mut capture = Capture::new("nonce", false);
    let failure = push(
        &mut capture,
        json!({"k":"error","code":"world_not_ready",
        "message":"任意游戏语言\n"}),
    )
    .unwrap_err();
    assert_eq!(failure, "DST_MAP_WORLD_NOT_READY: 任意游戏语言");

    // Similar text and even a prefixed message cannot impersonate an engine code.
    for message in [
        "The shard world is not ready.",
        "DST_MAP_WORLD_NOT_READY: misleading",
    ] {
        let mut capture = Capture::new("nonce", false);
        let failure = push(&mut capture, json!({"k":"error","message":message})).unwrap_err();
        assert!(failure.starts_with("DST map: "));
        assert!(!failure.starts_with("DST_MAP_"));
    }
    let mut capture = Capture::new("nonce", false);
    assert_eq!(
        push(
            &mut capture,
            json!({"k":"error","code":"response_timeout",
        "message":"unknown native classification"})
        )
        .unwrap_err(),
        "Invalid native DST map frame."
    );
}

#[test]
fn actual_native_query_marks_unready_worlds_without_claiming_generation_failure() {
    for setup in [
        "TheWorld.ismastersim = false",
        "function InGamePlay() return false end",
    ] {
        for terrain in [false, true] {
            assert_eq!(
                native_query_fixture(terrain, setup).unwrap_err(),
                "DST_MAP_WORLD_NOT_READY: The shard world is not ready."
            );
        }
    }
}

#[test]
fn malformed_optional_inventory_metadata_never_drops_valid_landmarks() {
    for inventory in [
        json!(null),
        json!(42),
        json!({"atlas":"path"}),
        json!({"atlas":"x".repeat(513),"element":"image.tex"}),
        json!({"atlas":"界".repeat(171),"element":"image.tex"}),
        json!({"atlas":"valid.xml","element":"x".repeat(129)}),
        json!({"atlas":"valid.xml","element":"bad\n.tex"}),
    ] {
        let mut capture = Capture::new("nonce", true);
        begin(&mut capture);
        push(
            &mut capture,
            json!({"k":"landmark","id":"1","prefab":"actual","icon":"original.png",
            "mapX":1,"mapZ":1,"inventoryIcon":inventory}),
        )
        .unwrap();
        let landmark = &capture
            .snapshot
            .as_ref()
            .unwrap()
            .landmarks
            .as_ref()
            .unwrap()[0];
        assert_eq!(landmark.icon, "original.png");
        assert!(landmark.inventory_icon.is_none());
        assert!(
            serde_json::to_value(landmark)
                .unwrap()
                .get("inventoryIcon")
                .is_none()
        );
    }
}

#[test]
fn native_query_exports_actual_inventory_declarations_but_excludes_player_and_ghost_landmarks() {
    let source =
        include_str!("../../../../modules/dontstarve/world_map_shared/render_observer.lua");
    let setup = format!(
        r#"
        unpack=table.unpack
        local module=(function() {source} end)()
        function resolvefilepath(path) return path end
        local render=module.new(_G)
        local entries={{}}
        __lgsm_world_map_observer={{version=1,installedBeforeWorld=true,entries=entries,render=render}}
        Ents[1].components={{inventoryitem={{}}}}
        Ents[1].replica={{inventoryitem={{GetImage=function() return 'declared-item.tex' end,
            GetAtlas=function() return '../mods/enabled/images/item.xml' end}}}}
        entries[Ents[1].MiniMapEntity]={{icon='orphan.png',iconObserved=true}}
        for index,tag in ipairs({{'player','playerghost'}}) do
            local entity=landmark(100+index,'arbitrary-role',true,false,true)
            entity.HasTag=function(_,query) return query==tag end
            entries[entity.MiniMapEntity]={{icon='real-player.png',iconObserved=true}}
            Ents[100+index]=entity
        end
        AllPlayers[1].MiniMapEntity={{}}
        entries[AllPlayers[1].MiniMapEntity]={{icon='real-player.png',iconObserved=true}}
    "#
    );
    let snapshot = native_query_fixture(true, &setup).unwrap();
    let landmarks = snapshot.landmarks.unwrap();
    assert_eq!(landmarks.len(), 1);
    assert_eq!(landmarks[0].icon, "orphan.png");
    let declared = landmarks[0].inventory_icon.as_ref().unwrap();
    assert_eq!(declared.atlas, "../mods/enabled/images/item.xml");
    assert_eq!(declared.element, "declared-item.tex");
    assert_eq!(snapshot.players.len(), 1);
    assert_eq!(snapshot.players[0].icon.as_deref(), Some("real-player.png"));
}

#[test]
fn applied_terrain_layers_preserve_order_unknown_style_and_nullable_broken_winner() {
    let mut capture = Capture::new("nonce", true);
    begin(&mut capture);
    push(&mut capture, json!({"k":"tiles","offset":0,"runs":[[7,6]]})).unwrap();
    push(
        &mut capture,
        json!({"k":"name","id":7,"name":"RUNTIME_TILE","kind":"land"}),
    )
    .unwrap();
    push(&mut capture,json!({"k":"layer","tileId":7,"atlas":"first.xml","texture":"first.tex","noiseTexture":"first_noise.tex","sampleStyle":0})).unwrap();
    push(&mut capture,json!({"k":"layer","tileId":7,"atlas":null,"texture":null,"noiseTexture":null,"sampleStyle":null})).unwrap();
    let snapshot = push(&mut capture, json!({"k":"end","players":0}))
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.terrain_layers.len(), 2);
    assert_eq!(snapshot.terrain_layers[0].sample_style, Some(0));
    assert!(snapshot.terrain_layers[1].texture.is_none());
    assert!(snapshot.terrain_layers[1].sample_style.is_none());
    let json = serde_json::to_value(&snapshot).unwrap();
    assert!(json["terrainLayers"][1]["atlas"].is_null());
    assert!(json["terrainLayers"][1]["sampleStyle"].is_null());
}

#[test]
fn maximum_world_retains_geometry_when_artwork_exceeds_shared_transport_budget() {
    let mut capture = Capture::new("nonce", false);
    begin(&mut capture);
    let mut snapshot = push(&mut capture, json!({"k":"end","players":0}))
        .unwrap()
        .unwrap();
    snapshot.width = 1024;
    snapshot.height = 1024;
    snapshot.tiles = Some(vec![u16::MAX; MAX_TILES]);
    snapshot.tile_names.insert(u16::MAX, "MOD_WORLD".into());
    snapshot
        .tile_kinds
        .insert(u16::MAX, DstTerrainKind::Unknown);
    snapshot.art = Some(DstMapArt {
        textures: (0..64)
            .map(|index| (format!("texture{index}"), "a".repeat(176 * 1024)))
            .collect(),
        ..Default::default()
    });
    assert!(serde_json::to_vec(&snapshot).unwrap().len() > 16 * 1024 * 1024);
    constrain_response(&mut snapshot).unwrap();
    assert_eq!(snapshot.tiles.as_ref().unwrap().len(), MAX_TILES);
    assert!(!snapshot.art.as_ref().unwrap().issues.is_empty());
    assert!(serde_json::to_vec(&snapshot).unwrap().len() < MAX_RESPONSE_BYTES);
}

#[test]
fn terrain_requires_complete_ordered_native_tiles_names_and_players() {
    let mut capture = Capture::new("nonce", true);
    begin(&mut capture);
    push(
        &mut capture,
        json!({"k":"tiles","offset":0,"runs":[[1,2],[4,1],[5,3]]}),
    )
    .unwrap();
    for (id, name, kind) in [
        (1, "IMPASSABLE", "void"),
        (4, "DIRT", "land"),
        (5, "MOD_ISLAND_SAND", "land"),
    ] {
        push(
            &mut capture,
            json!({"k":"name","id":id,"name":name,"kind":kind}),
        )
        .unwrap();
    }
    push(
        &mut capture,
        json!({"k":"player","id":"KU_A","name":"玩家","character":"wilson",
        "x":-2.0,"z":0.0,"mapX":1.0,"mapZ":1.0}),
    )
    .unwrap();
    let snapshot = push(&mut capture, json!({"k":"end","players":1}))
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.tiles.unwrap(), vec![1, 1, 4, 5, 5, 5]);
    assert_eq!(snapshot.players[0].name, "玩家");
    assert_eq!(snapshot.tile_names[&5], "MOD_ISLAND_SAND");
    assert_eq!(snapshot.tile_kinds[&1], DstTerrainKind::Void);
    assert_eq!(snapshot.tile_kinds[&5], DstTerrainKind::Land);
    assert_eq!(snapshot.day, 12);
}

#[test]
fn stale_nonce_console_echo_or_chat_cannot_supply_map_frames() {
    let mut capture = Capture::new("nonce", false);
    for line in [
        "[00:01:02]: [LGSM-DST-MAP:old] {}",
        "[00:01:02]: RemoteCommandInput: print('[LGSM-DST-MAP:nonce] {}')",
        "[00:01:02]: [Say] [LGSM-DST-MAP:nonce] {}",
    ] {
        assert!(capture.push(line).unwrap().is_none());
    }
    begin(&mut capture);
    assert!(
        push(&mut capture, json!({"k":"end","players":0}))
            .unwrap()
            .unwrap()
            .tiles
            .is_none()
    );
}

#[test]
fn incomplete_duplicate_oversized_or_out_of_order_frames_are_rejected() {
    for value in [
        json!({"k":"tiles","offset":1,"runs":[[2,6]]}),
        json!({"k":"tiles","offset":0,"runs":[[2,0]]}),
        json!({"k":"tiles","offset":0,"runs":[[2,7]]}),
        json!({"k":"end","players":0}),
    ] {
        let mut capture = Capture::new("nonce", true);
        begin(&mut capture);
        assert!(push(&mut capture, value).is_err());
    }
    let mut capture = Capture::new("nonce", false);
    assert!(
        push(
            &mut capture,
            json!({"k":"begin","width":2048,"height":2048,"tileScale":4,
        "session":"native","day":1,"season":"winter","phase":"night"})
        )
        .is_err()
    );
    let mut capture = Capture::new("nonce", false);
    begin(&mut capture);
    let player = json!({"k":"player","id":"KU_A","name":"A","character":"wilson", "x":1,"z":2,"mapX":1,"mapZ":2});
    push(&mut capture, player.clone()).unwrap();
    assert!(push(&mut capture, player).is_err());
}

#[test]
fn native_query_is_valid_lua_and_only_interpolates_server_owned_values() {
    for terrain in [false, true] {
        let command = query("0123456789abcdef0123456789abcdef", terrain).unwrap();
        assert!(command.len() < MAX_QUERY_BYTES);
        assert!(!command.contains('\n'));
        assert!(!command.contains("__NONCE__"));
        assert!(!command.contains("__LANDMARK_ICONS__"));
        mlua::Lua::new().load(&command).into_function().unwrap();
    }
}

#[test]
fn native_query_budget_counts_utf8_bytes_and_the_terminating_newline() {
    let at_limit = format!("{}中", "x".repeat(MAX_QUERY_BYTES - 4));
    assert_eq!(at_limit.len() + 1, MAX_QUERY_BYTES);
    assert_eq!(bounded_query(at_limit.clone()).unwrap(), at_limit);
    assert!(bounded_query(format!("{at_limit}x")).is_err());
    assert!(bounded_query("中".repeat(MAX_QUERY_BYTES.div_ceil(3))).is_err());
}

#[test]
fn terrain_frames_preserve_native_texture_landmark_and_road_coordinates() {
    let mut capture = Capture::new("nonce", true);
    begin(&mut capture);
    push(&mut capture, json!({"k":"tiles","offset":0,"runs":[[4,6]]})).unwrap();
    push(&mut capture, json!({"k":"name","id":4,"name":"DIRT","kind":"land","noiseTexture":"levels/textures/mini_dirt_noise.tex"})).unwrap();
    push(&mut capture, json!({"k":"landmark","id":"1467","prefab":"pigking","icon":"pigking.png","mapX":1.25,"mapZ":0.5})).unwrap();
    push(
        &mut capture,
        json!({"k":"road","index":0,"offset":0,"complete":true,"points":[[0.25,0.5],[1.25,0.5],[2.5,1.75]]}),
    )
    .unwrap();
    let snapshot = push(
        &mut capture,
        json!({"k":"end","players":0,"landmarks":1,"roads":1,"landmarksTruncated":true}),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        snapshot.tile_textures[&4],
        "levels/textures/mini_dirt_noise.tex"
    );
    let landmark = &snapshot.landmarks.unwrap()[0];
    assert_eq!(
        (
            &*landmark.id,
            &*landmark.icon,
            landmark.map_x,
            landmark.map_z
        ),
        ("1467", "pigking.png", 1.25, 0.5)
    );
    assert_eq!(
        snapshot.roads.unwrap()[0],
        [[0.25, 0.5], [1.25, 0.5], [2.5, 1.75]]
    );
    assert!(snapshot.landmarks_truncated);
}

#[test]
fn ocean_color_frames_preserve_rgba_bytes_and_reject_invalid_channels() {
    let mut capture = Capture::new("nonce", true);
    begin(&mut capture);
    push(
        &mut capture,
        json!({"k":"tiles","offset":0,"runs":[[200,6]]}),
    )
    .unwrap();
    push(
        &mut capture,
        json!({"k":"name","id":200,"name":"OCEAN","kind":"ocean","minimapColor":[0,128,255,102]}),
    )
    .unwrap();
    let snapshot = push(&mut capture, json!({"k":"end","players":0}))
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.tile_colors[&200], [0, 128, 255, 102]);
    assert_eq!(
        serde_json::to_value(&snapshot).unwrap()["tileColors"]["200"],
        json!([0, 128, 255, 102])
    );
    for color in [
        json!([-1, 0, 0, 255]),
        json!([0, 256, 0, 255]),
        json!([0, 0.5, 0, 255]),
        json!([0, 0, 255]),
        json!([0, 0, 0, 255, 1]),
        json!([0, "0", 0, 255]),
    ] {
        let mut capture = Capture::new("nonce", true);
        begin(&mut capture);
        assert!(
            push(
                &mut capture,
                json!({"k":"name","id":200,"name":"OCEAN","kind":"ocean","minimapColor":color})
            )
            .is_err()
        );
    }
}

#[test]
fn terrain_kind_is_required_and_only_native_ocean_accepts_minimap_colors() {
    for frame in [
        json!({"k":"name","id":264,"name":"OCEAN_ICE"}),
        json!({"k":"name","id":264,"name":"OCEAN_ICE","kind":"water"}),
        json!({"k":"name","id":264,"name":"OCEAN_ICE","kind":null}),
        json!({"k":"name","id":264,"name":"OCEAN_ICE","kind":1}),
    ] {
        let mut capture = Capture::new("nonce", true);
        begin(&mut capture);
        assert!(push(&mut capture, frame).is_err());
    }
    for kind in ["land", "void", "unknown"] {
        let mut capture = Capture::new("nonce", true);
        begin(&mut capture);
        assert!(
            push(
                &mut capture,
                json!({"k":"name","id":264,"name":"OCEAN_ICE",
                "kind":kind,"minimapColor":[1,2,3,255]})
            )
            .is_err()
        );
    }
    for (kind, expected) in [
        (DstTerrainKind::Land, "land"),
        (DstTerrainKind::Ocean, "ocean"),
        (DstTerrainKind::Void, "void"),
        (DstTerrainKind::Unknown, "unknown"),
    ] {
        assert_eq!(serde_json::to_value(kind).unwrap(), expected);
    }
}

#[test]
fn invalid_or_incomplete_landmarks_and_roads_never_form_a_snapshot() {
    for frame in [
        json!({"k":"road","points":[[1,2]]}),
        json!({"k":"road","points":[[1,2,3],[4,5]]}),
        json!({"k":"road","points":[[1,2],[10000001,5]]}),
        json!({"k":"road","points":vec![[1,2];257]}),
        json!({"k":"landmark","id":"1","prefab":"pigking","icon":"pigking.png","mapX":null,"mapZ":1}),
        json!({"k":"landmark","id":"1","prefab":"pigking","icon":"pigking.png","mapX":1,"mapZ":10000001}),
        json!({"k":"landmark","id":"1","prefab":"pigking","mapX":1,"mapZ":1}),
    ] {
        let mut capture = Capture::new("nonce", true);
        begin(&mut capture);
        assert!(push(&mut capture, frame).is_err());
    }
    let mut capture = Capture::new("nonce", true);
    begin(&mut capture);
    let landmark =
        json!({"k":"landmark","id":"1","prefab":"pigking","icon":"pigking.png","mapX":1,"mapZ":1});
    push(&mut capture, landmark.clone()).unwrap();
    assert!(push(&mut capture, landmark).is_err());
    for end in [
        json!({"k":"end","players":0,"landmarks":0,"roads":1}),
        json!({"k":"end","players":0,"landmarks":1,"roads":0}),
    ] {
        let mut capture = Capture::new("nonce", true);
        begin(&mut capture);
        push(&mut capture, json!({"k":"tiles","offset":0,"runs":[[4,6]]})).unwrap();
        push(
            &mut capture,
            json!({"k":"name","id":4,"name":"DIRT","kind":"land"}),
        )
        .unwrap();
        assert!(push(&mut capture, end).is_err());
    }
}

#[test]
fn position_only_queries_reject_terrain_landmarks_and_roads() {
    for frame in [
        json!({"k":"name","id":4,"name":"DIRT","kind":"land","noiseTexture":"mods/example/mini.tex"}),
        json!({"k":"landmark","id":"1","prefab":"pigking","icon":"pigking.png","mapX":1,"mapZ":1}),
        json!({"k":"road","points":[[1,2],[3,4]]}),
        json!({"k":"end","players":0,"landmarks":1,"roads":0}),
    ] {
        let mut capture = Capture::new("nonce", false);
        begin(&mut capture);
        assert!(push(&mut capture, frame).is_err());
    }
}

#[test]
fn native_road_capture_has_total_point_and_road_count_limits() {
    for (points, allowed) in [(256, 256), (2, 1024)] {
        let mut capture = Capture::new("nonce", true);
        begin(&mut capture);
        for index in 0..allowed {
            for offset in (0..points).step_by(32) {
                let count = (points - offset).min(32);
                push(
                    &mut capture,
                    json!({"k":"road","index":index,"offset":offset,
                    "complete":offset + count == points,"points":vec![[1,2];count]}),
                )
                .unwrap();
            }
        }
        assert!(
            push(
                &mut capture,
                json!({"k":"road","index":allowed,"offset":0,
            "complete":true,"points":[[1,2],[3,4]]})
            )
            .is_err()
        );
    }
}

fn lua_json(value: mlua::Value) -> mlua::Result<serde_json::Value> {
    use mlua::Value;
    Ok(match value {
        Value::Nil => serde_json::Value::Null,
        Value::Boolean(value) => json!(value),
        Value::Integer(value) => json!(value),
        Value::Number(value) => json!(value),
        Value::String(value) => json!(value.to_str()?.to_string()),
        Value::Table(table) if table.raw_len() > 0 => serde_json::Value::Array(
            table
                .sequence_values()
                .map(|value| lua_json(value?))
                .collect::<mlua::Result<_>>()?,
        ),
        Value::Table(table) => serde_json::Value::Object(
            table
                .pairs::<String, mlua::Value>()
                .map(|entry| {
                    let (key, value) = entry?;
                    Ok((key, lua_json(value)?))
                })
                .collect::<mlua::Result<_>>()?,
        ),
        _ => {
            return Err(mlua::Error::RuntimeError(
                "Unsupported map frame fixture value".into(),
            ));
        }
    })
}

// Replace only native Lua API boundaries. The emitted frames are produced by
// the exact query used in production and consumed by the real capture parser.
fn native_query_fixture(terrain: bool, setup: &str) -> Result<DstMapSnapshot, String> {
    let lua = mlua::Lua::new();
    let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let output = lines.clone();
    lua.globals()
        .set(
            "print",
            lua.create_function(move |_, line: String| {
                output.lock().unwrap().push(line);
                Ok(())
            })
            .unwrap(),
        )
        .unwrap();
    lua.globals()
        .set(
            "encode_frame",
            lua.create_function(|_, value| Ok(lua_json(value)?.to_string()))
                .unwrap(),
        )
        .unwrap();
    lua.load(r#"
        package.preload.json = function() return {
            encode_compliant = encode_frame,
            -- Native json.encode escapes apostrophes as \' (invalid external
            -- JSON). Do not let a compliant fixture hide use of that API.
            encode = function() error('External map frames require json.encode_compliant') end
        } end
        package.preload.worldtiledefs = function() return { minimap = {
            { 1, { noise_texture = "data/levels/textures/mini_grass_noise.tex" } },
            { 2, { noise_texture = "../mods/example/data/levels/textures/mini_custom.tex" } }
        } } end
        TILE_SCALE = 4
        INVERTED_WORLD_TILES = { [1] = "GRASS", [2] = "MOD_CUSTOM" }
        TileGroupManager = { IsOceanTile = function() return false end,
            IsInvalidTile = function() return false end, IsLandTile = function() return true end }
        function GetTileInfo() error("Land colors must not be read as ocean colors") end
        function InGamePlay() return true end
        APP_VERSION = "756039"
        function resolvefilepath_soft(path) return path end
        function GetTimeReal() return 0 end
        TheNet = { GetSessionIdentifier = function() return "fixture-session" end }
        TheWorld = { ismastersim = true, state = { cycles = 2, season = "autumn", phase = "day" }, Map = {} }
        function TheWorld.Map:GetSize() return 3, 2 end
        function TheWorld.Map:GetTile(x, z) return x == 0 and 1 or 2 end
        function TheWorld.Map:GetTileCoordsAtPoint(x, y, z) return math.floor((x + 6) / 4), math.floor((z + 4) / 4) end
        function TheWorld.Map:GetTileCenterPoint(x, y, z)
            local tx, tz = self:GetTileCoordsAtPoint(x, y, z)
            return tx * 4 - 4, 0, tz * 4 - 2
        end
        function landmark(guid, prefab, visible, limbo, valid)
            return { GUID = guid, prefab = prefab, MiniMapEntity = {},
                Transform = { GetWorldPosition = function() return -2, 0, 0 end },
                IsValid = function() return valid end,
                HasTag = function(_, tag) return limbo and tag == "INLIMBO" end,
                entity = { IsVisible = function() return visible end } }
        end
        Ents = { landmark(1, "pigking", true, false, true), landmark(2, "pigking", false, false, true),
            landmark(3, "pigking", true, true, true), landmark(4, "pigking", true, false, false),
            landmark(5, "unrecognized_mod_prefab", true, false, true) }
        Roads = { { 3, { -2, 0 }, { 2, 4 } } }
        AllPlayers = { { userid = "KU_fixture", name = "Player", prefab = "wilson",
            Transform = { GetWorldPosition = function() return -2, 0, 0 end } } }
        function GetPlayerClientTable() return {{userid="KU_fixture"}} end
    "#).exec().unwrap();
    lua.load(setup).exec().unwrap();
    lua.load(query("nonce", terrain).unwrap()).exec().unwrap();
    assert!(
        lua.load("return TheWorld._lgsm_world_map_query == nil")
            .eval::<bool>()
            .unwrap()
    );
    let mut capture = Capture::new("nonce", terrain);
    let mut result = None;
    for line in lines.lock().unwrap().iter() {
        assert!(
            line.len() <= 3900,
            "Native console would truncate this frame"
        );
        if let Some(snapshot) = capture.push(line)? {
            result = Some(snapshot);
        }
    }
    result.ok_or_else(|| "Native fixture emitted no complete snapshot".into())
}

#[test]
fn native_query_emits_real_frame_shapes_without_rewriting_mod_texture_paths() {
    let snapshot = native_query_fixture(true, "").unwrap();
    assert_eq!(
        snapshot.tile_textures[&1],
        "levels/textures/mini_grass_noise.tex"
    );
    assert_eq!(
        snapshot.tile_textures[&2],
        "../mods/example/data/levels/textures/mini_custom.tex"
    );
    let landmarks = snapshot.landmarks.unwrap();
    assert_eq!(landmarks.len(), 1);
    assert_eq!(
        (&*landmarks[0].id, &*landmarks[0].icon),
        ("1", "pigking.png")
    );
    assert_eq!((landmarks[0].map_x, landmarks[0].map_z), (1.0, 1.0));
    assert_eq!(snapshot.roads.unwrap()[0], [[1.0, 1.0], [2.0, 2.0]]);
    assert_eq!(
        (snapshot.players[0].map_x, snapshot.players[0].map_z),
        (1.0, 1.0)
    );
    assert!(!snapshot.landmarks_truncated);
    let positions = native_query_fixture(
        false,
        "Ents = nil; Roads = 'must not inspect'; TileGroupManager = nil; GetTileInfo = nil",
    )
    .unwrap();
    assert!(
        positions.landmarks.is_none()
            && positions.roads.is_none()
            && positions.tile_textures.is_empty()
            && positions.tile_base_textures.is_empty()
            && positions.tile_colors.is_empty()
            && positions.tile_kinds.is_empty()
    );
    assert_eq!(positions.players.len(), 1);
}

#[test]
fn native_query_uses_successful_server_minimap_calls_without_a_client_import() {
    let snapshot = native_query_fixture(true, r#"
        local component = {}
        TheWorld.minimap = {MiniMap=component}
        ModManager = {enabledmods={'a-local-mod'}}
        function ModManager:GetMod() return {modinfo={version='2.0'}} end
        function ModManager:GetPostInitData() error('Actual MiniMap calls must be authoritative') end
        TileGroupManager.IsOceanTile = function(_, id) return id == 2 end
        function GetTileInfo() error('An observed transparent-ocean exclusion must retain its texture') end
        __lgsm_world_map_observer = {version=1,entries={},installedBeforeWorld=true,render={}}
        function __lgsm_world_map_observer.render:report(target, metadata)
            assert(target == component and metadata.session == 'fixture-session')
            assert(metadata.gameVersion == '756039' and metadata.width == 3 and metadata.height == 2)
            assert(#metadata.mods == 1 and metadata.mods[1].name == 'a-local-mod' and metadata.mods[1].version == '2.0')
            local layers={
                {tileId=1,noiseTexture='data/levels/textures/mini_grass_noise.tex'},
                {tileId=1,noiseTexture='../mods/a-local-mod/levels/mini_first.tex'},
                {tileId=1,noiseTexture='../mods/a-local-mod/levels/mini_final.tex'},
                {tileId=2,noiseTexture='../mods/a-local-mod/levels/mini_ocean.tex'}}
            for _, layer in ipairs(layers) do layer.atlas='data/levels/tiles/map_edge.xml'; layer.texture='levels/tiles/map_edge.tex' end
            return {layers=layers,atlases={'data/minimap/minimap_data1.xml','../mods/a-local-mod/minimap/custom.xml',
                    'data/minimap/minimap_data1.xml'},transparentOceanExclusions={2}}
        end
    "#).unwrap();
    assert_eq!(
        snapshot.tile_textures[&1],
        "../mods/a-local-mod/levels/mini_final.tex"
    );
    assert_eq!(
        snapshot.tile_textures[&2],
        "../mods/a-local-mod/levels/mini_ocean.tex"
    );
    assert!(snapshot.tile_colors.is_empty());
    assert_eq!(snapshot.tile_base_textures[&1], "levels/tiles/map_edge.tex");
    assert_eq!(
        snapshot.registered_atlases,
        [
            "minimap/minimap_data1.xml",
            "../mods/a-local-mod/minimap/custom.xml",
            "minimap/minimap_data1.xml"
        ]
    );
    assert!(snapshot.collection_issues.is_empty());
}

#[test]
fn native_custom_materials_retain_actual_applied_order_and_only_used_final_bases() {
    let snapshot = native_query_fixture(true, r#"
        TheWorld.minimap = {MiniMap={}}
        function TheWorld.Map:GetSize() return 40,1 end
        function TheWorld.Map:GetTile(x) return x+1 end
        __lgsm_world_map_observer = {version=1,entries={},installedBeforeWorld=true,render={}}
        function __lgsm_world_map_observer.render:report()
            local layers={}
            for id=1,40 do layers[#layers+1]={tileId=id,atlas='../mods/example/custom.xml',
                texture='../mods/example/custom.tex',noiseTexture='../mods/example/mini_custom.tex'} end
            layers[#layers+1]={tileId=999,atlas='../mods/example/unused.xml',
                texture='../mods/example/unused.tex',noiseTexture='../mods/example/mini_unused.tex'}
            layers[#layers+1]={tileId=1,atlas='levels/tiles/map_edge.xml',
                texture='data/levels/tiles/map_edge.tex',noiseTexture='data/levels/textures/mini_grass_noise.tex'}
            return {layers=layers,atlases={'minimap/minimap_data1.xml'}}
        end
    "#).unwrap();
    // Asset availability belongs to the host's real material loader, rather
    // than a blanket assumption that every custom edge is unsupported.
    assert!(snapshot.collection_issues.is_empty());
    assert_eq!(snapshot.terrain_layers.len(), 42);
    assert_eq!(snapshot.terrain_layers[0].tile_id, 1);
    assert_eq!(snapshot.terrain_layers[40].tile_id, 999);
    assert_eq!(snapshot.terrain_layers[41].tile_id, 1);
    assert_eq!(
        snapshot.terrain_layers[41].texture.as_deref(),
        Some("levels/tiles/map_edge.tex")
    );
    assert!(
        snapshot
            .terrain_layers
            .iter()
            .all(|layer| layer.sample_style.is_none())
    );
    assert_eq!(
        snapshot.tile_textures[&1],
        "levels/textures/mini_grass_noise.tex"
    );
    assert_eq!(snapshot.tile_base_textures[&1], "levels/tiles/map_edge.tex");
    assert_eq!(
        snapshot.tile_base_textures[&2],
        "../mods/example/custom.tex"
    );
    assert!(!snapshot.tile_base_textures.contains_key(&999));
}

#[test]
fn native_bad_material_fields_are_isolated_from_other_tiles_geometry_and_players() {
    for bad in [
        "false",
        "42",
        "string.rep('x',513)",
        "'bad\\001path'",
        "string.char(255)",
    ] {
        let setup = format!(
            r#"
            TheWorld.minimap = {{MiniMap={{}}}}
            __lgsm_world_map_observer = {{version=1,entries={{}},installedBeforeWorld=true,render={{}}}}
            function __lgsm_world_map_observer.render:report()
                return {{atlases={{}},layers={{
                    {{tileId=1,atlas='levels/tiles/map_edge.xml',texture={bad},noiseTexture={bad}}},
                    {{tileId=2,atlas='../mods/example/map.xml',texture='../mods/example/base.tex',noiseTexture='../mods/example/noise.tex'}}
                }}}}
            end
        "#
        );
        let snapshot = native_query_fixture(true, &setup).unwrap();
        assert_eq!(snapshot.tiles.as_ref().unwrap().len(), 6);
        assert_eq!(snapshot.players.len(), 1);
        assert!(!snapshot.tile_textures.contains_key(&1));
        assert!(!snapshot.tile_base_textures.contains_key(&1));
        assert_eq!(snapshot.tile_textures[&2], "../mods/example/noise.tex");
        assert_eq!(snapshot.tile_base_textures[&2], "../mods/example/base.tex");
        assert_eq!(snapshot.terrain_layers.len(), 2);
        assert!(snapshot.terrain_layers[0].texture.is_none());
        assert!(snapshot.terrain_layers[0].noise_texture.is_none());
        assert!(
            snapshot
                .collection_issues
                .iter()
                .any(|issue| issue.contains("invalid terrain material fields"))
        );
    }
}

#[test]
fn actual_observer_and_query_isolate_a_broken_final_layer_and_uncertain_atlas_order() {
    let source =
        include_str!("../../../../modules/dontstarve/world_map_shared/render_observer.lua");
    for bad_atlas in [false, true] {
        let setup = format!(
            r#"
            unpack = table.unpack
            local module = (function() {source} end)()
            local group_methods = {{}}
            for _, name in ipairs({{'AddInvalidTile','RemoveInvalidTile','AddValidTile',
                'RemoveValidTile','SetValidTileRange','SetIsTransparentOceanTileGroup'}}) do
                group_methods[name] = function() return true end
            end
            setmetatable(TileGroupManager, {{__index=group_methods}})
            TileGroups = {{TransparentOceanTiles=100}}
            local handle = 0
            local manager = setmetatable({{}}, {{__index={{CreateRenderLayer=function()
                handle = handle + 1; return handle end}}}})
            local mini = setmetatable({{}}, {{__index={{AddRenderLayer=function() return true end,
                AddAtlas=function() return true end}}}})
            TheWorld.minimap = {{MiniMap=mini}}
            local render = module.new(_G)
            render:check_load_order({{{{modname='LGSM'}}}},'LGSM')
            render:attach_tile_groups(TileGroupManager,TileGroups.TransparentOceanTiles)
            render:attach_manager(manager); render:attach_minimap(mini)
            mini:AddRenderLayer(manager:CreateRenderLayer(1,'levels/tiles/map_edge.xml',
                'levels/tiles/map_edge.tex','levels/textures/mini_grass_noise.tex'))
            mini:AddRenderLayer(manager:CreateRenderLayer(2,'../mods/example/map.xml',
                '../mods/example/base.tex','../mods/example/noise.tex'))
            mini:AddRenderLayer(manager:CreateRenderLayer(1,'levels/tiles/map_edge.xml',
                string.rep('x',513),string.char(255)))
            mini:AddAtlas('minimap/minimap_data1.xml')
            if {bad_atlas} then mini:AddAtlas(string.char(255)) end
            mini:AddAtlas('../mods/example/later.xml')
            __lgsm_world_map_observer = {{version=1,installedBeforeWorld=true,entries={{}},render=render}}
        "#
        );
        let snapshot = native_query_fixture(true, &setup).unwrap();
        assert_eq!(snapshot.tiles.as_ref().unwrap().len(), 6);
        assert_eq!(snapshot.players.len(), 1);
        assert!(!snapshot.tile_textures.contains_key(&1));
        assert!(!snapshot.tile_base_textures.contains_key(&1));
        assert_eq!(snapshot.tile_textures[&2], "../mods/example/noise.tex");
        assert_eq!(snapshot.tile_base_textures[&2], "../mods/example/base.tex");
        assert!(
            snapshot
                .collection_issues
                .iter()
                .any(|issue| issue.contains("invalid native material fields"))
        );
        assert_eq!(snapshot.icons_uncertain, bad_atlas);
        assert_eq!(snapshot.registered_atlases.is_empty(), bad_atlas);
        if bad_atlas {
            assert!(
                snapshot
                    .collection_issues
                    .iter()
                    .any(|issue| issue.contains("Icon registration is uncertain"))
            );
        }
        assert!(
            serde_json::to_value(snapshot)
                .unwrap()
                .get("iconsUncertain")
                .is_none()
        );
    }
}

#[test]
fn native_material_observation_failure_retains_standard_maps_and_reports_the_gap() {
    for failure in [
        "return nil, 'An unobserved native handle was applied.'",
        "error('native-report-failure')",
    ] {
        let snapshot = native_query_fixture(true, &format!(r#"
            TheWorld.minimap = {{MiniMap={{}}}}
            ModManager = {{enabledmods={{}}}}
            function ModManager:GetPostInitData() return {{{{'data/minimap/minimap_data1.xml'}}}} end
            __lgsm_world_map_observer = {{version=1,entries={{}},installedBeforeWorld=true,render={{}}}}
            function __lgsm_world_map_observer.render:report() {failure} end
        "#)).unwrap();
        assert_eq!(
            snapshot.tile_textures[&1],
            "levels/textures/mini_grass_noise.tex"
        );
        assert_eq!(snapshot.registered_atlases, ["minimap/minimap_data1.xml"]);
        assert_eq!(snapshot.collection_issues.len(), 1);
        assert!(
            snapshot.collection_issues[0]
                .contains("Native map material observation is unavailable")
        );
    }
}

#[test]
fn native_query_uses_runtime_mod_registration_and_observed_icons() {
    let snapshot = native_query_fixture(true, r#"
        ModManager = { enabledmods = { 'workshop-123', 'a-local-mod' } }
        function ModManager:GetMod(name) return {modinfo={version='1.0',all_clients_require_mod=name=='workshop-123'}} end
        KnownModIndex = {GetModInfo=function() error('Must use loaded Mod metadata') end}
        function ModManager:GetPostInitData(kind)
            assert(kind == 'MinimapAtlases')
            return {{ 'images/custom.xml', 'images/custom.xml' }}
        end
        function resolvefilepath_soft(path)
            if path == 'images/custom.xml' then return '../mods/workshop-123/images/custom.xml' end
            if path == 'data/levels/textures/mini_grass_noise.tex' then return '../mods/a-local-mod/levels/mini_override.tex' end
            return path
        end
        local entries = {}
        __lgsm_world_map_observer = {version=1,entries=entries,installedBeforeWorld=true}
        entries[Ents[1].MiniMapEntity] = {icon='changed-native.tex',iconObserved=true,priority=12}
        entries[Ents[5].MiniMapEntity] = {icon='custom-building.tex',iconObserved=true,priority=8}
        AllPlayers[1].MiniMapEntity = {}
        entries[AllPlayers[1].MiniMapEntity] = {icon='custom-player.tex',iconObserved=true}
    "#).unwrap();
    assert_eq!(snapshot.enabled_mods, ["workshop-123", "a-local-mod"]);
    assert_eq!(snapshot.mod_versions["workshop-123"], "1.0");
    assert_eq!(
        snapshot.client_required_mods,
        BTreeSet::from(["workshop-123".into()])
    );
    assert_eq!(
        snapshot.registered_atlases,
        ["../mods/workshop-123/images/custom.xml"]
    );
    assert_eq!(
        snapshot.tile_textures[&1],
        "../mods/a-local-mod/levels/mini_override.tex"
    );
    assert!(snapshot.collection_issues.is_empty());
    let landmarks = snapshot.landmarks.unwrap();
    assert_eq!(landmarks.len(), 2);
    assert!(
        landmarks
            .iter()
            .any(|item| item.icon == "custom-building.tex" && item.priority == 8.0)
    );
    assert_eq!(
        snapshot.players[0].icon.as_deref(),
        Some("custom-player.tex")
    );
    let disabled = native_query_fixture(true, "__lgsm_world_map_observer={version=1,installedBeforeWorld=true,entries={[Ents[1].MiniMapEntity]={icon='pigking.png',iconObserved=true,enabled=false}}}").unwrap();
    assert!(disabled.landmarks.unwrap().is_empty());
}

#[test]
fn native_query_excludes_disconnected_characters_but_keeps_current_shard_players() {
    for terrain in [true, false] {
        let snapshot = native_query_fixture(
            terrain,
            r#"
            AllPlayers[2] = { userid='KU_offline', name='Disconnected character', prefab='willow',
                Transform={GetWorldPosition=function() return 1,0,2 end} }
            function GetPlayerClientTable()
                return {{}, {userid=''}, {userid='KU_other_shard'}, {userid='KU_fixture'}}
            end
        "#,
        )
        .unwrap();
        assert_eq!(snapshot.players.len(), 1);
        assert_eq!(snapshot.players[0].id, "KU_fixture");
        let empty =
            native_query_fixture(terrain, "function GetPlayerClientTable() return {} end").unwrap();
        assert!(empty.players.is_empty());
    }
}

#[test]
fn native_query_treats_an_explicitly_empty_icon_as_hidden_without_falling_back() {
    let snapshot = native_query_fixture(
        true,
        r#"
        local entries = {}
        __lgsm_world_map_observer = {version=1,entries=entries,installedBeforeWorld=true}
        entries[Ents[1].MiniMapEntity] = {icon='',iconObserved=true}
        AllPlayers[1].MiniMapEntity = {}
        entries[AllPlayers[1].MiniMapEntity] = {icon='',iconObserved=true}
    "#,
    )
    .unwrap();
    assert!(snapshot.landmarks.unwrap().is_empty());
    assert_eq!(snapshot.players.len(), 1);
    assert!(snapshot.players[0].icon.is_none());
}

#[test]
fn native_query_resolves_registered_ground_names_without_overriding_native_names() {
    let snapshot = native_query_fixture(
        true,
        r#"
        INVERTED_WORLD_TILES = { [1] = 'NATIVE_GRASS' }
        function GetWorldTileMap() return { DIFFERENT_GRASS=1, MOD_GROUND=2, MOD_ALIAS=2 } end
    "#,
    )
    .unwrap();
    assert_eq!(snapshot.tile_names[&1], "NATIVE_GRASS");
    assert_eq!(snapshot.tile_names[&2], "MOD_ALIAS");
}

#[test]
fn native_terrain_groups_override_labels_for_fake_ground_ocean_ice_and_dynamic_tiles() {
    // 756039 registers FAKE_GROUND in IMPASSABLE and OCEAN_ICE in LAND.
    // Other IDs exercise arbitrary Mod labels and the unclassified-ID case.
    let snapshot = native_query_fixture(
        true,
        r#"
        local ids={200,264,12000,512,65000,60000}
        INVERTED_WORLD_TILES={ [200]='FAKE_GROUND', [264]='OCEAN_ICE',
            [12000]='DRY_GARDEN', [512]='DEEP_WATER', [60000]='FLOWER_FIELD' }
        function TheWorld.Map:GetTile(x,z) return ids[z*3+x+1] end
        function TileGroupManager:IsInvalidTile(id) return id == 200 or id == 60000 end
        function TileGroupManager:IsOceanTile(id) return id == 12000 or id == 60000 end
        function TileGroupManager:IsLandTile(id) return id == 264 or id == 512 end
        function GetTileInfo(id)
            assert(id == 12000, 'Only classified ocean without an exclusion reads ocean colors')
            return {colors={minimap_color={2,4,6,128}}}
        end
    "#,
    )
    .unwrap();
    assert_eq!(
        snapshot.tile_kinds,
        BTreeMap::from([
            (200, DstTerrainKind::Void),
            (264, DstTerrainKind::Land),
            (12000, DstTerrainKind::Ocean),
            (512, DstTerrainKind::Land),
            (65000, DstTerrainKind::Unknown),
            (60000, DstTerrainKind::Void),
        ])
    );
    assert_eq!(snapshot.tile_names[&65000], "65000");
    assert_eq!(
        snapshot.tile_colors,
        BTreeMap::from([(12000, [2, 4, 6, 128])])
    );
    let wire = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(wire["tileKinds"]["200"], "void");
    assert_eq!(wire["tileKinds"]["264"], "land");
}

#[test]
fn terrain_notices_separate_unknown_classification_from_missing_materials() {
    let mut snapshot = native_query_fixture(true, "").unwrap();
    snapshot.tiles = Some(vec![200, 264, 264, 12000, 65000, 700]);
    snapshot.tile_kinds = BTreeMap::from([
        (200, DstTerrainKind::Void),
        (264, DstTerrainKind::Land),
        (12000, DstTerrainKind::Ocean),
        (65000, DstTerrainKind::Unknown),
        (700, DstTerrainKind::Land),
    ]);
    snapshot.tile_textures = BTreeMap::from([
        (65000, "../mods/example/arbitrary.tex".into()),
        // Missing/corrupt requested artwork already has its loader notice.
        (700, "../mods/example/missing.tex".into()),
    ]);
    snapshot.tile_colors = BTreeMap::from([(12000, [1, 2, 3, 128])]);
    let issues = terrain_material_issues(&snapshot);
    assert_eq!(issues.len(), 2);
    assert!(issues[0].contains("unknown for 1 tile types (65000)"));
    assert!(!issues[0].contains("schematic"));
    assert!(issues[1].contains("1 tile types (264); 2 map tiles use schematic colors"));
    assert!(!issues[1].contains("200"));
    assert!(!issues[1].contains("700"));
    snapshot.tiles = None;
    assert!(terrain_material_issues(&snapshot).is_empty());
}

#[test]
fn terrain_material_notices_bound_samples_but_count_all_used_tiles() {
    let mut snapshot = native_query_fixture(true, "").unwrap();
    snapshot.tiles = Some((300..340).chain(std::iter::repeat_n(300, 10)).collect());
    snapshot.tile_kinds = (300..340).map(|id| (id, DstTerrainKind::Land)).collect();
    snapshot.tile_textures.clear();
    snapshot.tile_colors.clear();
    let issues = terrain_material_issues(&snapshot);
    assert_eq!(issues.len(), 1);
    assert!(issues[0].contains("40 tile types (300,301,302,303,304,305,306,307)"));
    assert!(issues[0].contains("50 map tiles use schematic colors"));
    assert!(issues[0].len() < 256);
}

#[test]
fn native_mod_frame_requires_a_boolean_client_installation_flag() {
    for required in [serde_json::Value::Null, json!("false"), json!(1)] {
        let mut capture = Capture::new("nonce", true);
        begin(&mut capture);
        assert!(
            push(
                &mut capture,
                json!({"k":"mod","name":"workshop-1","version":"1.0","requiredOnClient":required})
            )
            .is_err()
        );
    }
}

#[test]
fn native_query_chunks_long_roads_without_losing_precision_or_order() {
    let snapshot = native_query_fixture(
        true,
        "local road={3}; for i=1,256 do road[#road+1]={i/7,i/11} end; Roads={road}",
    )
    .unwrap();
    assert_eq!(snapshot.roads.as_ref().unwrap()[0].len(), 256);
    assert!((snapshot.roads.unwrap()[0][255][0] - (256.0 / 7.0 + 6.0) / 4.0).abs() < 1e-9);
    for value in [
        json!({"k":"road","index":1,"offset":0,"complete":true,"points":[[0,0],[1,1]]}),
        json!({"k":"road","index":0,"offset":1,"complete":true,"points":[[0,0],[1,1]]}),
        json!({"k":"road","index":0,"offset":0,"complete":true,"points":[[0,0]]}),
    ] {
        let mut capture = Capture::new("nonce", true);
        begin(&mut capture);
        assert!(push(&mut capture, value).is_err());
    }
    let mut capture = Capture::new("nonce", true);
    begin(&mut capture);
    push(
        &mut capture,
        json!({"k":"road","index":0,"offset":0,"complete":false,"points":[[0,0],[1,1]]}),
    )
    .unwrap();
    assert!(push(&mut capture, json!({"k":"end","players":0,"roads":0})).is_err());
}

#[test]
fn native_query_reads_minimap_colors_only_for_engine_classified_ocean_tiles() {
    let setup = "function TileGroupManager:IsOceanTile(id) return id == 2 end; function GetTileInfo(id) assert(id == 2); return { colors = { minimap_color = { 3, 87, 162, 204 } } } end";
    let snapshot = native_query_fixture(true, setup).unwrap();
    assert_eq!(
        snapshot.tile_colors,
        BTreeMap::from([(2, [3, 87, 162, 204])])
    );
    let missing = native_query_fixture(true, "function TileGroupManager:IsOceanTile(id) return id == 2 end; function GetTileInfo() return {} end").unwrap();
    assert!(missing.tile_colors.is_empty());
    for color in [
        "{0,0,0}",
        "{0,0,0,255,1}",
        "{-1,0,0,255}",
        "{0,256,0,255}",
        "{0,0.5,0,255}",
        "{0,'0',0,255}",
        "{0,0/0,0,255}",
    ] {
        let setup = format!(
            "function TileGroupManager:IsOceanTile(id) return id == 2 end; function GetTileInfo() return {{ colors = {{ minimap_color = {color} }} }} end"
        );
        let snapshot = native_query_fixture(true, &setup).unwrap();
        assert!(snapshot.tile_colors.is_empty());
        assert_eq!(snapshot.tiles.as_ref().unwrap().len(), 6);
        assert_eq!(snapshot.players.len(), 1);
        assert!(
            snapshot
                .collection_issues
                .iter()
                .any(|issue| issue.contains("invalid terrain material fields"))
        );
    }
}

#[test]
fn native_query_truncates_player_text_at_utf8_boundaries() {
    let snapshot =
        native_query_fixture(false, "AllPlayers[1].name = string.rep('中文🙂', 80)").unwrap();
    assert!(snapshot.players[0].name.len() <= 256);
    assert!(snapshot.players[0].name.starts_with("中文🙂"));
    assert!(!snapshot.players[0].name.contains('\u{fffd}'));
}

#[test]
fn native_query_uses_external_json_for_quoted_player_names() {
    let snapshot = native_query_fixture(
        false,
        r#"AllPlayers[1].name = "O'Brien\\path\n第二行\t\"quoted\"""#,
    )
    .unwrap();
    assert_eq!(snapshot.players[0].name, "O'Brien\\path 第二行 \"quoted\"");
}

#[test]
fn native_entity_limits_are_explicitly_truncated_and_road_limits_are_errors() {
    for (setup, count) in [
        (
            "Ents = {}; local invalid = landmark(1, 'pigking', true, false, false); for i=1,100001 do Ents[i]=invalid end",
            0,
        ),
        (
            "Ents = {}; for i=1,32769 do Ents[i]=landmark(i, 'pigking', true, false, true) end",
            32768,
        ),
    ] {
        let snapshot = native_query_fixture(true, setup).unwrap();
        assert_eq!(snapshot.landmarks.unwrap().len(), count);
        assert!(snapshot.landmarks_truncated);
    }
    for (setup, error) in [
        ("Roads = {{3, {1,2}}}", "2 to 256 control points"),
        (
            "Roads = {{3, {1,2}, {3}}}",
            "Invalid native road control point",
        ),
        (
            "Roads = {}; for i=1,1025 do Roads[i]={3,{1,2},{3,4}} end",
            "1024-road limit",
        ),
        (
            "local road={3}; for i=1,256 do road[i+1]={1,2} end; Roads={}; for i=1,257 do Roads[i]=road end",
            "65536-point limit",
        ),
    ] {
        assert!(
            native_query_fixture(true, setup)
                .unwrap_err()
                .contains(error)
        );
    }
}
