-- Read-only Dragonwilds roster bridge. No game rules or player objects are modified.
-- Empty-world path verified with Dragonwilds build 24574222 and UE4SS 3.0.1-1125-g527a483b.
-- Requires EngineTick dispatch; UObject reads never fall back to LoopAsync.
local PROTOCOL = 1
local SOURCE = "net_driver_client_connections"
local MAX_PLAYERS = 256
local REQUEST_LIMIT = 512
local REQUEST_AGE = 10
local DISPATCH_TIMEOUT = 3

local script = debug.getinfo(1, "S").source:gsub("^@", "")
local root = script:match("^(.+)[/\\]RSDragonwilds[/\\]Binaries[/\\]Win64[/\\]ue4ss[/\\]Mods[/\\]LgsmPlayerQuery[/\\]Scripts[/\\]")
if not root then
    print("[LgsmPlayerQuery] Cannot resolve the dedicated server root; bridge disabled.\n")
    return
end
local directory = root .. "\\langame_player_query\\"
local request_path = directory .. "request.json"
local response_path = directory .. "response.json"
local temporary_path = directory .. "response.json.tmp"
local boot_id = string.format("%08x%08x%08x%08x", os.time() & 0xffffffff,
    math.random(0, 0x7fffffff), math.random(0, 0x7fffffff), math.random(0, 0x7fffffff))
local pending, outgoing, last_request, last_error = nil, nil, nil, nil
local generation = 0
local dispatch_pending = false

local function log_error(message)
    if last_error ~= message then
        print("[LgsmPlayerQuery] " .. message .. "\n")
        last_error = message
    end
end

local function valid(object)
    return object ~= nil and object:IsValid() == true
end

local function same(left, right)
    return valid(left) and valid(right) and left:GetAddress() == right:GetAddress()
end

local function require_condition(condition, code)
    if not condition then error(code, 0) end
end

local function array_count(array)
    local count = array:GetArrayNum()
    require_condition(type(count) == "number" and count % 1 == 0
        and count >= 0 and count <= MAX_PLAYERS, "roster_limit")
    return count
end

local function player_name(value)
    local name = value:ToString()
    require_condition(type(name) == "string" and #name > 0 and #name <= 512
        and not name:find("[%z\1-\31\127]") and name:find("%S") ~= nil
        and utf8.len(name) ~= nil, "identity_unavailable")
    return name
end

local function collect()
    require_condition(type(IsInGameThread) == "function" and IsInGameThread(), "dispatcher_unavailable")
    local selected = nil
    local drivers = FindAllOf("RedpointEOSNetDriver")
    require_condition(type(drivers) == "table" and #drivers <= 16, "world_unavailable")
    for _, driver in ipairs(drivers) do
        if valid(driver) and driver.NetDriverName:ToString() == "GameNetDriver" then
            require_condition(selected == nil, "world_ambiguous")
            selected = driver
        end
    end
    require_condition(valid(selected), "world_unavailable")
    local world = selected.World
    require_condition(valid(world) and world:GetFullName() == "World /Game/Maps/World/L_World.L_World"
        and same(world.NetDriver, selected)
        and not valid(selected.ServerConnection), "world_unavailable")
    local game_state = world.GameState
    local game_mode = world.AuthorityGameMode
    require_condition(valid(game_state) and game_state:IsA("/Script/Dominion.DominionGameStateBase")
        and valid(game_mode) and game_mode:IsA("/Script/Dominion.DominionGameMode")
        and game_state.bReplicatedHasBegunPlay == true, "world_unavailable")

    -- This native const/BlueprintPure getter was reflected on build 24574222.
    -- Reject a changed signature before invoking it; it is not an auth API.
    local ready_function = StaticFindObject("/Script/Dominion.DominionPlayerController:IsPlayerReady")
    require_condition(valid(ready_function) and ready_function:GetFunctionFlags() == 0x54020401,
        "collection_failed")
    local parameters = 0
    ready_function:ForEachProperty(function(property)
        parameters = parameters + 1
        require_condition(parameters == 1 and property:GetFName():ToString() == "ReturnValue"
            and property:GetClass():GetFName():ToString() == "BoolProperty", "collection_failed")
    end)
    require_condition(parameters == 1, "collection_failed")

    -- Cross-check the live replication roster against direct connections. A
    -- pending login, travel, teardown, or unreadable identity fails the entire
    -- snapshot instead of returning a partial list or a placeholder name.
    local expected, expected_count = {}, 0
    local state_array = game_state.PlayerArray
    local state_count = array_count(state_array)
    state_array:ForEach(function(_, parameter)
        local state = parameter:get()
        require_condition(valid(state) and state:IsA("/Script/Dominion.DominionPlayerState")
            and state.bIsInactive == false and state.bFromPreviousLevel == false
            and state.bIsABot == false, "roster_incomplete")
        local address = state:GetAddress()
        require_condition(expected[address] == nil, "roster_incomplete")
        expected[address] = true
        expected_count = expected_count + 1
    end)
    require_condition(expected_count == state_count, "roster_incomplete")

    local players, identities = {}, {}
    local connections = selected.ClientConnections
    local connection_count = array_count(connections)
    connections:ForEach(function(_, parameter)
        local connection = parameter:get()
        require_condition(valid(connection) and connection:IsA("/Script/Engine.NetConnection")
            and same(connection.Driver, selected), "roster_incomplete")
        -- Dragonwilds has no verified split-screen contract. Reject it explicitly
        -- rather than omitting child sessions from a purported complete list.
        require_condition(array_count(connection.Children) == 0, "roster_incomplete")
        local controller = connection.PlayerController
        require_condition(valid(controller) and controller:IsA("/Script/Dominion.DominionPlayerController")
            and same(connection.OwningActor, controller)
            and same(controller.Player, connection)
            and same(controller.NetConnection, connection), "roster_incomplete")
        local state = controller.PlayerState
        require_condition(valid(state) and expected[state:GetAddress()] == true
            and same(state.Owner, controller), "roster_incomplete")
        -- A connected controller can still be loading its player profile. Do
        -- not publish its identity until the game's read-only readiness check.
        require_condition(ready_function(controller) == true, "roster_incomplete")
        local id = state.PlayerId
        require_condition(type(id) == "number" and id % 1 == 0 and id >= 0
            and id <= 2147483647, "identity_unavailable")
        local session_id = string.format("%d", id)
        require_condition(identities[session_id] == nil, "roster_incomplete")
        identities[session_id] = true
        expected[state:GetAddress()] = nil
        players[#players + 1] = { name = player_name(state.PlayerNamePrivate), session_id = session_id }
    end)
    require_condition(#players == connection_count and #players == expected_count
        and next(expected) == nil and array_count(connections) == connection_count
        and array_count(state_array) == state_count, "roster_incomplete")
    table.sort(players, function(left, right) return left.session_id < right.session_id end)
    -- Capacity is not part of the verified player-identity contract. Preserve
    -- the native host's configured value instead of guessing an engine default.
    return players
end

local function quoted(value)
    return '"' .. value:gsub('[%z\1-\31\\"]', function(character)
        if character == '"' then return '\\"' end
        if character == '\\' then return '\\\\' end
        return string.format("\\u%04x", character:byte())
    end) .. '"'
end

local function encode(response)
    local rows = {}
    for _, player in ipairs(response.players) do
        rows[#rows + 1] = '{"name":' .. quoted(player.name) .. ',"session_id":' .. quoted(player.session_id) .. '}'
    end
    local value = '{"protocol":' .. PROTOCOL .. ',"request_id":' .. quoted(response.request_id)
        .. ',"boot_id":' .. quoted(boot_id) .. ',"timestamp":' .. response.timestamp
        .. ',"complete":' .. tostring(response.complete) .. ',"source":' .. quoted(SOURCE)
        .. ',"current_players":' .. #response.players .. ',"players":[' .. table.concat(rows, ',') .. ']'
    if response.max_players then value = value .. ',"max_players":' .. response.max_players end
    if response.error then value = value .. ',"error":' .. quoted(response.error) end
    return value .. '}\n'
end

local function failure(request, code)
    outgoing = { request_id = request.request_id, timestamp = os.time(), complete = false,
        players = {}, error = code }
end

local function read_request()
    local file = io.open(request_path, "rb")
    if not file then return nil end
    local content = file:read(REQUEST_LIMIT + 1)
    file:close()
    if not content or #content > REQUEST_LIMIT then return nil end
    -- The native writer emits exactly these two fields. Parse that deliberately
    -- small grammar rather than executing input or loading a JSON dependency.
    local id, timestamp = content:match('^%s*{%s*"request_id"%s*:%s*"([0-9a-f]+)"%s*,%s*"requested_at"%s*:%s*(%d+)%s*}%s*$')
    if not id then
        timestamp, id = content:match('^%s*{%s*"requested_at"%s*:%s*(%d+)%s*,%s*"request_id"%s*:%s*"([0-9a-f]+)"%s*}%s*$')
    end
    if not id or #id ~= 32 or #timestamp > 12 then return nil end
    timestamp = tonumber(timestamp)
    local age = os.time() - timestamp
    if age < -2 or age > REQUEST_AGE then return nil end
    return { request_id = id, requested_at = timestamp }
end

local function flush_response()
    if not outgoing then return end
    local response = outgoing
    local file = io.open(temporary_path, "wb")
    if not file then log_error("Response directory is unavailable."); return end
    local written = file:write(encode(response))
    local closed = file:close()
    if not written or not closed then log_error("Response write failed."); return end
    -- Windows Lua rename cannot replace an existing file. The native reader
    -- tolerates this short absence and only accepts the matching request ID.
    local removed, _, remove_code = os.remove(response_path)
    if not removed and remove_code ~= 2 then log_error("Response replacement failed."); return end
    if not os.rename(temporary_path, response_path) then log_error("Response replacement failed."); return end
    if outgoing == response then outgoing = nil end
    last_error = nil
end

LoopAsync(250, function()
    flush_response()
    local now = os.time()
    if pending and now - pending.queued_at >= DISPATCH_TIMEOUT then
        failure(pending, "dispatcher_timeout")
        pending = nil
        generation = generation + 1
        -- Keep one closure queued until the game thread drains it. Never grow
        -- UE4SS's queue when a dispatcher is unavailable or the world is paused.
    end
    local request = read_request()
    if not request or request.request_id == last_request then return false end
    last_request = request.request_id
    if dispatch_pending then failure(request, "dispatcher_unavailable"); return false end
    if type(ExecuteInGameThread) ~= "function" or type(EGameThreadMethod) ~= "table"
        or EGameThreadMethod.EngineTick == nil then
        failure(request, "dispatcher_unavailable"); return false
    end
    generation = generation + 1
    local token = generation
    pending = { request_id = request.request_id, queued_at = now }
    dispatch_pending = true
    local ok = pcall(function()
        ExecuteInGameThread(function()
            if token ~= generation then dispatch_pending = false; return end
            local success, players, maximum = pcall(collect)
            dispatch_pending = false
            -- LuaRaw serializes VM execution, but C calls may release its lock.
            -- Re-check after reflection so a timeout cannot publish old work.
            if token ~= generation then return end
            if success then
                outgoing = { request_id = request.request_id, timestamp = os.time(), complete = true,
                    players = players, max_players = maximum }
            else
                local known = { world_unavailable = true, world_ambiguous = true, roster_limit = true,
                    roster_incomplete = true, identity_unavailable = true, dispatcher_unavailable = true }
                failure(request, known[players] and players or "collection_failed")
            end
            pending = nil
        end, EGameThreadMethod.EngineTick)
    end)
    if not ok then
        dispatch_pending = false
        pending = nil
        generation = generation + 1
        failure(request, "dispatcher_unavailable")
    end
    return false
end)
