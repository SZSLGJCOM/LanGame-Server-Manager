-- Native Dragonwilds query/control bridge. Every UObject access and effect runs
-- on EngineTick. The Rust owner authenticates the process and serializes I/O.
local MAX_PLAYERS, REQUEST_LIMIT = 256, 1024
local REQUEST_AGE, DISPATCH_TIMEOUT, OPERATION_BUDGET = 10, 3, 5
local RECENT_TTL, RECENT_LIMIT = 32, 512
local SOURCE = "dragonwilds_native_control"
local script = debug.getinfo(1, "S").source:gsub("^@", "")
local root = script:match("^(.+)[/\\]RSDragonwilds[/\\]Binaries[/\\]Win64[/\\]ue4ss[/\\]Mods[/\\]LgsmPlayerControl[/\\]Scripts[/\\]")
if not root then print("[LgsmPlayerControl] Cannot resolve the server root; bridge disabled.\n"); return end
local directory = root .. "\\langame_player_control\\"
local request_path, response_path = directory .. "request.json", directory .. "response.json"
local temporary_path = response_path .. ".tmp"
local function random_token()
    return string.format("%08x%08x%08x%08x", math.random(0, 0x7fffffff),
        math.random(0, 0x7fffffff), math.random(0, 0x7fffffff), math.random(0, 0x7fffffff))
end
local boot_id = random_token()
local sessions, recent = {}, {}
local pending, outgoing, dispatch_pending = nil, nil, false
local generation = 0
local function valid(object) return object ~= nil and object:IsValid() == true end
local function same(left, right) return valid(left) and valid(right) and left:GetAddress() == right:GetAddress() end
local function require_condition(condition, code) if not condition then error(code, 0) end end
local function hex(value) return #value == 32 and value:match("^[0-9a-f]+$") ~= nil end
local function decimal(value)
    return #value > 0 and #value <= 10 and value:match("^%d+$") ~= nil
        and (#value == 1 or value:sub(1, 1) ~= "0") and tonumber(value) <= 2147483647
end
local function array_count(array, maximum)
    local count = array:GetArrayNum()
    require_condition(type(count) == "number" and count % 1 == 0 and count >= 0
        and count <= (maximum or MAX_PLAYERS), "roster_limit")
    return count
end
local function native_string(value)
    if type(value) == "string" then return value end
    return value:ToString()
end
local function name(value)
    local text = native_string(value)
    require_condition(type(text) == "string" and #text > 0 and #text <= 512
        and not text:find("[%z\1-\31\127]") and text:find("%S") and utf8.len(text), "identity_unavailable")
    return text
end
local function signature(path, flags, parameters)
    local fn = StaticFindObject(path)
    require_condition(valid(fn) and fn:GetFunctionFlags() == flags, "api_unavailable")
    local count = 0
    fn:ForEachProperty(function(property)
        count = count + 1
        local expected = parameters[count]
        require_condition(expected and property:GetFName():ToString() == expected[1]
            and property:GetClass():GetFName():ToString() == expected[2], "api_unavailable")
    end)
    require_condition(count == #parameters, "api_unavailable")
    return fn
end
local function cdo(path)
    local class = StaticFindObject(path)
    require_condition(valid(class), "api_unavailable")
    local object = class:GetCDO()
    require_condition(valid(object) and object:IsA(path), "api_unavailable")
    return object
end
local READY = "/Script/Dominion.DominionPlayerController:IsPlayerReady"
local IDS = "/Script/OnlineSubsystemBlueprints.OnlineHelpers"
local PRIVILEGES = "/Script/Dominion.PrivilegeFunctionLibrary"
local KICK = "/Script/RedpointEOSFramework.RedpointFrameworkBlueprintLibrary"
local function kick_api()
    -- Fixed UE4SS 527a483b exposes FText(string) as a callable userdata. A
    -- UFunction obtained by StaticFindObject is invoked as fn(CDO, arguments).
    local fn = signature(KICK .. ":KickPlayerController", 0x4022405,
        {{ "PlayerController", "ObjectProperty" }, { "KickReason", "TextProperty" }, { "ReturnValue", "BoolProperty" }})
    return { fn = fn, self = cdo(KICK), reason = FText("Removed by the server administrator.") }
end
local native_function
local native_exchange
local function persisted_bans()
    local file = io.open(root .. "\\RSDragonwilds\\Saved\\Config\\WindowsServer\\DedicatedServer.ini", "rb")
    require_condition(file ~= nil, "persistence_unavailable")
    local content = file:read(8 * 1024 * 1024 + 1); file:close()
    require_condition(content and #content <= 8 * 1024 * 1024, "persistence_unavailable")
    if content:sub(1, 2) == "\255\254" then
        require_condition(#content % 2 == 0, "persistence_unavailable")
        local ascii = {}
        for index = 3, #content, 2 do
            local low, high = content:byte(index, index + 1)
            ascii[#ascii + 1] = high == 0 and string.char(low) or "?"
        end
        content = table.concat(ascii)
    elseif content:sub(1, 3) == "\239\187\191" then content = content:sub(4) end
    local result, in_section, sections = {}, false, 0
    for line in (content .. "\n"):gmatch("([^\n]*)\n") do
        line = line:gsub("\r$", "")
        if line:match("^%s*%[") then
            in_section = line:match("^%s*%[/Script/Dominion%.DedicatedServerSettings%]%s*$") ~= nil
            if in_section then sections = sections + 1 end
        elseif in_section and line:match("^%s*KnownPlayerList=") then
            -- Verified native export: the account is the first field and the
            -- ban flag is the final field. Never split unquoted player names on
            -- commas or echo lines that can contain future password fields.
            local account = line:match("^%s*KnownPlayerList=%(UserId=([0-9a-f]+),")
            local flag = line:match(",bIsBanned=(%a+)%)%s*$")
            require_condition(account and hex(account) and (flag == "True" or flag == "False")
                and result[account] == nil, "persistence_unavailable")
            result[account] = flag == "True"
        end
    end
    require_condition(sections == 1, "persistence_unavailable")
    return result
end
local function native_context(world, id_fn, id_cdo)
    if not native_function then
        local dll = root .. "\\RSDragonwilds\\Binaries\\Win64\\ue4ss\\Mods\\LgsmPlayerControl\\native\\LgsmPlayerControl.dll"
        native_function = package.loadlib(dll, "luaopen_lgsm_dragonwilds_control")
    end
    require_condition(type(native_function) == "function", "api_unavailable")
    local candidates = FindAllOf("DomMatchmakerSubsystem")
    require_condition(type(candidates) == "table" and #candidates <= 4, "api_unavailable")
    local matchmaker
    for _, candidate in ipairs(candidates) do
        if valid(candidate) and candidate:GetClass():GetFullName() == "Class /Script/Dominion.DomMatchmakerSubsystem"
            and same(candidate:GetOuter(), world.OwningGameInstance) then
            require_condition(matchmaker == nil, "api_unavailable"); matchmaker = candidate
        end
    end
    require_condition(valid(matchmaker), "api_unavailable")
    local settings = cdo("/Script/Dominion.DedicatedServerSettings")
    local known, count = {}, 0
    local rows = settings.KnownPlayerList
    local total = array_count(rows, 4096)
    rows:ForEach(function(index, parameter)
        local row = parameter:get()
        local account = native_string(id_fn(id_cdo, row.UserId))
        require_condition(type(index) == "number" and index == count + 1
            and type(account) == "string" and hex(account) and known[account] == nil
            and type(row.bIsBanned) == "boolean", "identity_unavailable")
        known[account] = { index = index - 1, banned = row.bIsBanned }
        count = count + 1
    end)
    require_condition(count == total and array_count(rows, 4096) == total, "roster_incomplete")
    local context = { matchmaker = matchmaker, settings = settings, known = known, persisted = persisted_bans() }
    -- A loaded DLL is not proof that its fingerprint, game-thread and object
    -- gates passed. Only its read-only probe receipt may advertise native ban.
    native_exchange(context, { request_id = random_token(), action = "probe" }, 0, "ready")
    return context
end
native_exchange = function(context, request, index, expected_status, validate_current)
    local receipt = directory .. "native-response.txt"
    local removed, _, code = os.remove(receipt)
    require_condition(removed or code == 2, "persistence_unavailable")
    local file = io.open(directory .. "native-request.txt", "wb")
    require_condition(file ~= nil, "persistence_unavailable")
    local content = "schema=1\nnonce=" .. request.request_id .. "\nop=" .. request.action
        .. "\nmatchmaker_address=" .. string.format("%d", context.matchmaker:GetAddress())
        .. "\nsettings_address=" .. string.format("%d", context.settings:GetAddress())
        .. "\nknown_player_index=" .. index .. "\n"
    local written, closed = file:write(content), file:close()
    require_condition(written and closed, "persistence_unavailable")
    if validate_current then validate_current() end
    native_function()
    file = io.open(receipt, "rb")
    require_condition(file ~= nil, "effect_unconfirmed")
    local response = file:read(1025); file:close()
    require_condition(response and #response <= 1024, "effect_unconfirmed")
    local nonce, action, status, err = response:match("^schema=1\nnonce=([0-9a-f]+)\nop=([a-z]+)\nstatus=([a-z]+)\nerror=([a-z_]*)\n$")
    require_condition(nonce == request.request_id and action == request.action
        and status == expected_status and err == "", "effect_unconfirmed")
end
local function native_dispatch(context, request, validate_current)
    local known = context.known[request.account_id]
    require_condition(known and context.persisted[request.account_id] == known.banned, "persistence_unavailable")
    require_condition(known.banned ~= (request.action == "ban"), "target_changed")
    native_exchange(context, request, known.index, "dispatched", validate_current)
end
local function collect()
    require_condition(type(IsInGameThread) == "function" and IsInGameThread(), "dispatcher_unavailable")
    local driver
    local drivers = FindAllOf("RedpointEOSNetDriver")
    require_condition(type(drivers) == "table" and #drivers <= 16, "world_unavailable")
    for _, candidate in ipairs(drivers) do
        if valid(candidate) and candidate.NetDriverName:ToString() == "GameNetDriver" then
            require_condition(driver == nil, "world_ambiguous"); driver = candidate
        end
    end
    require_condition(valid(driver), "world_unavailable")
    local world = driver.World
    require_condition(valid(world) and world:GetFullName() == "World /Game/Maps/World/L_World.L_World"
        and same(world.NetDriver, driver) and not valid(driver.ServerConnection), "world_unavailable")
    local gs, gm = world.GameState, world.AuthorityGameMode
    require_condition(valid(gs) and gs:IsA("/Script/Dominion.DominionGameStateBase")
        and valid(gm) and gm:IsA("/Script/Dominion.DominionGameMode")
        and gs.bReplicatedHasBegunPlay == true, "world_unavailable")
    local ready = signature(READY, 0x54020401, {{ "ReturnValue", "BoolProperty" }})
    local id_fn = signature(IDS .. ":Conv_FUniqueNetIdReplToString", 0x14422401,
        {{ "InNetId", "StructProperty" }, { "ReturnValue", "StrProperty" }})
    local valid_id = signature(IDS .. ":FUniqueNetIdIsValid", 0x14422401,
        {{ "InNetId", "StructProperty" }, { "ReturnValue", "BoolProperty" }})
    local id_type = signature(IDS .. ":FUniqueNetIdGetType", 0x14422401,
        {{ "InNetId", "StructProperty" }, { "ReturnValue", "NameProperty" }})
    local admin_fn = signature(PRIVILEGES .. ":ControllerIsOwnerOrAdmin", 0x14022401,
        {{ "Controller", "ObjectProperty" }, { "ReturnValue", "BoolProperty" }})
    local id_cdo, admin_cdo = cdo(IDS), cdo(PRIVILEGES)
    local kick_ok, kick = pcall(kick_api)
    local native_ok, native = pcall(native_context, world, id_fn, id_cdo)
    local expected, expected_count = {}, 0
    local states, connections = gs.PlayerArray, driver.ClientConnections
    local state_count, connection_count = array_count(states), array_count(connections)
    states:ForEach(function(_, parameter)
        local state = parameter:get()
        require_condition(valid(state) and state:IsA("/Script/Dominion.DominionPlayerState")
            and state.bIsInactive == false and state.bFromPreviousLevel == false
            and state.bIsABot == false, "roster_incomplete")
        local address = state:GetAddress()
        require_condition(expected[address] == nil, "roster_incomplete")
        expected[address], expected_count = true, expected_count + 1
    end)
    require_condition(expected_count == state_count, "roster_incomplete")
    local players, ids, accounts, tokens, next_sessions = {}, {}, {}, {}, {}
    connections:ForEach(function(_, parameter)
        local connection = parameter:get()
        require_condition(valid(connection) and connection:IsA("/Script/Engine.NetConnection")
            and same(connection.Driver, driver) and array_count(connection.Children) == 0, "roster_incomplete")
        local controller = connection.PlayerController
        require_condition(valid(controller) and controller:IsA("/Script/Dominion.DominionPlayerController")
            and same(connection.OwningActor, controller) and same(controller.Player, connection)
            and same(controller.NetConnection, connection), "roster_incomplete")
        local state = controller.PlayerState
        require_condition(valid(state) and expected[state:GetAddress()] and same(state.Owner, controller)
            and ready(controller) == true, "roster_incomplete")
        local numeric = state.PlayerId
        require_condition(type(numeric) == "number" and numeric % 1 == 0
            and numeric >= 0 and numeric <= 2147483647, "identity_unavailable")
        local id = string.format("%d", numeric)
        require_condition(valid_id(id_cdo, state.UniqueID) == true
            and native_string(id_type(id_cdo, state.UniqueID)):lower() == "redpointeos", "identity_unavailable")
        local account = native_string(id_fn(id_cdo, state.UniqueID))
        local admin = admin_fn(admin_cdo, controller)
        require_condition(type(account) == "string" and hex(account) and type(admin) == "boolean"
            and not ids[id] and not accounts[account], "identity_unavailable")
        ids[id], accounts[account], expected[state:GetAddress()] = true, true, nil
        local address = controller:GetAddress()
        local fingerprint = controller:GetFullName() .. ":" .. state:GetFullName() .. ":" .. connection:GetFullName()
        local previous = sessions[address]
        local token = previous and previous.fingerprint == fingerprint and previous.id == id
            and previous.account == account and previous.connection == connection:GetAddress() and previous.token
        if not token then token = random_token() end
        require_condition(not tokens[token], "identity_unavailable")
        tokens[token] = true
        next_sessions[address] = { fingerprint = fingerprint, id = id, account = account,
            connection = connection:GetAddress(), token = token }
        local actions = kick_ok and { "kick_player" } or {}
        if native_ok and native.known[account] and native.known[account].banned == false
            and native.persisted[account] == false then actions[#actions + 1] = "ban_player" end
        players[#players + 1] = { name = name(state.PlayerNamePrivate), session_id = id,
            account_id = account, session_token = token, admin = admin,
            actions = actions, controller = controller, connection = connection }
    end)
    require_condition(#players == connection_count and #players == expected_count and next(expected) == nil
        and array_count(states) == state_count and array_count(connections) == connection_count, "roster_incomplete")
    sessions = next_sessions
    table.sort(players, function(left, right) return tonumber(left.session_id) < tonumber(right.session_id) end)
    return players, kick_ok and kick or nil, native_ok and native or nil
end
local function quoted(value)
    return '"' .. value:gsub('[%z\1-\31\\"]', function(character)
        if character == '"' then return '\\"' end
        if character == '\\' then return '\\\\' end
        return string.format("\\u%04x", character:byte())
    end) .. '"'
end
local function encode(response)
    local request = response.request
    local value = '{"protocol":1,"request_id":' .. quoted(request.request_id) .. ',"timestamp":' .. os.time()
    if request.action == "query" then
        local rows = {}
        for _, player in ipairs(response.players or {}) do
            local actions = {}; for _, action in ipairs(player.actions) do actions[#actions + 1] = quoted(action) end
            rows[#rows + 1] = '{"name":' .. quoted(player.name) .. ',"session_id":' .. quoted(player.session_id)
                .. ',"account_id":' .. quoted(player.account_id) .. ',"session_token":' .. quoted(player.session_token)
                .. ',"admin":' .. tostring(player.admin) .. ',"actions":[' .. table.concat(actions, ',') .. ']}'
        end
        value = value .. ',"boot_id":' .. quoted(boot_id) .. ',"source":' .. quoted(SOURCE)
            .. ',"complete":' .. tostring(response.complete) .. ',"current_players":' .. #rows
            .. ',"players":[' .. table.concat(rows, ',') .. ']'
    else
        value = value .. ',"action":' .. quoted(request.action) .. ',"boot_id":' .. quoted(request.boot_id)
            .. ',"session_id":' .. quoted(request.session_id) .. ',"session_token":' .. quoted(request.session_token)
            .. ',"account_id":' .. quoted(request.account_id) .. ',"complete":' .. tostring(response.complete)
            .. ',"applied":' .. tostring(response.applied == true)
    end
    return value .. ',"error":' .. (response.error and quoted(response.error) or 'null') .. '}\n'
end
local function failure(request, code)
    outgoing = { request = request, complete = false, applied = false, error = code }
end
local function read_request()
    local file = io.open(request_path, "rb"); if not file then return nil end
    local content = file:read(REQUEST_LIMIT + 1); file:close()
    if not content or #content > REQUEST_LIMIT then return nil end
    -- This deliberately closed grammar accepts only the owning Rust writer's
    -- exact key order, without escapes, duplicate fields, extra fields or eval.
    local id, timestamp, action, boot, session, token, account = content:match(
        '^%s*{%s*"protocol"%s*:%s*1%s*,%s*"request_id"%s*:%s*"([0-9a-f]+)"%s*,%s*"requested_at"%s*:%s*(%d+)%s*,%s*"action"%s*:%s*"([a-z]+)"%s*,%s*"boot_id"%s*:%s*"([0-9a-f]*)"%s*,%s*"session_id"%s*:%s*"(%d*)"%s*,%s*"session_token"%s*:%s*"([0-9a-f]*)"%s*,%s*"account_id"%s*:%s*"([0-9a-f]*)"%s*}%s*$')
    if not id or not hex(id) or #timestamp > 12 or (#timestamp > 1 and timestamp:sub(1, 1) == "0") then return nil end
    timestamp = tonumber(timestamp)
    local age = os.time() - timestamp
    if age < -2 or age > REQUEST_AGE then return nil end
    if action == "query" then
        if boot ~= "" or session ~= "" or token ~= "" or account ~= "" then return nil end
    elseif action == "kick" or action == "ban" then
        if not hex(boot) or not decimal(session) or not hex(token) or not hex(account) then return nil end
    elseif action == "unban" then
        if not hex(boot) or session ~= "" or token ~= "" or not hex(account) then return nil end
    else return nil end
    return { request_id = id, requested_at = timestamp, action = action, boot_id = boot,
        session_id = session, session_token = token, account_id = account }
end
local function fresh(request)
    local age = os.time() - request.requested_at
    return age >= -2 and age <= OPERATION_BUDGET
end
local function effect_fresh(request)
    local age = os.time() - request.requested_at
    -- Seconds are coarse: <=3 means an effect starts before the fourth second,
    -- leaving room inside the host's precise five-second response deadline.
    return age >= -2 and age <= 3
end
local function target_row(players, request)
    for _, player in ipairs(players) do
        if player.session_id == request.session_id and player.session_token == request.session_token
            and player.account_id == request.account_id then return player end
    end
end
local known = { dispatcher_unavailable = true, dispatcher_timeout = true, api_unavailable = true, world_unavailable = true,
    world_ambiguous = true, roster_limit = true, roster_incomplete = true, identity_unavailable = true,
    request_expired = true, target_changed = true, native_rejected = true, effect_unconfirmed = true,
    persistence_unavailable = true }
local function error_code(value) return known[value] and value or "collection_failed" end
local function schedule()
    if dispatch_pending or not pending then return end
    local operation, token = pending, generation
    dispatch_pending, operation.queued_at = true, os.time()
    local ok = pcall(function()
        ExecuteInGameThread(function()
            if token ~= generation or pending ~= operation then dispatch_pending = false; return end
            local request = operation.request
            local success, result, kick = pcall(function()
                local function validate_current(effect)
                    require_condition(token == generation and pending == operation and fresh(request), "request_expired")
                    require_condition(not effect or effect_fresh(request), "request_expired")
                    local queued_age = os.time() - operation.queued_at
                    require_condition(queued_age >= 0 and queued_age < DISPATCH_TIMEOUT, "dispatcher_timeout")
                end
                require_condition(type(IsInGameThread) == "function" and IsInGameThread(), "dispatcher_unavailable")
                validate_current()
                local players, api, native = collect()
                validate_current()
                if request.action == "query" then return players end
                require_condition(request.boot_id == boot_id, "target_changed")
                local target = target_row(players, request)
                if operation.confirming then
                    if request.action == "kick" then return target == nil, nil end
                    require_condition(native ~= nil, "effect_unconfirmed")
                    local row = native.known[request.account_id]
                    local desired = request.action == "ban"
                    local confirmed = row and row.banned == desired and native.persisted[request.account_id] == desired
                    local connected = false
                    for _, player in ipairs(players) do
                        if player.account_id == request.account_id then connected = true end
                    end
                    return confirmed and (request.action == "unban" or not connected), nil
                end
                if request.action == "ban" or request.action == "unban" then
                    require_condition(native ~= nil, "api_unavailable")
                    require_condition(request.action == "unban" or target ~= nil, "target_changed")
                    native_dispatch(native, request, function() validate_current(true) end)
                    return false, true
                end
                require_condition(target ~= nil, "target_changed")
                require_condition(api ~= nil, "api_unavailable")
                -- No actor, name, controller or account is accepted from the
                -- request; the validated current roster supplies the object.
                validate_current(true)
                require_condition(api.fn(api.self, target.controller, api.reason) == true, "native_rejected")
                return false, true
            end)
            dispatch_pending = false
            if token ~= generation or pending ~= operation then return end
            if not success then
                if operation.confirming and fresh(request) and (result == "roster_incomplete" or result == "identity_unavailable") then return end
                failure(request, operation.confirming and "effect_unconfirmed" or error_code(result)); pending = nil; return
            end
            if request.action == "query" then
                outgoing = { request = request, complete = true, players = result }; pending = nil
            elseif result == true then
                outgoing = { request = request, complete = true, applied = true }; pending = nil
            elseif kick then
                operation.confirming = true
            end
        end, EGameThreadMethod.EngineTick)
    end)
    if not ok then
        dispatch_pending = false; pending = nil; generation = generation + 1
        failure(operation.request, operation.confirming and "effect_unconfirmed" or "dispatcher_unavailable")
    end
end
local function flush()
    if not outgoing then return end
    local response = outgoing
    local file = io.open(temporary_path, "wb")
    if not file then return end
    local written, closed = file:write(encode(response)), file:close()
    if not written or not closed then return end
    local removed, _, code = os.remove(response_path)
    if not removed and code ~= 2 then return end
    if not os.rename(temporary_path, response_path) then return end
    if outgoing == response then outgoing = nil end
end
LoopAsync(250, function()
    flush()
    local now = os.time()
    if pending and (not fresh(pending.request) or (dispatch_pending and now - pending.queued_at >= DISPATCH_TIMEOUT)) then
        failure(pending.request, pending.confirming and "effect_unconfirmed" or "dispatcher_timeout")
        pending = nil; generation = generation + 1
        -- A timed-out queued closure remains owned until it drains. No second
        -- effect can be queued behind it, and its generation cannot execute.
    end
    if pending and not dispatch_pending then schedule() end
    local request = read_request(); if not request then return false end
    local count = 0
    for id, timestamp in pairs(recent) do
        if now - timestamp > RECENT_TTL then recent[id] = nil else count = count + 1 end
    end
    if recent[request.request_id] then return false end
    -- The TTL exceeds the complete request validity window. An original file
    -- cannot become executable again when its recent-nonce record expires.
    if count >= RECENT_LIMIT then failure(request, "request_limit"); return false end
    recent[request.request_id] = now
    if pending or dispatch_pending then failure(request, "dispatcher_unavailable"); return false end
    if type(ExecuteInGameThread) ~= "function" or type(EGameThreadMethod) ~= "table"
        or EGameThreadMethod.EngineTick == nil then failure(request, "dispatcher_unavailable"); return false end
    generation = generation + 1
    pending = { request = request, confirming = false }
    schedule()
    return false
end)
