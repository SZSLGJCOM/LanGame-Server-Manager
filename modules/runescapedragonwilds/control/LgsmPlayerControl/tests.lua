-- Pure Lua behavioral harness. Usage: lua-harness tests.lua Scripts/main.lua.
-- All game, filesystem and native DLL boundaries below are deterministic mocks.
local production = assert(arg[1], "production main.lua path required")
local tests, failures = 0, {}
local base = "D:/probe\\langame_player_control\\"
local ini = "D:/probe\\RSDragonwilds\\Saved\\Config\\WindowsServer\\DedicatedServer.ini"
local invalid = { IsValid = function() return false end }
local function text(value) return { ToString = function() return value end } end
local function object(address, class)
    return { IsValid = function() return true end, GetAddress = function() return address end,
        IsA = function(_, expected) return expected == class end,
        GetFullName = function() return class .. "_" .. address end,
        GetClass = function() return { GetFullName = function() return "Class " .. class end } end }
end
local function array(rows)
    return { GetArrayNum = function() return #rows end, ForEach = function(_, callback)
        for index, value in ipairs(rows) do callback(index, { get = function() return value end }) end
    end }
end
local function harness(count)
    local state = { now = 1000, files = {}, queue = {}, rows = {}, connections = {}, known = {},
        kicks = 0, native_calls = 0, probes = 0, native_available = false, kick_result = true, remove_on_kick = true,
        persist_native = true, apply_native = true, remove_on_ban = true, random_counter = 0 }
    local driver = object(1, "/Script/RedpointEOSNetworking.RedpointEOSNetDriver")
    local world = object(2, "/Script/Engine.World")
    local gs, gm = object(3, "/Script/Dominion.DominionGameStateBase"), object(4, "/Script/Dominion.DominionGameMode")
    local instance = object(5, "/Script/Engine.GameInstance")
    world.GetFullName = function() return "World /Game/Maps/World/L_World.L_World" end
    world.NetDriver, world.GameState, world.AuthorityGameMode, world.OwningGameInstance = driver, gs, gm, instance
    driver.World, driver.ServerConnection, driver.NetDriverName = world, invalid, text("GameNetDriver")
    gs.bReplicatedHasBegunPlay = true
    for index = 1, count do
        local connection = object(100 + index, "/Script/Engine.NetConnection")
        local pc = object(200 + index, "/Script/Dominion.DominionPlayerController")
        local ps = object(300 + index, "/Script/Dominion.DominionPlayerState")
        connection.Driver, connection.PlayerController, connection.OwningActor = driver, pc, pc
        connection.Children = array({})
        pc.Player, pc.NetConnection, pc.PlayerState, pc.ready, pc.admin = connection, connection, ps, true, false
        ps.Owner, ps.PlayerId, ps.PlayerNamePrivate = pc, index, text('玩家 "' .. index .. '" \\ name')
        ps.bIsInactive, ps.bFromPreviousLevel, ps.bIsABot = false, false, false
        ps.UniqueID = { account = string.format("%032x", index), valid = true, kind = "RedpointEOS" }
        state.rows[index], state.connections[index] = ps, connection
        state.known[index] = { UserId = ps.UniqueID, bIsBanned = false, name = "Player " .. index }
    end
    gs.PlayerArray, driver.ClientConnections = array(state.rows), array(state.connections)
    state.driver, state.world, state.gs, state.gm = driver, world, gs, gm
    local settings = object(10, "/Script/Dominion.DedicatedServerSettings")
    settings.KnownPlayerList = array(state.known)
    local matchmaker = object(11, "/Script/Dominion.DomMatchmakerSubsystem")
    matchmaker.GetOuter = function() return instance end
    state.settings, state.matchmaker = settings, matchmaker
    function state.sync_ini()
        local lines = { "[/Script/Dominion.DedicatedServerSettings]" }
        for _, row in ipairs(state.known) do
            lines[#lines + 1] = "KnownPlayerList=(UserId=" .. row.UserId.account .. ",UserName=" .. row.name
                .. ",Privileges=(PrivilegeMask=14),bIsBanned=" .. (row.bIsBanned and "True" or "False") .. ")"
        end
        state.files[ini] = table.concat(lines, "\r\n") .. "\r\n"
    end
    state.sync_ini()
    function state.remove(controller)
        for index = #state.connections, 1, -1 do
            if state.connections[index].PlayerController == controller then table.remove(state.connections, index) end
        end
        for index = #state.rows, 1, -1 do
            if state.rows[index].Owner == controller then table.remove(state.rows, index) end
        end
    end
    local environment = setmetatable({}, { __index = _G })
    environment.math = setmetatable({ random = function(minimum, maximum)
        assert(minimum == 0 and maximum == 0x7fffffff)
        state.random_counter = state.random_counter + 1; return state.random_counter
    end }, { __index = math })
    environment.os = { time = function() return state.now end, remove = function(path)
        if state.remove_failure then return nil, "denied", 13 end
        if state.files[path] then state.files[path] = nil; return true end
        return nil, "missing", 2
    end, rename = function(from, to) state.files[to], state.files[from] = state.files[from], nil; return true end }
    environment.io = { open = function(path, mode)
        if state.open_failure == path then return nil end
        if mode == "rb" then
            local content = state.files[path]; if not content then return nil end
            return { read = function(_, limit) return content:sub(1, limit) end, close = function() return true end }
        end
        return { write = function(_, content)
                if state.native_write_failure and path == base .. "native-request.txt"
                    and content:find("\nop=ban\n", 1, true) then return nil, "write denied" end
                state.files[path] = content; return true
            end,
            close = function() return true end }
    end }
    environment.print = function() end
    environment.debug = { getinfo = function() return { source = "@D:/probe/RSDragonwilds/Binaries/Win64/ue4ss/Mods/LgsmPlayerControl/Scripts/main.lua" } end }
    environment.LoopAsync = function(delay, callback) assert(delay == 250); state.tick = callback end
    environment.ExecuteInGameThread = function(callback, method)
        assert(method == 0, "must select EngineTick"); state.queue[#state.queue + 1] = callback
    end
    environment.EGameThreadMethod = { EngineTick = 0 }
    environment.IsInGameThread = function() return state.in_game_thread == true end
    environment.FindAllOf = function(class)
        assert(state.in_game_thread, "no off-thread UObject reads")
        if class == "RedpointEOSNetDriver" then return { driver } end
        assert(class == "DomMatchmakerSubsystem"); return { matchmaker }
    end
    environment.FText = function(value)
        assert(state.in_game_thread and type(value) == "string")
        if state.ftext_failure then error("FText unavailable") end
        return text(value)
    end
    local ids = "/Script/OnlineSubsystemBlueprints.OnlineHelpers"
    local privileges = "/Script/Dominion.PrivilegeFunctionLibrary"
    local kick = "/Script/RedpointEOSFramework.RedpointFrameworkBlueprintLibrary"
    local functions = {
        ["/Script/Dominion.DominionPlayerController:IsPlayerReady"] = {
            flags = 0x54020401, properties = {{ "ReturnValue", "BoolProperty" }},
            call = function(pc) return pc.ready end },
        [ids .. ":Conv_FUniqueNetIdReplToString"] = {
            flags = 0x14422401, properties = {{ "InNetId", "StructProperty" }, { "ReturnValue", "StrProperty" }},
            call = function(_, uid)
                if state.collect_delay then state.now = state.now + state.collect_delay; state.collect_delay = nil end
                return text(uid.account)
            end },
        [ids .. ":FUniqueNetIdIsValid"] = {
            flags = 0x14422401, properties = {{ "InNetId", "StructProperty" }, { "ReturnValue", "BoolProperty" }},
            call = function(_, uid) return uid.valid end },
        [ids .. ":FUniqueNetIdGetType"] = {
            flags = 0x14422401, properties = {{ "InNetId", "StructProperty" }, { "ReturnValue", "NameProperty" }},
            call = function(_, uid) return text(uid.kind) end },
        [privileges .. ":ControllerIsOwnerOrAdmin"] = {
            flags = 0x14022401, properties = {{ "Controller", "ObjectProperty" }, { "ReturnValue", "BoolProperty" }},
            call = function(_, pc) return pc.admin end },
        [kick .. ":KickPlayerController"] = {
            flags = 0x4022405,
            properties = {{ "PlayerController", "ObjectProperty" }, { "KickReason", "TextProperty" }, { "ReturnValue", "BoolProperty" }},
            call = function(_, pc, reason)
                assert(reason:ToString() == "Removed by the server administrator.")
                state.kicks = state.kicks + 1
                if state.remove_on_kick and state.kick_result == true then state.remove(pc) end
                return state.kick_result
            end },
    }
    state.functions = functions
    environment.StaticFindObject = function(path)
        assert(state.in_game_thread, "no off-thread reflection")
        local spec = functions[path]
        if spec then return setmetatable({ IsValid = function() return true end,
            GetFunctionFlags = function() return spec.flags end, ForEachProperty = function(_, callback)
                for _, property in ipairs(spec.properties) do callback({ GetFName = function() return text(property[1]) end,
                    GetClass = function() return { GetFName = function() return text(property[2]) end } end }) end
            end }, { __call = function(_, ...) assert(state.in_game_thread); return spec.call(...) end }) end
        local default = path == "/Script/Dominion.DedicatedServerSettings" and settings or object(20, path)
        return { IsValid = function() return true end, GetCDO = function() return default end }
    end
    environment.package = { loadlib = function(path, symbol)
        assert(state.in_game_thread and path == "D:/probe\\RSDragonwilds\\Binaries\\Win64\\ue4ss\\Mods\\LgsmPlayerControl\\native\\LgsmPlayerControl.dll")
        assert(symbol == "luaopen_lgsm_dragonwilds_control")
        if not state.native_available then return nil, "not installed" end
        return function(...)
            assert(select("#", ...) == 0 and state.in_game_thread)
            local request = assert(state.files[base .. "native-request.txt"])
            local id, operation, match_address, settings_address, index = request:match(
                "^schema=1\nnonce=([0-9a-f]+)\nop=([a-z]+)\nmatchmaker_address=(%d+)\nsettings_address=(%d+)\nknown_player_index=(%d+)\n$")
            assert(id and match_address == "11" and settings_address == "10")
            if operation == "probe" then
                state.probes = state.probes + 1
                state.files[base .. "native-response.txt"] = "schema=1\nnonce=" .. id
                    .. "\nop=probe\nstatus=" .. (state.probe_failure and "failed" or "ready") .. "\nerror=\n"
                return
            end
            state.native_calls = state.native_calls + 1
            local row = assert(state.known[tonumber(index) + 1])
            if state.apply_native then row.bIsBanned = operation == "ban" end
            if state.persist_native then state.sync_ini() end
            if operation == "ban" and state.remove_on_ban then
                for number = #state.connections, 1, -1 do
                    local controller = state.connections[number].PlayerController
                    if controller.PlayerState.UniqueID.account == row.UserId.account then state.remove(controller) end
                end
            end
            if not state.omit_native_receipt then
                state.files[base .. "native-response.txt"] = "schema=1\nnonce=" .. (state.receipt_nonce or id)
                    .. "\nop=" .. operation .. "\nstatus=dispatched\nerror=\n"
            end
        end
    end }
    state.environment = environment
    assert(loadfile(production, "t", environment))()
    state.sequence = 0
    function state.request(action, target, id, timestamp)
        state.sequence = state.sequence + 1
        id = id or string.format("%032x", state.sequence)
        target = target or {}
        state.last_nonce = id
        state.files[base .. "request.json"] = '{"protocol":1,"request_id":"' .. id
            .. '","requested_at":' .. (timestamp or state.now) .. ',"action":"' .. (action or "query")
            .. '","boot_id":"' .. (target.boot_id or "") .. '","session_id":"' .. (target.session_id or "")
            .. '","session_token":"' .. (target.session_token or "") .. '","account_id":"' .. (target.account_id or "") .. '"}'
        state.tick(); return id
    end
    function state.drain()
        local queue = state.queue; state.queue = {}
        for _, callback in ipairs(queue) do state.in_game_thread = true; callback(); state.in_game_thread = false end
        state.tick()
    end
    function state.response() return state.files[base .. "response.json"] or "" end
    function state.query() state.request(); state.drain(); return state.response() end
    function state.target(index)
        local account = state.rows[index].UniqueID.account
        local response = state.response()
        return { boot_id = assert(response:match('"boot_id":"([0-9a-f]+)"')),
            session_id = tostring(state.rows[index].PlayerId), account_id = account,
            session_token = assert(response:match('"account_id":"' .. account .. '","session_token":"([0-9a-f]+)"')) }
    end
    return state
end
local function test(description, callback)
    tests = tests + 1
    local ok, err = pcall(callback)
    if not ok then failures[#failures + 1] = description .. ": " .. tostring(err) end
end
local function has(response, text) assert(response:find(text, 1, true), response) end
local function failure(state, code)
    state.drain(); state.tick()
    has(state.response(), '"complete":false'); has(state.response(), '"error":"' .. code .. '"')
end

test("empty query waits for EngineTick and reports a complete roster", function()
    local state = harness(0); state.request(); assert(state.response() == "" and #state.queue == 1)
    state.drain(); has(state.response(), '"complete":true'); has(state.response(), '"current_players":0')
end)
test("roles and accounts come from native getters", function()
    local state = harness(2); state.rows[2].Owner.admin = true
    local response = state.query(); has(response, '"admin":false'); has(response, '"admin":true')
    has(response, '"account_id":"' .. string.format("%032x", 1) .. '"')
    has(response, '"actions":["kick_player"]'); assert(not response:find("ban_player", 1, true))
end)
test("same names never merge distinct accounts", function()
    local state = harness(2); state.rows[2].PlayerNamePrivate = state.rows[1].PlayerNamePrivate
    has(state.query(), '"current_players":2')
end)
test("invalid and foreign account namespaces cannot authorize actions", function()
    for _, field in ipairs({ "account", "valid", "kind" }) do
        local state = harness(1)
        state.rows[1].UniqueID[field] = ({ account = "bad", valid = false, kind = "STEAM" })[field]
        state.request(); failure(state, "identity_unavailable"); assert(state.kicks == 0)
    end
end)
test("native FName EOS namespace comparison ignores ASCII case", function()
    local state = harness(1); state.rows[1].UniqueID.kind = "REDPOINTEOS"
    has(state.query(), '"complete":true')
end)
test("duplicate accounts and sessions fail complete collection", function()
    local state = harness(2); state.rows[2].UniqueID.account = state.rows[1].UniqueID.account
    state.request(); failure(state, "identity_unavailable")
    state = harness(2); state.rows[2].PlayerId = state.rows[1].PlayerId
    state.request(); failure(state, "identity_unavailable")
end)
test("signature drift removes kick capability without inventing success", function()
    local state = harness(1)
    state.functions["/Script/RedpointEOSFramework.RedpointFrameworkBlueprintLibrary:KickPlayerController"].flags = 0
    has(state.query(), '"actions":[]')
    state.request("kick", state.target(1)); failure(state, "api_unavailable"); assert(state.kicks == 0)
end)
test("unavailable FText keeps query read only", function()
    local state = harness(1); state.ftext_failure = true
    has(state.query(), '"actions":[]')
end)
test("nonboolean admin getter is not silently replaced with false", function()
    local state = harness(1); state.rows[1].Owner.admin = nil
    state.request(); failure(state, "identity_unavailable")
end)
test("kick calls only the current bound controller then confirms a later roster", function()
    local state = harness(2); state.query(); local target = state.target(2)
    state.request("kick", target); state.drain()
    assert(state.kicks == 1 and #state.rows == 1)
    assert(not state.response():find('"applied":true', 1, true))
    state.drain(); has(state.response(), '"complete":true'); has(state.response(), '"applied":true')
    assert(state.rows[1].PlayerId == 1)
end)
test("native false never reports a successful kick", function()
    local state = harness(1); state.query(); local target = state.target(1); state.kick_result = false
    state.request("kick", target); failure(state, "native_rejected")
    assert(state.kicks == 1 and #state.rows == 1)
end)
test("accepted kick without disconnection expires with an unknown effect", function()
    local state = harness(1); state.query(); local target = state.target(1); state.remove_on_kick = false
    state.request("kick", target); state.drain(); state.drain()
    state.now = state.now + 6; state.tick(); state.drain()
    has(state.response(), '"error":"effect_unconfirmed"'); assert(state.kicks == 1)
end)
test("queued timeout cancels an effect before the game thread resumes", function()
    local state = harness(1); state.query(); local target = state.target(1)
    state.request("kick", target); state.now = state.now + 3; state.tick(); state.tick()
    has(state.response(), '"error":"dispatcher_timeout"')
    state.request(); assert(#state.queue == 1)
    state.drain(); assert(state.kicks == 0)
end)
test("late game-thread callback checks its own queue deadline even before the async timer", function()
    local state = harness(1); state.query(); local target = state.target(1)
    state.request("kick", target); state.now = state.now + 3
    state.drain(); state.tick()
    has(state.response(), '"error":"dispatcher_timeout"'); assert(state.kicks == 0)
end)
test("time spent reading native roster cannot authorize an expired effect", function()
    local state = harness(1); state.query(); local target = state.target(1)
    state.collect_delay = 6
    state.request("kick", target); state.drain(); state.tick()
    has(state.response(), '"error":"request_expired"'); assert(state.kicks == 0)
end)
test("same nonce cannot execute twice after an intervening request", function()
    local state = harness(1); state.query(); local target = state.target(1)
    local id = state.request("kick", target); state.drain(); state.drain()
    state.query(); state.request("kick", target, id); state.drain()
    assert(state.kicks == 1)
end)
test("expired or malformed requests never reach the dispatcher", function()
    local state = harness(1); state.request("query", nil, nil, state.now - 11); assert(#state.queue == 0)
    state.request("query", nil, "../bad"); assert(#state.queue == 0)
    state.request("query", { boot_id = string.rep("a", 32) }); assert(#state.queue == 0)
    state.request("arbitrary"); assert(#state.queue == 0)
end)
test("old boot and tampered account targets are rejected", function()
    for _, field in ipairs({ "boot_id", "account_id", "session_token" }) do
        local state = harness(1); state.query(); local target = state.target(1)
        target[field] = string.rep("f", 32)
        state.request("kick", target); failure(state, "target_changed"); assert(state.kicks == 0)
    end
end)
test("replaced controller rotates the token even if its numeric address is reused", function()
    local state = harness(1); state.query(); local old = state.target(1)
    state.rows[1].Owner.GetFullName = function() return "replaced_controller" end
    state.query(); local current = state.target(1)
    assert(current.session_token ~= old.session_token)
    state.request("kick", old); failure(state, "target_changed"); assert(state.kicks == 0)
end)
test("native bridge absent rejects bans and advertises only verified kick", function()
    local state = harness(1); has(state.query(), '"actions":["kick_player"]')
    state.request("ban", state.target(1)); failure(state, "api_unavailable"); assert(state.native_calls == 0)
end)
test("loaded native bridge with a failed read-only probe cannot advertise bans", function()
    local state = harness(1); state.native_available = true; state.probe_failure = true
    has(state.query(), '"actions":["kick_player"]')
    assert(state.probes == 1 and state.native_calls == 0)
    state.request("ban", state.target(1)); failure(state, "api_unavailable")
    assert(state.native_calls == 0)
end)
test("native ban confirms native flag, persistent flag and disconnected account", function()
    local state = harness(1); state.native_available = true
    has(state.query(), '"actions":["kick_player","ban_player"]')
    state.request("ban", state.target(1)); state.drain(); assert(state.native_calls == 1)
    state.drain(); has(state.response(), '"applied":true')
    assert(state.known[1].bIsBanned and #state.rows == 0)
end)
test("native dispatch receipt alone is insufficient to confirm a ban", function()
    for _, option in ipairs({ "apply_native", "persist_native", "remove_on_ban" }) do
        local state = harness(1); state.native_available = true; state.query(); local target = state.target(1)
        state[option] = false
        state.request("ban", target); state.drain(); state.drain()
        state.now = state.now + 6; state.tick(); state.drain()
        has(state.response(), '"error":"effect_unconfirmed"'); assert(state.native_calls == 1)
    end
end)
test("a newly bound connection for a banned account prevents completed confirmation", function()
    local state = harness(1); state.native_available = true; state.query(); local target = state.target(1)
    state.remove_on_ban = false
    state.request("ban", target); state.drain()
    state.rows[1].Owner.GetFullName = function() return "reconnected_controller" end
    state.drain(); assert(not state.response():find('"applied":true', 1, true))
    state.now = state.now + 6; state.tick(); state.drain()
    has(state.response(), '"error":"effect_unconfirmed"'); assert(state.native_calls == 1)
end)
test("unban checks the current boot and confirms both persisted facts", function()
    local state = harness(1); state.native_available = true; state.known[1].bIsBanned = true; state.sync_ini()
    state.query(); local target = state.target(1); target.session_id, target.session_token = "", ""
    state.request("unban", target); state.drain(); state.drain()
    has(state.response(), '"applied":true'); assert(not state.known[1].bIsBanned and state.native_calls == 1)
end)
test("wrong native receipt leaves outcome unknown and never retries", function()
    local state = harness(1); state.native_available = true; state.query(); local target = state.target(1)
    state.receipt_nonce = string.rep("e", 32)
    state.request("ban", target); failure(state, "effect_unconfirmed")
    state.drain(); assert(state.native_calls == 1)
end)
test("unverified persistence cannot authorize a native mutation", function()
    local state = harness(1); state.native_available = true
    state.files[ini] = state.files[ini]:gsub("False", "True")
    has(state.query(), '"actions":["kick_player"]')
    state.request("ban", state.target(1)); failure(state, "persistence_unavailable")
    assert(state.native_calls == 0)
end)
test("world ownership mismatch cannot advertise native ban", function()
    local state = harness(1); state.native_available = true
    state.matchmaker.GetOuter = function() return invalid end
    has(state.query(), '"actions":["kick_player"]')
end)
test("names containing commas and parentheses do not corrupt account/ban parsing", function()
    local state = harness(1); state.native_available = true
    state.known[1].name = "Name, (nested),bIsBanned=True)"; state.sync_ini()
    has(state.query(), '"actions":["kick_player","ban_player"]')
end)
test("duplicate persisted accounts disable ban capability", function()
    local state = harness(1); state.native_available = true
    local line = state.files[ini]:match("(KnownPlayerList=[^\r\n]+)")
    state.files[ini] = state.files[ini] .. line .. "\r\n"
    has(state.query(), '"actions":["kick_player"]')
end)
test("UTF16LE native INI can confirm persisted facts without parsing player names", function()
    local state = harness(1); state.native_available = true
    local bytes = { "\255\254" }
    for index = 1, #state.files[ini] do bytes[#bytes + 1] = state.files[ini]:sub(index, index) .. "\0" end
    state.files[ini] = table.concat(bytes)
    has(state.query(), '"actions":["kick_player","ban_player"]')
end)
test("malformed strict request order, duplicate keys and oversized requests are ignored", function()
    for _, mutate in ipairs({
        function(value) return value:gsub('"protocol":1', '"protocol":1,"protocol":1') end,
        function(value) return value:gsub('"account_id":""}', '"account_id":"","extra":1}') end,
        function(value) return value:gsub('"requested_at":1000', '"requested_at":01000') end,
        function(value) return value .. "x" end,
        function() return string.rep("x", 1025) end,
    }) do
        local state = harness(0); state.request(); state.drain()
        state.files[base .. "request.json"] = mutate(state.files[base .. "request.json"])
        state.tick(); assert(#state.queue == 0)
    end
end)
test("expired original nonce files never replay after the recent cache TTL", function()
    local state = harness(1); state.query(); local target = state.target(1)
    state.request("kick", target); state.drain(); state.drain()
    local original = state.files[base .. "request.json"]
    state.query(); state.now = state.now + 33
    state.files[base .. "request.json"] = original; state.tick(); state.drain()
    assert(state.kicks == 1 and #state.queue == 0)
end)
test("action accepted late cannot execute beyond its overall deadline", function()
    local state = harness(1); state.query(); local target = state.target(1)
    state.request("kick", target, nil, state.now - 6)
    state.drain(); assert(state.kicks == 0)
end)
test("effects stop before the coarse seconds window can exceed the precise host deadline", function()
    for _, action in ipairs({ "kick", "ban" }) do
        local state = harness(1); state.native_available = true; state.query(); local target = state.target(1)
        state.request(action, target, nil, state.now - 4); state.drain(); state.tick()
        has(state.response(), '"error":"request_expired"')
        assert(state.kicks == 0 and state.native_calls == 0)
    end
end)
test("native request publication failure cannot dispatch the native function", function()
    local state = harness(1); state.native_available = true; state.query(); local target = state.target(1)
    state.native_write_failure = true
    state.request("ban", target); failure(state, "persistence_unavailable")
    assert(state.native_calls == 0)
end)
test("bounded recent nonce storage rejects excess requests without effects", function()
    local state = harness(0)
    for _ = 1, 512 do state.query() end
    state.request(); state.drain(); state.tick()
    has(state.response(), '"error":"request_limit"'); assert(#state.queue == 0)
    assert(state.kicks == 0 and state.native_calls == 0)
end)
assert(#failures == 0, table.concat(failures, "\n"))
print("LgsmPlayerControl tests passed: " .. tests)
return tests
