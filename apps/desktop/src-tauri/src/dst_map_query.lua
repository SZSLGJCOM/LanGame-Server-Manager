local nonce = "__NONCE__"
local terrain = __TERRAIN__
local world = TheWorld
local json = require("json")
local function emit(value)
    local line = "[LGSM-DST-MAP:" .. nonce .. "] " .. json.encode_compliant(value)
    -- The native console truncates at 4095 bytes, even if the log reader allows more.
    if #line > 3900 then error("Map frame exceeds the native console limit") end
    print(line)
end
if not world or not world.ismastersim or not InGamePlay() then
    emit({ k = "error", code = "world_not_ready", message = "The shard world is not ready." })
    return
end
local map = world.Map
local function geometry()
    local manager = world.components and world.components.virtualroommanager
    local parts = {}
    for _, room in ipairs(manager and manager.virtualroomsets or {}) do
        if #parts >= 64 then error("Too many virtual room sets") end
        local origin = room.origin or {}
        parts[#parts + 1] = table.concat({ room.inst.GUID, room.currentroomindex,
            origin.tx or -1, origin.ty or -1, assert(room._lgsm_map_revision) }, ":")
    end
    local value = table.concat(parts, ",")
    if #value > 2048 then error("Map geometry identity exceeds limit") end
    return value
end
local valid, revision = pcall(geometry)
if not valid then emit({ k = "error", message = "Invalid room state; update the map observer and restart the shard." }) return end
local prior = world._lgsm_world_map_query
if prior then
    if prior.owner ~= "LGSM" or GetTimeReal() < prior.expires then
        emit({ k = "error", message = "This shard already has a map query in progress." })
        return
    end
    if prior.task then prior.task:Cancel() end
end
local active = { owner = "LGSM", expires = GetTimeReal() + 20000 }
world._lgsm_world_map_query = active
local function release()
    if world._lgsm_world_map_query == active then world._lgsm_world_map_query = nil end
end
local width, height = map:GetSize()
if width < 1 or height < 1 or width > 2048 or height > 2048 or width * height > 1048576 then
    emit({ k = "error", message = "The world exceeds the map collection limit." })
    release()
    return
end
local utf_error = "Invalid UTF-8 in native map text."
local function clean(value, limit)
    value = string.gsub(tostring(value or ""), "%c", " ")
    local cursor, last = 1, 0
    while cursor <= #value and cursor <= limit do
        local first = string.byte(value, cursor)
        local size = first < 128 and 1 or first >= 194 and first <= 223 and 2
            or first >= 224 and first <= 239 and 3 or first >= 240 and first <= 244 and 4
        if not size then error(utf_error) end
        for index = 1, size - 1 do
            local byte = string.byte(value, cursor + index)
            if not byte or byte < 128 or byte > 191 then error(utf_error) end
        end
        local second = string.byte(value, cursor + 1)
        if (first == 224 and second < 160) or (first == 237 and second >= 160)
            or (first == 240 and second < 144) or (first == 244 and second >= 144) then
            error(utf_error)
        end
        if cursor + size - 1 > limit then break end
        last = cursor + size - 1
        cursor = last + 1
    end
    return string.sub(value, 1, last)
end
emit({ k = "begin", width = width, height = height, tileScale = TILE_SCALE, geometryRevision = revision,
    gameVersion = clean(rawget(_G, "APP_VERSION"), 64),
    session = TheNet:GetSessionIdentifier(), day = world.state.cycles + 1,
    season = clean(world.state.season, 64), phase = clean(world.state.phase, 64) })
local offset = 0
local used = {}
__LANDMARK_ICONS__
local obs = rawget(_G, "__lgsm_world_map_observer")
local observed = type(obs) == "table" and obs.version == 1 and type(obs.entries) == "table"
local mats
local function map_icon(entity)
    local entry = observed and entity.MiniMapEntity and obs.entries[entity.MiniMapEntity]
    if entry then
        if entry.enabled == false then return nil, 0 end
        -- Native SetIcon("") clears the image. Lua's empty string is truthy:
        -- do not export it as a missing asset or restore the prefab fallback.
        if entry.iconObserved and entry.icon ~= "" then return entry.icon, entry.priority or 0 end
        return nil, 0
    end
    -- Existing servers need a normal startup to observe initial native calls.
    -- This limited native fallback never guesses a Mod prefab's icon.
    return landmark_icons[entity.prefab], 0
end
local failure, failure_code
local function reject(message)
    failure = message
    error(message)
end
local function finite(value)
    return type(value) == "number" and value == value and math.abs(value) <= 10000000
end
local function coordinates(x, z)
    if not finite(x) or not finite(z) then reject("Invalid native map coordinates.") end
    local tx, tz = map:GetTileCoordsAtPoint(x, 0, z)
    local cx, _, cz = map:GetTileCenterPoint(x, 0, z)
    local map_x = tx + .5 + (x - cx) / TILE_SCALE
    local map_z = tz + .5 + (z - cz) / TILE_SCALE
    if not finite(map_x) or not finite(map_z) then reject("Invalid native map coordinates.") end
    return map_x, map_z
end
local function emit_terrain_names()
    local textures, bases = {}, {}
    local tile_names = {}
    local invalid = 0
    local function noise(path)
        if type(path) ~= "string" then return nil end
        if string.sub(path, 1, 5) == "data/" then path = string.sub(path, 6) end
        local ok, safe = pcall(clean, path, 512)
        return ok and #path > 0 and safe == path and path or nil
    end
    if type(GetWorldTileMap) == "function" then
        local count = 0
        for name, id in pairs(GetWorldTileMap()) do
            count = count + 1
            if count > 8192 then reject("Too many registered world tile names.") end
            if type(name) == "string" and type(id) == "number" and id >= 0 and id <= 65535 and id % 1 == 0 then
                -- Prefer a stable name when a Mod registers multiple aliases.
                if not tile_names[id] or name < tile_names[id] then tile_names[id] = name end
            end
        end
    end
    if not mats then
        local definitions = require("worldtiledefs").minimap
        local count = 0
        for _, definition in pairs(definitions) do
            count = count + 1
            if count > 4096 then reject("Too many native minimap terrain definitions.") end
            local id = type(definition) == "table" and definition[1]
            local properties = id and definition[2]
            local texture = type(properties) == "table" and properties.noise_texture
            if used[id] and texture then
                -- Resolve only used fallback definitions; actual native calls are authoritative.
                local ok, resolved = pcall(resolvefilepath_soft, texture)
                texture = ok and noise(resolved or texture)
                textures[id] = texture or nil
                if not texture then invalid = invalid + 1 end
            end
        end
    end
    local excluded = {}
    if mats then
        -- These are successful calls on this shard's actual MiniMap, including
        -- Mod post-inits that never update worldtiledefs.minimap.
        textures = {}
        for _, layer in ipairs(mats.layers) do
            local texture = noise(layer.noiseTexture)
            textures[layer.tileId] = texture
            if used[layer.tileId] and not texture then invalid = invalid + 1 end
            bases[layer.tileId] = noise(layer.texture)
            if used[layer.tileId] and layer.texture and not bases[layer.tileId] then invalid = invalid + 1 end
            emit({k="layer",tileId=layer.tileId,atlas=noise(layer.atlas),texture=bases[layer.tileId],noiseTexture=texture,sampleStyle=layer.sampleStyle})
        end
        for _, id in ipairs(mats.transparentOceanExclusions or {}) do excluded[id] = true end
    end
    for id in pairs(used) do
        -- Names and aliases are labels; native tile groups determine behavior.
        local kind = TileGroupManager:IsInvalidTile(id) and "void"
            or TileGroupManager:IsOceanTile(id) and "ocean"
            or TileGroupManager:IsLandTile(id) and "land" or "unknown"
        local map_color
        -- Land also receives default colors from tilemanager.lua. Only ocean
        -- tiles use this native minimap color as their map fill.
        if kind == "ocean" and not excluded[id] then
            local ok, color = pcall(function()
                local properties = GetTileInfo(id)
                local colors = properties and properties.colors
                return colors and colors.minimap_color
            end)
            if not ok then color = false end
            if color ~= nil then
                local valid = type(color) == "table" and #color == 4
                local candidate = {}
                for channel = 1, 4 do
                    local value = valid and color[channel]
                    if type(value) ~= "number" or value ~= value or value < 0 or value > 255 or value % 1 ~= 0 then
                        valid = false
                    else
                        candidate[channel] = value
                    end
                end
                if valid then map_color = candidate else invalid = invalid + 1 end
            end
        end
        emit({ k = "name", id = id, name = clean(INVERTED_WORLD_TILES[id] or tile_names[id] or id, 128), kind = kind,
            noiseTexture = textures[id], baseTexture = bases[id], minimapColor = map_color })
    end
    if invalid > 0 then emit({k = "issue", message = "Ignored " .. invalid
        .. " invalid terrain material fields; tile IDs are retained."}) end
end
local function emit_mod_artwork()
    if not observed or not obs.installedBeforeWorld then
        emit({ k = "issue", message = "Initial Mod icons require the server's next normal startup." })
    elseif obs.issue then
        emit({ k = "issue", message = "Map icon observer: " .. clean(obs.issue, 200) })
    end
    local manager = rawget(_G, "ModManager")
    local mods = {}
    local count = 0
    for _, name in ipairs(manager and manager.enabledmods or {}) do
        count = count + 1
        if count > 128 or type(name) ~= "string" or #name > 128 then reject("Enabled map Mods exceed the collection limit.") end
        local loaded = manager:GetMod(name)
        local info = loaded and loaded.modinfo
        if type(info) ~= "table" then reject("An enabled Mod has no loaded metadata.") end
        emit({ k = "mod", name = name, version = clean(info.version, 128),
            requiredOnClient = info.all_clients_require_mod == true })
        mods[#mods + 1] = {name = name, version = clean(info.version, 128)}
    end
    if observed and obs.render then
        local ok, report, message = pcall(function()
            local component = world.minimap and world.minimap.MiniMap
            if not component then return nil, "The shard's native MiniMap is unavailable." end
            return obs.render:report(component, {gameVersion = rawget(_G, "APP_VERSION"),
                session = TheNet:GetSessionIdentifier(), width = width, height = height, mods = mods})
        end)
        if ok and report then
            mats = report
            if report.issue then emit({k="issue",message=report.issue,iconsUncertain=report.iconsUncertain}) end
            for _, path in ipairs(report.atlases) do
                if string.sub(path, 1, 5) == "data/" then path = string.sub(path, 6) end
                -- Preserve actual application order, including repeated calls.
                emit({ k = "atlas", path = path })
            end
            return
        end
        emit({ k = "issue", message = "Native map material observation is unavailable: "
            .. clean(ok and message or report, 200) })
    end
    if not manager then return end
    count = 0
    local seen = {}
    for _, atlases in ipairs(manager:GetPostInitData("MinimapAtlases")) do
        for _, path in ipairs(atlases) do
            if type(path) ~= "string" then reject("Invalid registered minimap atlas.") end
            local resolved = resolvefilepath_soft(path) or path
            if string.sub(resolved, 1, 5) == "data/" then resolved = string.sub(resolved, 6) end
            if not seen[resolved] then
                seen[resolved] = true
                count = count + 1
                if count > 512 or #resolved > 512 then reject("Registered minimap atlases exceed the collection limit.") end
                emit({ k = "atlas", path = resolved })
            end
        end
    end
end
local function emit_landmarks()
    local scanned_players, count, truncated = 0, 0, false
    local inventory_budget = {}
    -- Do not retain a next(Ents, key) cursor across ticks: removed entities can
    -- invalidate it. This single bounded traversal reads only real live entities.
    for _, entity in pairs(Ents) do
        if scanned_players >= 100000 then truncated = true break end
        scanned_players = scanned_players + 1
        local icon, priority = map_icon(entity)
        if icon and type(entity.GUID) == "number" and entity:IsValid() and entity.Transform and entity.MiniMapEntity
            and not entity:HasTag("INLIMBO") and not entity:HasTag("player")
            and not entity:HasTag("playerghost") and entity.entity:IsVisible() then
            if count >= 32768 then truncated = true break end
            if type(icon) ~= "string" or #icon > 128 or not finite(priority) then reject("Invalid observed map icon.") end
            local x, _, z = entity.Transform:GetWorldPosition()
            local map_x, map_z = coordinates(x, z)
            count = count + 1
            emit({ k = "landmark", id = tostring(entity.GUID), prefab = entity.prefab,
                icon = icon, priority = priority, mapX = map_x, mapZ = map_z,
                inventoryIcon = observed and obs.render and obs.render.inventory_icon and obs.render:inventory_icon(entity, inventory_budget) or nil })
        end
    end
    return count, truncated
end
local function emit_roads()
    if Roads == nil then return 0 end
    if type(Roads) ~= "table" then reject("Invalid native road collection.") end
    local count, total = 0, 0
    -- gamelogic.lua loads { weight, {worldX, worldZ}, ... } control points.
    for _, road in pairs(Roads) do
        if count >= 1024 then reject("Native roads exceed the 1024-road limit.") end
        if type(road) ~= "table" or not finite(road[1]) or #road < 3 or #road > 257 then
            reject("Native roads require 2 to 256 control points each.")
        end
        total = total + #road - 1
        if total > 65536 then reject("Native roads exceed the 65536-point limit.") end
        local points = {}
        local offset = 0
        for index = 2, #road do
            local point = road[index]
            if type(point) ~= "table" or #point ~= 2 then reject("Invalid native road control point.") end
            local map_x, map_z = coordinates(point[1], point[2])
            points[#points + 1] = { map_x, map_z }
            -- Keep even full-precision coordinates below the native line cap.
            if #points == 32 or index == #road then
                emit({ k = "road", index = count, offset = offset, points = points, complete = index == #road })
                offset = offset + #points
                points = {}
            end
        end
        count = count + 1
    end
    return count
end
local function finish()
    local landmarks, roads, truncated = 0, 0, false
    if terrain then
        emit_mod_artwork()
        emit_terrain_names()
        landmarks, truncated = emit_landmarks()
        roads = emit_roads()
    end
    -- AllPlayers tracks character entities until removal, which can outlive
    -- their connection. Intersect it with the game's current player clients.
    local clients = GetPlayerClientTable()
    if type(clients) ~= "table" then reject("Native player connections are unavailable.") end
    local connected, client_count = {}, 0
    for _, client in ipairs(clients) do
        client_count = client_count + 1
        if client_count > 256 then reject("Too many native player connections.") end
        if type(client) ~= "table" then reject("Invalid native player connection.") end
        if type(client.userid) == "string" and client.userid ~= "" then
            if #client.userid > 128 then reject("Native player identity exceeds the limit.") end
            connected[client.userid] = true
        end
    end
    local count, scanned_players = 0, 0
    for _, player in ipairs(AllPlayers) do
        scanned_players = scanned_players + 1
        if scanned_players > 1024 then reject("Native player entities exceed the collection limit.") end
        if player.userid and connected[player.userid] and player.Transform then
            count = count + 1
            if count > 256 then error("Too many players for map collection") end
            local x, _, z = player.Transform:GetWorldPosition()
            local map_x, map_z = coordinates(x, z)
            local entry = observed and player.MiniMapEntity and obs.entries[player.MiniMapEntity]
            local icon = entry and entry.enabled ~= false and entry.iconObserved and entry.icon or nil
            if icon == "" then icon = nil end
            emit({ k = "player", id = clean(player.userid, 128), name = clean(player.name, 256),
                character = clean(player.prefab, 128), icon = icon, x = x, z = z,
                mapX = map_x, mapZ = map_z })
        end
    end
    emit({ k = "end", players = count, landmarks = landmarks, roads = roads,
        landmarksTruncated = truncated })
    release()
end
local step
step = function()
    local ok = pcall(function()
        if TheWorld ~= world then error("The shard world changed during collection") end
        if geometry() ~= revision then
            failure_code = "geometry_changed"
            reject("Virtual rooms changed during collection.")
        end
        if GetTimeReal() >= active.expires then error("The world map query expired") end
        if not terrain then finish() return end
        -- Yield every 4096 native tile reads. No persistent timer or game mutation.
        local until_offset = math.min(width * height, offset + 4096)
        while offset < until_offset do
            local runs = {}
            local start = offset
            local stop = math.min(until_offset, start + 128)
            while offset < stop do
                local id = map:GetTile(offset % width, math.floor(offset / width))
                used[id] = true
                local last = runs[#runs]
                if last and last[1] == id then last[2] = last[2] + 1
                else runs[#runs + 1] = { id, 1 } end
                offset = offset + 1
            end
            emit({ k = "tiles", offset = start, runs = runs })
        end
        if offset == width * height then finish()
        else active.task = world:DoTaskInTime(0, step) end
    end)
    if not ok then release() emit({ k = "error", code = failure_code, message = failure or "The native world map query failed." }) end
end
step()
