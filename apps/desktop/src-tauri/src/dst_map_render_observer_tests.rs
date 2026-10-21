use mlua::{Lua, Table};

const OBSERVER: &str =
    include_str!("../../../../modules/dontstarve/world_map_shared/render_observer.lua");

fn fixture() -> Lua {
    let lua = Lua::new();
    lua.load(
        r#"
        unpack = table.unpack
        changes = 0
        manager_index = {}
        next_handle = 0
        function manager_index:CreateRenderLayer(tile, atlas, texture, noise, extra)
            if tile == -1 then error('native-error') end
            next_handle = next_handle + 1
            return next_handle, nil, extra, nil
        end
        manager = setmetatable({}, {__index=manager_index})
        function manager_index:SetSampleStyle(style, extra)
            if style == -1 then error('native-style-error') end
            return 'style', nil, extra, nil
        end
        mini_index = {}
        function mini_index:AddRenderLayer(handle, extra)
            if handle == -1 then error('native-add-error') end
            return 'applied', nil, extra, nil
        end
        function mini_index:AddAtlas(path, extra) return 'atlas', nil, extra end
        mini = setmetatable({}, {__index=mini_index})
        group_index = {}
        for _, name in ipairs({'AddInvalidTile','RemoveInvalidTile','AddValidTile',
                'RemoveValidTile','SetValidTileRange','SetIsTransparentOceanTileGroup'}) do
            local operation = name
            group_index[name] = function(_,group,tile,extra)
                if group == -1 or tile == -1 then error('native-group-error') end
                return operation, nil, extra, nil
            end
        end
        TileGroupManager = setmetatable({}, {__index=group_index})
        TileGroups = {TransparentOceanTiles=100}
        meta = {gameVersion='756039',session='ABC123',width=378,height=378,
            mods={{name='workshop-1505270912',version='1.2'}}}
    "#,
    )
    .exec()
    .unwrap();
    let module: Table = lua.load(OBSERVER).eval().unwrap();
    lua.globals().set("observer_module", module).unwrap();
    lua.load(
        r#"
        observer = observer_module.new(_G, function() changes = changes + 1 end)
        observer:check_load_order({{modname='collector'}}, 'collector')
        observer:attach_tile_groups(TileGroupManager, TileGroups.TransparentOceanTiles)
        observer:attach_manager(manager)
        observer:attach_minimap(mini)
    "#,
    )
    .exec()
    .unwrap();
    lua
}

#[test]
fn inventory_getters_preserve_explicit_strings_and_verify_hashes_without_prefab_guessing() {
    fixture().load(r#"
        function resolvefilepath(path)
            if path == 'images/item.xml' then return '../mods/enabled/images/item.xml' end
            return path
        end
        local hashes = {['../mods/enabled/images/item.xml']=41,['declared.tex']=42,['override.tex']=43}
        function hash(value) return hashes[value] or 0 end
        local item={atlasname='images/item.xml',imagename='declared'}
        local replica={GetImage=function() return 'actual-different.tex' end,
            GetAtlas=function() return '../mods/enabled/images/item.xml' end}
        local entity={prefab='must-not-be-used',components={inventoryitem=item},replica={inventoryitem=replica}}
        local source=assert(observer:inventory_icon(entity))
        assert(source.element=='actual-different.tex' and source.atlas=='../mods/enabled/images/item.xml')
        assert(observer:inventory_icon({})==nil)
        local allowance={bytes=0}
        assert(observer:inventory_icon(entity,allowance) and allowance.bytes>0 and allowance.bytes<1048576)
        replica.GetImage=function() return 42 end; replica.GetAtlas=function() return 41 end
        source=assert(observer:inventory_icon(entity))
        assert(source.element=='declared.tex' and item.imagename=='declared' and item.atlasname=='images/item.xml')
        replica.overrideimage='override.tex'; replica.GetImage=function() return 43 end
        assert(observer:inventory_icon(entity).element=='override.tex')
        replica.GetImage=function() return 99 end
        assert(observer:inventory_icon(entity)==nil)
        replica.GetImage=function() return 42 end; item.imagename=nil
        assert(observer:inventory_icon(entity)==nil)
        item.imagename='declared'; item.atlasname=nil
        assert(observer:inventory_icon(entity)==nil)
        replica.GetAtlas=function() return 'data/images/native.xml' end
        replica.GetImage=function() return 'native.tex' end
        assert(observer:inventory_icon(entity).atlas=='images/native.xml')
        local budget={bytes=1048576}
        assert(observer:inventory_icon(entity,budget)==nil and budget.bytes==1048576)
        replica.GetImage=function() error('broken Mod getter') end
        assert(observer:inventory_icon(entity)==nil)
        replica.GetImage=function() return string.rep('x',129) end
        assert(observer:inventory_icon(entity)==nil)
        replica.GetImage=function() return 'bad\0name.tex' end
        assert(observer:inventory_icon(entity)==nil)
        replica.GetImage=function() return 'still-valid.tex' end
        assert(observer:inventory_icon(entity).element=='still-valid.tex')
        assert(observer.issue==nil and changes==0)
    "#).exec().unwrap();
}

#[test]
fn preserves_native_returns_errors_and_actual_application_order() {
    fixture()
        .load(
            r#"
        observer:attach_manager(manager)
        observer:attach_minimap(mini)
        local a = table.pack(manager:CreateRenderLayer(7,'a.xml','a.tex','a_noise.tex',42))
        assert(a.n == 4 and a[2] == nil and a[3] == 42 and a[4] == nil)
        local b = manager:CreateRenderLayer(7,'b.xml','b.tex','b_noise.tex')
        local unused = manager:CreateRenderLayer(99,'unused','unused','unused')
        local r = table.pack(mini:AddRenderLayer(b, 71))
        assert(r.n == 4 and r[1] == 'applied' and r[3] == 71)
        mini:AddRenderLayer(a[1])
        mini:AddRenderLayer(b)
        mini:AddAtlas('first.xml')
        mini:AddAtlas('../mods/workshop-1505270912/map.xml')
        mini:AddAtlas('first.xml')
        local report = assert(observer:report(mini, meta))
        assert(#report.layers == 3 and report.layers[1].texture == 'b.tex')
        assert(report.layers[2].texture == 'a.tex' and report.layers[3].texture == 'b.tex')
        assert(#report.atlases == 3 and report.atlases[3] == 'first.xml')
        assert(changes == 6)
        local ok, err = pcall(manager.CreateRenderLayer, manager, -1)
        assert(not ok and string.find(err,'native%-error'))
        ok, err = pcall(mini.AddRenderLayer, mini, -1)
        assert(not ok and string.find(err,'native%-add%-error'))
        assert(#assert(observer:report(mini,meta)).layers == 3)
    "#,
        )
        .exec()
        .unwrap();
}

#[test]
fn sample_style_tracks_successful_actual_manager_calls_without_inventing_defaults() {
    fixture().load(r#"
        mini:AddAtlas('a.xml')
        mini:AddRenderLayer(manager:CreateRenderLayer(7,'a.xml','a.tex','n.tex'))
        assert(assert(observer:report(mini,meta)).layers[1].sampleStyle == nil)
        local result=table.pack(manager:SetSampleStyle(0,41))
        assert(result.n==4 and result[1]=='style' and result[2]==nil and result[3]==41 and result[4]==nil)
        assert(assert(observer:report(mini,meta)).layers[1].sampleStyle==0)
        manager:SetSampleStyle(1)
        local other=setmetatable({}, {__index=manager_index})
        mini:AddRenderLayer(other:CreateRenderLayer(8,'b.xml','b.tex','n.tex'))
        local report=assert(observer:report(mini,meta))
        assert(report.layers[1].sampleStyle==1 and report.layers[2].sampleStyle==nil)
        local ok,err=pcall(manager.SetSampleStyle,manager,-1)
        assert(not ok and string.find(err,'native%-style%-error'))
        assert(assert(observer:report(mini,meta)).layers[1].sampleStyle==1)
        other:SetSampleStyle('accepted-but-unknown')
        report=assert(observer:report(mini,meta))
        assert(report.layers[1].sampleStyle==1 and report.layers[2].sampleStyle==nil)
        assert(report.layers[2].atlas==nil and report.layers[2].texture=='b.tex' and report.layers[2].noiseTexture=='n.tex')
        assert(string.find(report.issue,'invalid native material'))
        assert(string.find(report.issue,'Unsupported native sample style'))
        other:SetSampleStyle(300)
        report=assert(observer:report(mini,meta))
        assert(report.layers[2].atlas==nil and report.layers[2].sampleStyle==nil)
        other:SetSampleStyle(0)
        report=assert(observer:report(mini,meta))
        assert(report.layers[2].atlas=='b.xml' and report.layers[2].sampleStyle==0)
    "#).exec().unwrap();
}

#[test]
fn rejects_unknown_handles_invalid_fields_and_bounded_observation() {
    fixture()
        .load(
            r#"
        mini:AddAtlas('a.xml')
        mini:AddRenderLayer(999)
        local report, err = observer:report(mini, meta)
        assert(not report and string.find(err, 'unobserved'))
    "#,
        )
        .exec()
        .unwrap();
    fixture()
        .load(
            r#"
        local h = manager:CreateRenderLayer(7,'a.xml','a.tex',string.rep('x',513))
        mini:AddRenderLayer(h); mini:AddAtlas('a.xml')
        local report = assert(observer:report(mini,meta))
        assert(#report.layers == 1 and report.layers[1].tileId == 7)
        assert(report.layers[1].texture == 'a.tex' and report.layers[1].noiseTexture == nil)
        assert(string.find(report.issue,'invalid native material'))
        assert(observer.issue == nil)
    "#,
        )
        .exec()
        .unwrap();
    fixture()
        .load(
            r#"
        local h = manager:CreateRenderLayer(7,'a.xml','a.tex','n.tex')
        mini:AddRenderLayer(h)
        for i=1,513 do mini:AddAtlas('a.xml') end
        local report, err = observer:report(mini,meta)
        assert(not report and string.find(err,'exceed 512'))
    "#,
        )
        .exec()
        .unwrap();
}

#[test]
fn invalid_atlas_order_disables_only_icon_artwork_and_does_not_poison_later_reports() {
    for bad in ["string.rep('x',513)", "'bad\\001path'", "string.char(255)"] {
        fixture()
            .load(format!(
                r#"
            local h = manager:CreateRenderLayer(7,'a.xml','a.tex','n.tex')
            mini:AddRenderLayer(h)
            mini:AddAtlas('first.xml'); mini:AddAtlas({bad}); mini:AddAtlas('later.xml')
            local report = assert(observer:report(mini,meta))
            assert(report.iconsUncertain and #report.atlases == 0)
            assert(#report.layers == 1 and report.layers[1].noiseTexture == 'n.tex')
            assert(string.find(report.issue,'Icon registration is uncertain'))
            assert(observer.issue == nil)
        "#
            ))
            .exec()
            .unwrap();
    }
}

#[test]
fn keeps_independent_minimaps_and_requires_actual_world_identity() {
    fixture()
        .load(
            r#"
        local other = setmetatable({}, {__index=mini_index})
        local a = manager:CreateRenderLayer(1,'a.xml','a.tex','n.tex')
        local b = manager:CreateRenderLayer(2,'b.xml','b.tex','m.tex')
        mini:AddRenderLayer(a); mini:AddAtlas('a.xml')
        other:AddRenderLayer(b); other:AddAtlas('b.xml')
        assert(assert(observer:report(mini,meta)).layers[1].tileId == 1)
        assert(assert(observer:report(other,meta)).layers[1].tileId == 2)
        meta.session = ''; assert(not observer:report(mini,meta))
        meta.session = 'ABC123'; meta.gameVersion = 'DEV_UNKNOWN'
        assert(not observer:report(mini,meta))
        meta.gameVersion = '756039'; meta.mods[1].version = nil
        assert(not observer:report(mini,meta))
        meta.mods[1].version = '1.2'; meta.width=2049; assert(not observer:report(mini,meta))
        meta.width=2048; meta.height=1024; assert(not observer:report(mini,meta))
    "#,
        )
        .exec()
        .unwrap();
}

#[test]
fn constructor_hooks_observe_before_prefab_applies_materials() {
    fixture()
        .load(
            r#"
        Ents = {}
        local index = {}
        function index:GetGUID() return self.guid end
        function index:AddMapLayerManager() return manager, nil, 'extra', nil end
        function index:AddMiniMap()
            Ents[self.guid].MiniMap = setmetatable({}, {__index=mini_index})
            return 'added', nil, 17
        end
        local inst = {entity=setmetatable({guid=8},{__index=index})}
        Ents[8] = inst
        observer:attach_entity(inst)
        observer:attach_entity(inst)
        local ret = table.pack(inst.entity:AddMapLayerManager())
        assert(ret.n == 4 and ret[1] == manager and ret[3] == 'extra')
        ret = table.pack(inst.entity:AddMiniMap())
        assert(ret.n == 3 and ret[1] == 'added' and ret[3] == 17)
        local handle = manager:CreateRenderLayer(5,'map.xml','map.tex','noise.tex')
        inst.MiniMap:AddRenderLayer(handle)
        inst.MiniMap:AddAtlas('icons.xml')
        assert(#assert(observer:report(inst.MiniMap,meta)).layers == 1)
    "#,
        )
        .exec()
        .unwrap();
}

#[test]
fn observer_failures_do_not_replace_successful_native_results() {
    fixture()
        .load(
            r#"
        local failing = observer_module.new(_G, function() error('observer-failed') end)
        failing:attach_manager(manager); failing:attach_minimap(mini)
        local h = manager:CreateRenderLayer(1,'a.xml','a.tex','noise.tex')
        local r = table.pack(mini:AddRenderLayer(h, 42))
        assert(r.n == 4 and r[1] == 'applied' and r[3] == 42)
        local report, err = failing:report(mini,meta)
        assert(not report and string.find(err,'observer%-failed'))
    "#,
        )
        .exec()
        .unwrap();
}

#[test]
fn transparent_ocean_exclusions_follow_successful_native_mutations() {
    fixture().load(r#"
        mini:AddRenderLayer(manager:CreateRenderLayer(7,'a.xml','a.tex','n.tex'))
        mini:AddAtlas('a.xml')
        assert(assert(observer:report(mini,meta)).transparentOceanExclusions == nil)
        observer:attach_tile_groups(TileGroupManager,100) -- idempotent
        local ret = table.pack(TileGroupManager:AddInvalidTile(100,202,42))
        assert(ret.n == 4 and ret[1] == 'AddInvalidTile' and ret[3] == 42 and ret[4] == nil)
        TileGroupManager:AddInvalidTile(100,200)
        TileGroupManager:AddInvalidTile(100,202) -- duplicate is not another entry
        TileGroupManager:AddInvalidTile(101,300) -- another group is independent
        TileGroupManager:AddValidTile(101,300)
        TileGroupManager:SetIsTransparentOceanTileGroup(100) -- same group remains known
        local report = assert(observer:report(mini,meta))
        assert(#report.transparentOceanExclusions == 2)
        assert(report.transparentOceanExclusions[1] == 200 and report.transparentOceanExclusions[2] == 202)
        local ok, err = pcall(TileGroupManager.AddInvalidTile,TileGroupManager,100,-1)
        assert(not ok and string.find(err,'native%-group%-error'))
        assert(#assert(observer:report(mini,meta)).transparentOceanExclusions == 2)
        ret = table.pack(TileGroupManager:RemoveInvalidTile(100,200,51))
        assert(ret.n == 4 and ret[1] == 'RemoveInvalidTile' and ret[3] == 51)
        assert(assert(observer:report(mini,meta)).transparentOceanExclusions[1] == 202)
        TileGroupManager:RemoveInvalidTile(100,202)
        assert(assert(observer:report(mini,meta)).transparentOceanExclusions == nil)
    "#).exec().unwrap();
}

#[test]
fn transparent_ocean_unknown_membership_changes_fail_export_not_native_calls() {
    for method in ["AddValidTile", "RemoveValidTile", "SetValidTileRange"] {
        let lua = fixture();
        lua.globals().set("mutation_method", method).unwrap();
        lua.load(
            r#"
            mini:AddRenderLayer(manager:CreateRenderLayer(7,'a.xml','a.tex','n.tex'))
            mini:AddAtlas('a.xml')
            local ret = table.pack(TileGroupManager[mutation_method](TileGroupManager,100,200,42))
            assert(ret.n == 4 and ret[1] == mutation_method and ret[3] == 42)
            local report, err = observer:report(mini,meta)
            assert(not report and string.find(err,'unsupported membership'))
        "#,
        )
        .exec()
        .unwrap();
    }
    fixture()
        .load(
            r#"
        local ret = table.pack(TileGroupManager:SetIsTransparentOceanTileGroup(101,nil,42))
        assert(ret.n == 4 and ret[1] == 'SetIsTransparentOceanTileGroup' and ret[3] == 42)
        local report, err = observer:report(mini,meta)
        assert(not report and string.find(err,'replaced'))
    "#,
        )
        .exec()
        .unwrap();
}

#[test]
fn transparent_ocean_observation_bounds_unique_entries_and_rejects_invalid_ids() {
    fixture()
        .load(
            r#"
        mini:AddRenderLayer(manager:CreateRenderLayer(7,'a.xml','a.tex','n.tex'))
        mini:AddAtlas('a.xml')
        for tile=0,4095 do TileGroupManager:AddInvalidTile(100,tile) end
        TileGroupManager:AddInvalidTile(100,1)
        assert(#assert(observer:report(mini,meta)).transparentOceanExclusions == 4096)
        TileGroupManager:RemoveInvalidTile(100,1)
        TileGroupManager:AddInvalidTile(100,4096)
        assert(#assert(observer:report(mini,meta)).transparentOceanExclusions == 4096)
        TileGroupManager:AddInvalidTile(100,4097)
        local report, err = observer:report(mini,meta)
        assert(not report and string.find(err,'exceed 4096'))
    "#,
        )
        .exec()
        .unwrap();
    for tile in [0.5, 65536.0, f64::INFINITY, f64::NAN] {
        let lua = fixture();
        lua.globals().set("invalid_tile", tile).unwrap();
        lua.load(
            r#"
            TileGroupManager:AddInvalidTile(100,invalid_tile)
            local report, err = observer:report(mini,meta)
            assert(not report and string.find(err,'invalid tile ID'))
        "#,
        )
        .exec()
        .unwrap();
    }
}

#[test]
fn transparent_ocean_observation_requires_known_start_and_stable_group_identity() {
    fixture()
        .load(
            r#"
        observer:check_load_order({{modname='earlier-mod'},{modname='collector'}},'collector')
        local report, err = observer:report(mini,meta)
        assert(not report and string.find(err,'before material observation'))
    "#,
        )
        .exec()
        .unwrap();
    fixture()
        .load(
            r#"
        TileGroups.TransparentOceanTiles = 101
        local report, err = observer:report(mini,meta)
        assert(not report and string.find(err,'identity changed'))
    "#,
        )
        .exec()
        .unwrap();
}
