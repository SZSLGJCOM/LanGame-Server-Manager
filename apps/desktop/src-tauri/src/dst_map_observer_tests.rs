use mlua::Lua;

const OBSERVER: &str =
    include_str!("../../../../modules/dontstarve/world_map_observer/modmain.lua");
const MODINFO: &str = include_str!("../../../../modules/dontstarve/world_map_observer/modinfo.lua");
const RENDER_OBSERVER: &str =
    include_str!("../../../../modules/dontstarve/world_map_shared/render_observer.lua");

fn fixture() -> Lua {
    let lua = Lua::new();
    lua.globals()
        .set("render_observer_source", RENDER_OBSERVER)
        .unwrap();
    lua.load(
        r#"
        GLOBAL = _G
        unpack = table.unpack
        modname = 'lgsm_world_map_observer'
        ModManager = { mods = {{modname=modname}} }
        function modimport(name)
            assert(name == 'render_observer.lua')
            assert(load(render_observer_source))()
        end
        local groups = {}
        for _, name in ipairs({'AddInvalidTile','RemoveInvalidTile','AddValidTile',
                'RemoveValidTile','SetValidTileRange','SetIsTransparentOceanTileGroup'}) do
            groups[name] = function(_, group, tile) return 'native-group-result', nil, tile end
        end
        TileGroupManager = setmetatable({}, {__index=groups})
        TileGroups = {TransparentOceanTiles=100}
        local layers, next_handle = {}, 0
        function layers:CreateRenderLayer(tile, atlas, texture, noise, extra)
            if tile == -1 then error('native-render-error') end
            if self.nilHandle then return nil end
            next_handle = next_handle + 1
            return self.fixedHandle or next_handle, nil, extra, nil
        end
        MapLayerManager = setmetatable({}, {__index=layers})
        local map = {}
        function map:AddRenderLayer(handle, extra)
            self.appliedHandle = handle
            return 'native-layer-result', nil, extra
        end
        function map:AddAtlas(path) return 'native-atlas-result', nil end
        Ents = {}
        callbacks = {}
        component_callbacks = {}
        function AddComponentPostInit(name, fn) component_callbacks[name] = fn end
        nativeCalls = 0
        function AddGlobalClassPostConstruct(package, name, fn)
            assert(package == 'entityscript' and name == 'EntityScript')
            callbacks[#callbacks + 1] = fn
        end
        local mini = {}
        function mini:SetIcon(icon, extra)
            nativeCalls = nativeCalls + 1
            if icon == 'native-error' then error('native-icon-error') end
            self.nativeIcon = icon
            self.extra = extra
            return 'native-result', nil, extra, nil
        end
        function mini:CopyIcon(source)
            assert(type(source) == 'table', 'native-source-error')
            self.nativeIcon = source.nativeIcon
            return 'copy-result'
        end
        function mini:SetEnabled(enabled)
            self.nativeEnabled = enabled
            return 'enabled-result'
        end
        function mini:SetPriority(priority, extra)
            if priority == 'native-error' then error('native-priority-error') end
            self.nativePriority = priority
            return 'priority-result', nil, extra
        end
        local entity = {}
        function entity:GetGUID() return self.guid end
        function entity:AddMiniMapEntity()
            Ents[self.guid].MiniMapEntity = setmetatable({}, { __index = mini })
            return 'add-result', nil
        end
        function entity:AddMapLayerManager() return MapLayerManager end
        function entity:AddMiniMap()
            Ents[self.guid].MiniMap = setmetatable({}, {__index=map})
            return 'native-minimap-result', nil
        end
        local function create_entity(guid)
            local inst = { entity = setmetatable({guid=guid}, {__index=entity}) }
            for _,fn in ipairs(callbacks) do fn(inst) end
            Ents[guid] = inst
            return inst
        end
        function create(guid)
            local inst = create_entity(guid)
            local first, last = inst.entity:AddMiniMapEntity()
            assert(first == 'add-result' and last == nil)
            return inst.MiniMapEntity
        end
        function create_minimap(guid)
            local inst = create_entity(guid)
            local first, last = inst.entity:AddMiniMap()
            assert(first == 'native-minimap-result' and last == nil)
            return inst.MiniMap
        end
        function unknown_component(icon)
            return setmetatable({nativeIcon=icon}, {__index=mini})
        end
    "#,
    )
    .exec()
    .unwrap();
    lua.load(OBSERVER).exec().unwrap();
    lua
}

#[test]
fn observer_is_server_only_and_does_not_require_clients() {
    let lua = Lua::new();
    lua.load(MODINFO).exec().unwrap();
    assert!(lua.globals().get::<bool>("server_only_mod").unwrap());
    assert!(
        !lua.globals()
            .get::<bool>("all_clients_require_mod")
            .unwrap()
    );
    assert!(!lua.globals().get::<bool>("client_only_mod").unwrap());
}

#[test]
fn virtual_room_observation_preserves_native_results_and_counts_return_visits() {
    fixture()
        .load(
            r#"
        local room = {currentroomindex=0}
        function room:SetCurrentRoomIndex(index, extra)
            if index == -1 then error('native-room-error') end
            self.currentroomindex=index
            return 'room-result',nil,extra,nil
        end
        function room:SetOriginInTiles(x,y) self.origin={tx=x,ty=y} end
        component_callbacks.virtualroomset(room)
        local result=table.pack(room:SetCurrentRoomIndex(1,42))
        assert(result.n==4 and result[1]=='room-result' and result[2]==nil and result[3]==42)
        room:SetCurrentRoomIndex(0)
        assert(room.currentroomindex==0 and room._lgsm_map_revision==2)
        local ok,message=pcall(room.SetCurrentRoomIndex,room,-1)
        assert(not ok and message:find('native-room-error',1,true) and room._lgsm_map_revision==2)
        room:SetOriginInTiles(30,40)
        assert(room.origin.tx==30 and room._lgsm_map_revision==3)
    "#,
        )
        .exec()
        .unwrap();
}

#[test]
fn server_observer_records_applied_materials_and_postinit_overrides_with_the_shared_collector() {
    fixture().load(r#"
        local render = __lgsm_world_map_observer.render
        assert(render and render.issue == nil and __lgsm_world_map_observer.issue == nil)
        local mini = create_minimap(50)
        mini:AddAtlas('minimap/minimap_data2.xml')
        mini:AddAtlas('../mods/workshop-123/images/mod.xml')
        -- A dedicated renderer may reuse an opaque handle. Capture each
        -- association when it is applied, including a Mod's direct PostInit.
        MapLayerManager.fixedHandle = 0
        local first = table.pack(MapLayerManager:CreateRenderLayer(7,
            'levels/tiles/map_edge.xml','levels/tiles/map_edge.tex','standard.tex',42))
        assert(first.n == 4 and first[1] == 0 and first[2] == nil and first[3] == 42 and first[4] == nil)
        local applied = table.pack(mini:AddRenderLayer(first[1],71))
        assert(applied.n == 3 and applied[1] == 'native-layer-result' and applied[2] == nil and applied[3] == 71)
        mini:AddRenderLayer(MapLayerManager:CreateRenderLayer(7,
            'levels/tiles/map_edge.xml','levels/tiles/map_edge.tex','../mods/workshop-123/mini.tex'))
        TileGroupManager:AddInvalidTile(TileGroups.TransparentOceanTiles,203)
        local report = assert(render:report(mini, {
            gameVersion='756039',session='ABC123',width=378,height=378,mods={}}))
        assert(#report.layers == 2 and report.layers[1].noiseTexture == 'standard.tex')
        assert(report.layers[2].tileId == 7 and report.layers[2].noiseTexture == '../mods/workshop-123/mini.tex')
        assert(#report.atlases == 2 and report.atlases[2] == '../mods/workshop-123/images/mod.xml')
        assert(#report.transparentOceanExclusions == 1 and report.transparentOceanExclusions[1] == 203)
        local ok, message = pcall(MapLayerManager.CreateRenderLayer,MapLayerManager,-1)
        assert(not ok and message:find('native-render-error',1,true))
        assert(render.issue == nil)
    "#).exec().unwrap();
}

#[test]
fn server_observer_never_reports_success_when_native_layers_cannot_be_associated() {
    fixture()
        .load(
            r#"
        local mini = create_minimap(50)
        mini:AddAtlas('minimap/minimap_data2.xml')
        MapLayerManager.nilHandle = true
        assert(MapLayerManager:CreateRenderLayer(7,'a.xml','a.tex','noise.tex') == nil)
        local report, message = __lgsm_world_map_observer.render:report(mini, {})
        assert(report == nil and message:find('returned no handle',1,true))
        local icon = create(51)
        assert(icon:SetIcon('still-works.png') == 'native-result')
        assert(__lgsm_world_map_observer.entries[icon].icon == 'still-works.png')
        assert(__lgsm_world_map_observer.issue == nil)
    "#,
        )
        .exec()
        .unwrap();
}

#[test]
fn observer_captures_initial_dynamic_icons_and_preserves_native_calls() {
    let lua = fixture();
    lua.load(r#"
        local observer = __lgsm_world_map_observer
        assert(observer.version == 1 and observer.installedBeforeWorld)
        assert(getmetatable(observer.entries).__mode == 'k')
        local component = create(1)
        local returned = table.pack(component:SetIcon('a-dynamic-mod-icon.tex', 42))
        assert(returned.n == 4 and returned[1] == 'native-result' and returned[2] == nil
            and returned[3] == 42 and returned[4] == nil)
        assert(component.nativeIcon == 'a-dynamic-mod-icon.tex' and component.extra == 42)
        assert(observer.entries[component].icon == 'a-dynamic-mod-icon.tex')
        assert(observer.entries[component].iconObserved and observer.entries[component].enabled == nil)
        assert(component:SetEnabled(false) == 'enabled-result')
        assert(observer.entries[component].enabled == false and component.nativeEnabled == false)
        component:SetEnabled(true)
        assert(observer.entries[component].enabled == true)
        local priorityResult = table.pack(component:SetPriority(-3, 42))
        assert(priorityResult.n == 3 and priorityResult[1] == 'priority-result'
            and priorityResult[2] == nil and priorityResult[3] == 42)
        assert(component.nativePriority == -3 and observer.entries[component].priority == -3)
        local priorityOk, priorityError = pcall(component.SetPriority, component, 'native-error')
        assert(not priorityOk and priorityError:find('native-priority-error', 1, true))
        assert(observer.entries[component].priority == -3)
        local ok, error = pcall(component.SetIcon, component, 'native-error')
        assert(not ok and error:find('native-icon-error', 1, true))
        assert(observer.entries[component].icon == 'a-dynamic-mod-icon.tex')
        assert(nativeCalls == 2 and observer.issue == nil)
    "#).exec().unwrap();
}

#[test]
fn observer_copy_uses_native_component_identity_and_clears_unknown_sources() {
    let lua = fixture();
    lua.load(
        r#"
        local a, b = create(1), create(2)
        a:SetIcon('source.png')
        b:SetEnabled(false)
        assert(b:CopyIcon(a) == 'copy-result')
        local record = __lgsm_world_map_observer.entries[b]
        assert(record.icon == 'source.png' and record.iconObserved and record.enabled == false)
        a:SetIcon('changed.png')
        assert(record.icon == 'source.png')
        b:CopyIcon(unknown_component('unobserved.png'))
        assert(b.nativeIcon == 'unobserved.png' and record.icon == nil and not record.iconObserved)
        b:SetIcon('observed-again.png')
        local ok = pcall(b.CopyIcon, b, nil)
        assert(not ok and record.icon == 'observed-again.png' and record.iconObserved)
    "#,
    )
    .exec()
    .unwrap();
}

#[test]
fn observer_preserves_explicit_icon_clearing_and_copies_it() {
    let lua = fixture();
    lua.load(
        r#"
        local source, target = create(1), create(2)
        source:SetIcon('visible.png')
        source:SetIcon('')
        target:CopyIcon(source)
        for _, component in ipairs({ source, target }) do
            local record = __lgsm_world_map_observer.entries[component]
            assert(component.nativeIcon == '' and record.icon == '' and record.iconObserved)
        end
        assert(__lgsm_world_map_observer.issue == nil)
    "#,
    )
    .exec()
    .unwrap();
}

#[test]
fn observer_keeps_later_mod_wrappers_and_does_not_install_twice() {
    let lua = fixture();
    lua.load(
        r#"
        component = create(1)
        local index = getmetatable(component).__index
        local observed = index.SetIcon
        laterCalls = 0
        index.SetIcon = function(self, icon, ...)
            laterCalls = laterCalls + 1
            return observed(self, 'later-'..icon, ...)
        end
        create(2):SetIcon('dynamic.png')
        assert(laterCalls == 1)
        assert(__lgsm_world_map_observer.entries[Ents[2].MiniMapEntity].icon == 'later-dynamic.png')
    "#,
    )
    .exec()
    .unwrap();
    lua.load(OBSERVER).exec().unwrap();
    lua.load("assert(#callbacks == 1); component:SetIcon('again.png'); assert(laterCalls == 2)")
        .exec()
        .unwrap();
}

#[test]
fn observation_failure_does_not_change_successful_native_behavior() {
    let lua = fixture();
    lua.load(
        r#"
        local component = create(1)
        __lgsm_world_map_observer.entries = nil
        assert(component:SetIcon('native-still-works.png') == 'native-result')
        assert(component.nativeIcon == 'native-still-works.png')
        assert(type(__lgsm_world_map_observer.issue) == 'string')
    "#,
    )
    .exec()
    .unwrap();
}

#[test]
fn observer_does_not_keep_removed_components_alive() {
    let lua = fixture();
    lua.load(
        r#"
        local component = create(1)
        component:SetIcon('temporary.png')
        local entries = __lgsm_world_map_observer.entries
        assert(next(entries) ~= nil)
        Ents[1] = nil
        component = nil
        collectgarbage('collect')
        collectgarbage('collect')
        assert(next(entries) == nil)
    "#,
    )
    .exec()
    .unwrap();
}
