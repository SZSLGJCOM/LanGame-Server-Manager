-- LGSM managed: world map observer
-- Observe successful native calls without changing game arguments or results.
local M = {}
function M.new(G, changed)
    local self = { revision = 0, issue = nil }
    local handles, maps = {}, G.setmetatable({}, { __mode = "k" })
    local entities, managers, minimaps = {}, {}, {}
    local styles = G.setmetatable({}, { __mode = "k" })
    local transparent_manager, transparent_group
    local exclusions, exclusion_count = {}, 0
    local load_order_known = false
    local created, applied, atlases = 0, 0, 0
    local function fail(message)
        if self.issue == nil then
            self.issue = message
            self.revision = self.revision + 1
            if changed then G.pcall(changed) end
        end
    end
    local function pack(...) return { n = G.select("#", ...), ... } end
    local function method_table(object)
        local mt = object ~= nil and G.getmetatable(object) or nil
        return G.type(mt) == "table" and G.type(mt.__index) == "table" and mt.__index or nil
    end
    local function observe(fn, ...)
        local ok, message = G.pcall(fn, ...)
        if not ok then fail("Native material observation failed: " .. G.string.sub(G.tostring(message), 1, 160)) end
    end
    local function touched()
        self.revision = self.revision + 1
        if changed then changed() end
    end
    local function wrap(original, after)
        return function(object, ...)
            local results = pack(original(object, ...))
            observe(after, object, results, ...)
            return G.unpack(results, 1, results.n)
        end
    end
    local function state(component)
        local value = maps[component]
        if value == nil then value = { layers = {}, atlases = {} }; maps[component] = value end
        return value
    end
    local function valid_path(value)
        if G.type(value) ~= "string" or #value == 0 or #value > 512
            or G.string.find(value, "[%z\1-\31\127]") then return false end
        local cursor = 1
        while cursor <= #value do
            local first = G.string.byte(value, cursor)
            local size = first < 128 and 1 or first >= 194 and first <= 223 and 2
                or first >= 224 and first <= 239 and 3 or first >= 240 and first <= 244 and 4
            if not size then return false end
            for index = 1, size - 1 do
                local byte = G.string.byte(value, cursor + index)
                if not byte or byte < 128 or byte > 191 then return false end
            end
            local second = G.string.byte(value, cursor + 1)
            if (first == 224 and second < 160) or (first == 237 and second >= 160)
                or (first == 240 and second < 144) or (first == 244 and second >= 144) then return false end
            cursor = cursor + size
        end
        return true
    end
    function self:inventory_icon(entity, budget)
        -- Use the actual inventory UI getters. net_hash getters can return only
        -- numbers: verify an entity's explicit literal rather than reversing it
        -- or guessing an image from the prefab name.
        local ok, value = G.pcall(function()
            local replica = entity.replica and entity.replica.inventoryitem
            local item = entity.components and entity.components.inventoryitem
            if not replica or not item or G.type(replica.GetImage) ~= "function"
                or G.type(replica.GetAtlas) ~= "function" then return nil end
            local image, atlas = replica:GetImage(), replica:GetAtlas()
            local function matches(actual, literal)
                return valid_path(literal) and G.type(G.hash) == "function" and G.hash(literal) == actual
            end
            if G.type(image) == "number" then
                local declared = G.type(item.imagename) == "string" and item.imagename .. ".tex" or nil
                if matches(image, replica.overrideimage) then image = replica.overrideimage
                elseif matches(image, declared) then image = declared
                else return nil end
            end
            if G.type(atlas) == "number" then
                if not valid_path(item.atlasname) then return nil end
                local declared = G.resolvefilepath(item.atlasname)
                if not matches(atlas, declared) then return nil end
                atlas = declared
            end
            if not valid_path(image) or #image > 128 or not valid_path(atlas) then return nil end
            atlas = G.resolvefilepath(atlas)
            if not valid_path(atlas) then return nil end
            if G.string.sub(atlas, 1, 5) == "data/" then atlas = G.string.sub(atlas, 6) end
            if budget then
                local bytes = (budget.bytes or 0) + #atlas + #image + 48
                if bytes > 1048576 then return nil end
                budget.bytes = bytes
            end
            return { atlas = atlas, element = image }
        end)
        return ok and value or nil
    end
    function self:check_load_order(loaded_mods, current_mod)
        -- LoadMods sorts this actual execution list before invoking modmain.
        -- Earlier Mods could have changed native groups before observation.
        if G.type(loaded_mods) ~= "table" or G.type(current_mod) ~= "string"
            or G.type(loaded_mods[1]) ~= "table" or loaded_mods[1].modname ~= current_mod then
            fail("Another Mod ran before material observation; transparent ocean changes are unknown.")
            return
        end
        load_order_known = true
    end
    function self:attach_tile_groups(manager, group)
        if transparent_manager then
            if manager ~= transparent_manager or group ~= transparent_group then
                fail("The transparent ocean group changed after observation began.")
            end
            return
        end
        local index = method_table(manager)
        local methods = {"AddInvalidTile", "RemoveInvalidTile", "AddValidTile", "RemoveValidTile",
            "SetValidTileRange", "SetIsTransparentOceanTileGroup"}
        if group == nil or index == nil then fail("The native transparent ocean group is unavailable.") return end
        for _, name in G.ipairs(methods) do
            if G.type(index[name]) ~= "function" then fail("A native tile-group observation method is unavailable: " .. name) return end
        end
        transparent_manager, transparent_group = manager, group
        local function exclusion(object, _, target, tile, remove)
            if object ~= manager or target ~= group then return end
            if G.type(tile) ~= "number" or tile < 0 or tile > 65535 or tile % 1 ~= 0 then
                fail("A native transparent ocean exclusion has an invalid tile ID.") return
            end
            if remove then
                if exclusions[tile] then exclusions[tile] = nil; exclusion_count = exclusion_count - 1; touched() end
            elseif not exclusions[tile] then
                if exclusion_count >= 4096 then fail("Transparent ocean exclusions exceed 4096 tiles.") return end
                exclusions[tile] = true
                exclusion_count = exclusion_count + 1
                touched()
            end
        end
        index.AddInvalidTile = wrap(index.AddInvalidTile, function(object, results, target, tile)
            exclusion(object, results, target, tile, false)
        end)
        index.RemoveInvalidTile = wrap(index.RemoveInvalidTile, function(object, results, target, tile)
            exclusion(object, results, target, tile, true)
        end)
        for _, name in G.ipairs({"AddValidTile", "RemoveValidTile", "SetValidTileRange"}) do
            index[name] = wrap(index[name], function(object, _, target)
                if object == manager and target == group then
                    fail("The transparent ocean group received an unsupported membership change: " .. name)
                end
            end)
        end
        index.SetIsTransparentOceanTileGroup = wrap(index.SetIsTransparentOceanTileGroup, function(object, _, target)
            if object == manager and target ~= group then fail("The native transparent ocean group was replaced.") end
        end)
    end
    function self:attach_manager(manager)
        local index = method_table(manager)
        if index == nil or G.type(index.CreateRenderLayer) ~= "function" then
            fail("Native MapLayerManager method table is unavailable.") return
        end
        if managers[index] then return end
        if G.type(index.SetSampleStyle) == "function" then
            index.SetSampleStyle = wrap(index.SetSampleStyle, function(object, _, style)
                styles[object] = G.type(style) == "number" and style >= 0 and style <= 255
                    and style % 1 == 0 and style or false
                touched()
            end)
        end
        index.CreateRenderLayer = wrap(index.CreateRenderLayer, function(object, results, tile, atlas, texture, noise)
            local handle = results[1]
            if handle == nil then fail("CreateRenderLayer returned no handle.") return end
            if handles[handle] == nil then created = created + 1 end
            if created > 4096 then fail("Native render-layer observation exceeds 4096 handles.") return end
            handles[handle] = { tileId = tile, atlas = atlas, texture = texture, noiseTexture = noise, manager = object }
        end)
        managers[index] = true
    end
    function self:attach_minimap(component)
        local index = method_table(component)
        if index == nil or G.type(index.AddRenderLayer) ~= "function" or G.type(index.AddAtlas) ~= "function" then
            fail("Native MiniMap material methods are unavailable.") return
        end
        if minimaps[index] then return end
        index.AddRenderLayer = wrap(index.AddRenderLayer, function(object, _, handle)
            applied = applied + 1
            if applied > 4096 then fail("Applied minimap layers exceed 4096.") return end
            local layer = handles[handle]
            if layer == nil then fail("MiniMap applied an unobserved render-layer handle.") return end
            -- Copy the association at application time, preserving duplicate tiles
            -- and their order even if a later native call reuses a handle.
            G.table.insert(state(object).layers, { tileId = layer.tileId, atlas = layer.atlas,
                texture = layer.texture, noiseTexture = layer.noiseTexture, manager = layer.manager })
            touched()
        end)
        index.AddAtlas = wrap(index.AddAtlas, function(object, _, path)
            atlases = atlases + 1
            if atlases > 512 then fail("Applied minimap atlases exceed 512.") return end
            G.table.insert(state(object).atlases, path)
            touched()
        end)
        minimaps[index] = true
    end
    function self:attach_entity(inst)
        local index = method_table(inst.entity)
        if index == nil or G.type(index.AddMapLayerManager) ~= "function" or G.type(index.AddMiniMap) ~= "function" then
            fail("Native Entity map constructors are unavailable.") return
        end
        if entities[index] then return end
        index.AddMapLayerManager = wrap(index.AddMapLayerManager, function(_, results)
            self:attach_manager(results[1])
        end)
        index.AddMiniMap = wrap(index.AddMiniMap, function(entity)
            local current = G.Ents[entity:GetGUID()]
            if current == nil or current.MiniMap == nil then fail("Native MiniMap has no EntityScript owner.") return end
            self:attach_minimap(current.MiniMap)
        end)
        entities[index] = true
    end
    function self:report(component, metadata)
        if self.issue then return nil, self.issue end
        if not load_order_known or not transparent_manager then return nil, "Transparent ocean observation did not initialize." end
        if not G.TileGroups or G.TileGroups.TransparentOceanTiles ~= transparent_group then
            return nil, "The transparent ocean group identity changed."
        end
        local value = maps[component]
        if value == nil or #value.layers == 0 or #value.atlases == 0 then return nil, "No applied native minimap materials were observed." end
        if G.type(metadata.gameVersion) ~= "string" or not G.string.match(metadata.gameVersion, "^%d+$")
            or G.type(metadata.session) ~= "string" or #metadata.session > 128
            or not G.string.match(metadata.session, "^[%w_-]+$") then return nil, "Actual game version or world session is unavailable." end
        for _, key in G.ipairs({"width", "height"}) do
            local n = metadata[key]
            if G.type(n) ~= "number" or n < 1 or n > 2048 or n % 1 ~= 0 then return nil, "Actual map dimensions are unavailable." end
        end
        if metadata.width * metadata.height > 1048576 then return nil, "Map dimensions exceed the supported tile budget." end
        if G.type(metadata.mods) ~= "table" or #metadata.mods > 128 then return nil, "Enabled server Mod metadata exceeds 128 entries." end
        for _, mod in G.ipairs(metadata.mods) do
            if not valid_path(mod.name) or #mod.name > 128 or not valid_path(mod.version) or #mod.version > 128 then
                return nil, "An enabled server Mod is missing its actual name or version."
            end
        end
        local layers, invalid, invalid_styles = {}, 0, 0
        for _, layer in G.ipairs(value.layers) do
            if G.type(layer.tileId) == "number" and layer.tileId >= 0 and layer.tileId <= 65535 and layer.tileId % 1 == 0 then
                local safe = {tileId = layer.tileId}
                -- Styles belong to the actual manager, including calls after a
                -- layer was applied. Absence preserves an unobserved default.
                local style = styles[layer.manager]
                if style == false then
                    invalid = invalid + 1
                    invalid_styles = invalid_styles + 1
                else safe.sampleStyle = style end
                for _, key in G.ipairs({"atlas", "texture", "noiseTexture"}) do
                    if valid_path(layer[key]) then safe[key] = layer[key] else invalid = invalid + 1 end
                end
                -- An observed unusable style is not an unobserved default.
                -- Preserve the layer/winner and flat material, but do not let
                -- the renderer select nine-sample edges for this layer.
                if style == false then safe.atlas = nil end
                -- Keep the final application even when its artwork is broken:
                -- a previous layer must not silently become the new winner.
                G.table.insert(layers, safe)
            else
                invalid = invalid + 1
            end
        end
        local icons_uncertain = false
        for _, path in G.ipairs(value.atlases) do
            if not valid_path(path) then icons_uncertain = true end
        end
        local excluded = {}
        for tile in G.pairs(exclusions) do G.table.insert(excluded, tile) end
        G.table.sort(excluded)
        local report = { version = 1, gameVersion = metadata.gameVersion, session = metadata.session,
            width = metadata.width, height = metadata.height, mods = metadata.mods,
            layers = layers, atlases = icons_uncertain and {} or value.atlases }
        if invalid > 0 or icons_uncertain then
            report.issue = "Ignored " .. invalid .. " invalid native material fields."
            if invalid_styles > 0 then
                report.issue = report.issue .. " Unsupported native sample style on " .. invalid_styles .. " layers; edge artwork omitted."
            end
            if icons_uncertain then
                report.iconsUncertain = true
                report.issue = report.issue .. " Icon registration is uncertain; icon artwork is omitted."
            end
        end
        -- An absent field means no observed exclusions.
        if #excluded > 0 then report.transparentOceanExclusions = excluded end
        return report
    end
    return self
end
-- DST's modimport executes in the Mod environment and discards return values.
LGSMWorldMapRenderObserver = M
return M
