-- Pure Lua behavioral tests. Pass the production main.lua path as arg[1].
-- May run in Lua 5.4+ or in an isolated UE4SS test mod; no game APIs are used.
local production_path = assert(arg[1], "main.lua path required")
local tests, failures = 0, {}
local nonce = string.rep("a", 32)
local function object(address, class)
    return { IsValid = function() return true end, GetAddress = function() return address end,
        IsA = function(_, name) return name == class end }
end
local invalid = { IsValid = function() return false end }
local function array(rows)
    return { GetArrayNum = function() return #rows end,
        ForEach = function(_, callback)
            for index, value in ipairs(rows) do callback(index, { get = function() return value end }) end
        end }
end
local function text(value) return { ToString = function() return value end } end

local function harness(player_count)
    local state = { now = 1788830000, files = {}, writes = 0, queue = {}, rows = {}, connections = {} }
    local driver = object(1, "/Script/R5SocketSubsystem.R5NetDriver")
    local world = object(2, "/Script/Engine.World")
    local gs = object(3, "/Script/R5.R5GameState")
    local gm = object(4, "/Script/R5.R5GameMode")
    driver.World, driver.ServerConnection, driver.NetDriverName = world, invalid, text("GameNetDriver")
    world.NetDriver, world.GameState, world.AuthorityGameMode = driver, gs, gm
    gs.bReplicatedHasBegunPlay = true
    for index = 1, player_count do
        local connection = object(100 + index, "/Script/Engine.NetConnection")
        local pc = object(200 + index, "/Script/R5.R5PlayerController")
        local ps = object(300 + index, "/Script/R5.R5PlayerState")
        connection.Driver, connection.PlayerController, connection.OwningActor = driver, pc, pc
        connection.Children = array({})
        pc.Player, pc.NetConnection, pc.PlayerState = connection, connection, ps
        ps.Owner, ps.PlayerId, ps.PlayerNamePrivate = pc, index, text('玩家 "' .. index .. '" \\ name')
        ps.bIsInactive, ps.bFromPreviousLevel, ps.bIsABot = false, false, false
        state.rows[index], state.connections[index] = ps, connection
    end
    gs.PlayerArray, driver.ClientConnections = array(state.rows), array(state.connections)
    state.driver, state.world, state.gs = driver, world, gs
    local environment = setmetatable({}, { __index = _G })
    environment.os = { time = function() return state.now end,
        remove = function(path)
            if state.files[path] then state.files[path] = nil; return true end
            return nil, "missing", 2
        end,
        rename = function(from, to) state.files[to] = state.files[from]; state.files[from] = nil; return true end }
    environment.io = { open = function(path, mode)
        if mode == "rb" then
            local content = state.files[path]
            if not content then return nil end
            return { read = function(_, limit) return content:sub(1, limit) end, close = function() return true end }
        end
        return { write = function(_, content) state.files[path] = content; state.writes = state.writes + 1; return true end,
            close = function() return true end }
    end }
    environment.print = function() end
    environment.debug = { getinfo = function() return { source = "@D:/probe/R5/Binaries/Win64/ue4ss/Mods/LgsmPlayerQuery/Scripts/main.lua" } end }
    environment.LoopAsync = function(delay, callback) assert(delay == 250); state.tick = callback end
    environment.ExecuteInGameThread = function(callback) state.queue[#state.queue + 1] = callback end
    environment.EGameThreadMethod = { EngineTick = 0 }
    environment.IsInGameThread = function() return true end
    environment.FindAllOf = function(name) assert(name == "R5NetDriver"); return { driver } end
    assert(loadfile(production_path, "t", environment))()
    function state.request(id, timestamp)
        state.files["D:/probe\\langame_player_query\\request.json"] = '{"request_id":"' .. (id or nonce)
            .. '","requested_at":' .. (timestamp or state.now) .. '}'
        state.tick()
    end
    function state.drain()
        local queue = state.queue; state.queue = {}
        for _, callback in ipairs(queue) do callback() end
        state.tick()
    end
    function state.response() return state.files["D:/probe\\langame_player_query\\response.json"] or "" end
    return state
end

local function test(name, callback)
    tests = tests + 1
    local ok, err = pcall(callback)
    if not ok then failures[#failures + 1] = name .. ": " .. tostring(err) end
end
local function success(state, count)
    state.request(); assert(state.response() == "", "response must await game thread")
    state.drain()
    assert(state.response():find('"complete":true', 1, true))
    assert(state.response():find('"current_players":' .. count, 1, true))
    assert(not state.response():find('max_players', 1, true))
end
local function failure(state, code)
    state.request(); state.drain()
    assert(state.response():find('"complete":false', 1, true), state.response())
    assert(state.response():find('"error":"' .. code .. '"', 1, true), state.response())
    assert(state.response():find('"players":[]', 1, true))
end
test("empty current world", function() success(harness(0), 0) end)
test("two directly connected identities", function()
    local state = harness(2); success(state, 2)
    assert(state.response():find('玩家 \\"1\\" \\\\ name', 1, true))
end)
test("inactive identity rejects entire snapshot", function()
    local state = harness(2); state.rows[2].bIsInactive = true; failure(state, "roster_incomplete")
end)
test("previous world identity rejected", function()
    local state = harness(1); state.rows[1].bFromPreviousLevel = true; failure(state, "roster_incomplete")
end)
test("pending login rejects complete roster", function()
    local state = harness(1); state.connections[1].PlayerController = invalid; failure(state, "roster_incomplete")
end)
test("detached controller rejected", function()
    local state = harness(1); state.connections[1].PlayerController.NetConnection = invalid; failure(state, "roster_incomplete")
end)
test("duplicate session identity rejected", function()
    local state = harness(2); state.rows[2].PlayerId = state.rows[1].PlayerId; failure(state, "roster_incomplete")
end)
test("blank name is not replaced", function()
    local state = harness(1); state.rows[1].PlayerNamePrivate = text(""); failure(state, "identity_unavailable")
end)
test("unmatched game-state player rejected", function()
    local state = harness(2); table.remove(state.connections); failure(state, "roster_incomplete")
end)
test("world identity mismatch rejected", function()
    local state = harness(0); state.world.NetDriver = invalid; failure(state, "world_unavailable")
end)
test("unloaded world rejected", function()
    local state = harness(0); state.gs.bReplicatedHasBegunPlay = false; failure(state, "world_unavailable")
end)
test("stale and malformed requests ignored", function()
    local state = harness(0); state.request(nonce, state.now - 11); assert(#state.queue == 0)
    state.request("../malicious"); assert(#state.queue == 0)
end)
test("missing game thread cannot grow queue or emit stale success", function()
    local state = harness(0); state.request(); assert(#state.queue == 1)
    state.now = state.now + 3; state.tick(); state.tick()
    assert(state.response():find('"error":"dispatcher_timeout"', 1, true))
    state.request(string.rep("b", 32)); state.tick(); assert(#state.queue == 1)
    assert(state.response():find('"error":"dispatcher_unavailable"', 1, true))
    state.drain(); assert(not state.response():find('"complete":true', 1, true))
    state.request(string.rep("c", 32)); state.drain()
    assert(state.response():find('"complete":true', 1, true))
end)
test("request consumed only once", function()
    local state = harness(0); success(state, 0)
    state.request(); assert(#state.queue == 0)
end)
assert(#failures == 0, table.concat(failures, "\n"))
print("LgsmPlayerQuery tests passed: " .. tests)
return tests
