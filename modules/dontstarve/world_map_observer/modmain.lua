-- LGSM managed: world map observer
local G = GLOBAL
local key = "__lgsm_world_map_observer"
local previous = G.rawget(G, key)
if previous ~= nil then
    -- Never replace another observer's live state or install duplicate wrappers.
    return
end

local observer = {
    version = 1,
    entries = G.setmetatable({}, { __mode = "k" }),
    installedBeforeWorld = G.rawget(G, "TheWorld") == nil,
}
G.rawset(G, key, observer)

local wrapped_entities = G.setmetatable({}, { __mode = "k" })
local wrapped_minimaps = G.setmetatable({}, { __mode = "k" })
local function pack(...)
    return { n = G.select("#", ...), ... }
end
local function issue(message)
    if observer.issue == nil then
        observer.issue = G.string.sub(G.tostring(message), 1, 256)
    end
end
local function observe(fn, ...)
    -- An observation failure must not break a successful native game action.
    local ok, message = G.pcall(fn, ...)
    if not ok then issue(message) end
end
local function entry(component)
    local value = observer.entries[component]
    if value == nil then
        value = { iconObserved = false }
        observer.entries[component] = value
    end
    return value
end
local function methods(object)
    local mt = G.getmetatable(object)
    return G.type(mt) == "table" and G.type(mt.__index) == "table" and mt.__index or nil
end
local function record_icon(component, icon)
    local value = entry(component)
    value.iconObserved = G.type(icon) == "string"
    value.icon = value.iconObserved and icon or nil
end
local function record_copy(component, source)
    -- Native globalmapicon.lua passes target.MiniMapEntity, not an EntityScript.
    local from = observer.entries[source]
    local value = entry(component)
    value.iconObserved = from ~= nil and from.iconObserved == true
    value.icon = value.iconObserved and from.icon or nil
end
local function record_enabled(component, enabled)
    local value = entry(component)
    if G.type(enabled) == "boolean" then value.enabled = enabled else value.enabled = nil end
end
local function record_priority(component, priority)
    local value = entry(component)
    if G.type(priority) == "number" and priority == priority
        and priority ~= G.math.huge and priority ~= -G.math.huge then
        value.priority = priority
    else
        value.priority = nil
    end
end
local function wrap(original, record)
    return function(component, ...)
        -- Keep native errors, arguments, nils and multiple return values intact.
        local results = pack(original(component, ...))
        observe(record, component, ...)
        return G.unpack(results, 1, results.n)
    end
end
local function attach_minimap(component)
    local index = methods(component)
    if index == nil then issue("Native MiniMapEntity method table is unavailable.") return end
    if wrapped_minimaps[index] then return end
    if G.type(index.SetIcon) ~= "function" or G.type(index.CopyIcon) ~= "function"
        or G.type(index.SetEnabled) ~= "function" or G.type(index.SetPriority) ~= "function" then
        issue("Native MiniMapEntity icon methods are unavailable.")
        return
    end
    index.SetIcon = wrap(index.SetIcon, record_icon)
    index.CopyIcon = wrap(index.CopyIcon, record_copy)
    index.SetEnabled = wrap(index.SetEnabled, record_enabled)
    index.SetPriority = wrap(index.SetPriority, record_priority)
    wrapped_minimaps[index] = true
end
local function after_add(entity)
    local inst = G.Ents[entity:GetGUID()]
    if inst == nil or inst.MiniMapEntity == nil then
        issue("A native MiniMapEntity was added without its EntityScript.")
        return
    end
    attach_minimap(inst.MiniMapEntity)
end
local function attach_entity(inst)
    local index = methods(inst.entity)
    if index == nil or G.type(index.AddMiniMapEntity) ~= "function" then
        issue("Native Entity MiniMapEntity constructor is unavailable.")
        return
    end
    if wrapped_entities[index] then return end
    index.AddMiniMapEntity = wrap(index.AddMiniMapEntity, after_add)
    wrapped_entities[index] = true
end

local function attach_render_observer()
    -- Dedicated worlds also create their native minimap and apply Mod layers.
    -- Observe those successful calls with the shared bounded implementation;
    -- do not infer a Mod's mappings from filenames or execute its Lua again.
    modimport("render_observer.lua")
    local render = LGSMWorldMapRenderObserver.new(G)
    observer.render = render
    render:check_load_order(G.ModManager and G.ModManager.mods, modname)
    render:attach_tile_groups(G.TileGroupManager, G.TileGroups and G.TileGroups.TransparentOceanTiles)
    if G.MapLayerManager then render:attach_manager(G.MapLayerManager) end
    if G.TheGlobalInstance then render:attach_entity(G.TheGlobalInstance) end
end
observe(attach_render_observer)

-- Room indices can return to their previous value between map polls. Track
-- successful native changes as well as the current room identity.
AddComponentPostInit("virtualroomset", function(component)
    component._lgsm_map_revision = 0
    for _, name in G.ipairs({ "SetCurrentRoomIndex", "SetOriginInTiles" }) do
        local original = component[name]
        if G.type(original) == "function" then
            component[name] = wrap(original, function(room)
                room._lgsm_map_revision = (room._lgsm_map_revision or 0) + 1
            end)
        end
    end
end)

-- This runs inside CreateEntity, before a prefab adds its MiniMapEntity and
-- calls SetIcon. PrefabPostInitAny would run after those initial icon changes.
AddGlobalClassPostConstruct("entityscript", "EntityScript", function(inst)
    observe(attach_entity, inst)
    if observer.render then observe(observer.render.attach_entity, observer.render, inst) end
end)
