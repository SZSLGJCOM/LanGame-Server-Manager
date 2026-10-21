-- Bundled after one closed game policy. I/O only polls; all UObject access and
-- the sole native invocation run on EngineTick. A watchdog never retries.
local script = debug.getinfo(1, "S").source:gsub("^@", "")
local root = script:match("^(.+)[/\\]" .. SAVE_CONFIG.project .. "[/\\]Binaries[/\\]Win64[/\\]ue4ss[/\\]Mods[/\\]LgsmWorldSave[/\\]Scripts[/\\]")
if not root then print("[LgsmWorldSave] Private runtime path unavailable; disabled.\n"); return end
local directory = root .. "\\langame_world_save\\"
local pending, outgoing, dispatching, generation = nil, nil, nil, 0
local recent, current = {}, nil
-- Optional, unbound diagnostics. Only the exact Astro policy opts in; no
-- request identity or object data enters a log, and at most eight lines are
-- attempted per probe. Logging cannot replace or change a mandatory receipt.
local probe_stages = { queued=true, callback_entered=true, callback_entered_stale=true,
    world_context_begin=true, world_context_end=true, target_begin=true,
    native_probe_begin=true, native_probe_end=true, reject=true }
local probe_phases = { queued=true, preparing=true, observing=true, pending=true, executing=true }
local function traced_probe(operation)
    return SAVE_CONFIG.diagnose_probe_dispatch == true and SAVE_CONFIG.project == "Astro"
        and SAVE_CONFIG.executable == "AstroServer-Win64-Shipping.exe"
        and operation ~= nil and operation.request.action == "saveprobe"
end
local function trace_probe(operation, stage)
    if not traced_probe(operation) or not probe_stages[stage] then return end
    local count = operation.probe_trace_count or 0
    if count >= 8 then return end
    local previous = operation.probe_trace_stage or "none"
    operation.probe_trace_count, operation.probe_trace_stage = count + 1, stage
    local phase = probe_phases[operation.phase] and operation.phase or "none"
    local at = stage == "reject" and previous or stage
    pcall(print, "[LgsmWorldSave] probe_dispatch stage=" .. stage .. " phase=" .. phase .. " at=" .. at .. "\n")
end
local function token()
    return string.format("%08x%08x%08x%08x", math.random(0, 0x7fffffff), math.random(0, 0x7fffffff),
        math.random(0, 0x7fffffff), math.random(0, 0x7fffffff))
end
local boot = token()
-- Freeze once at module load: later session-file publication cannot grant an
-- old process the identity inherited only by the next managed launch.
local launch_token = SAVE_CONFIG.launch_identity and type(os.getenv) == "function"
    and os.getenv("LGSM_UNREAL_WORLD_SAVE_RUN") or nil
local function check(value, code) if not value then error(code, 0) end end
local function valid(object) return object ~= nil and object:IsValid() == true end
local function same(a, b) return valid(a) and valid(b) and a:GetAddress() == b:GetAddress() end
local function signature(path, flags, params)
    local fn = StaticFindObject(path)
    check(valid(fn) and fn:GetFunctionFlags() == flags, "api_unavailable")
    local count = 0
    fn:ForEachProperty(function(property)
        count = count + 1; local expected = params[count]
        check(expected and property:GetFName():ToString() == expected[1]
            and property:GetClass():GetFName():ToString() == expected[2], "api_unavailable")
    end)
    check(count == #params, "api_unavailable"); return fn
end
local function cdo(path)
    local class = StaticFindObject(path); check(valid(class), "api_unavailable")
    local object = class:GetCDO(); check(valid(object) and object:IsA(path), "api_unavailable"); return object
end
local function unique(class, predicate)
    local values, selected = FindAllOf(class), nil
    if type(values) ~= "table" and SAVE_CONFIG.project == "Astro" and class == "NetDriver" then
        print("[LgsmWorldSave] readiness stage=native_driver_missing candidates=0\n")
    end
    check(type(values) == "table" and #values <= 64, "world_unavailable")
    for _, value in ipairs(values) do
        if valid(value) and predicate(value) then check(selected == nil, "world_ambiguous"); selected = value end
    end
    if not valid(selected) and SAVE_CONFIG.project == "Astro" and class == "NetDriver" then
        print("[LgsmWorldSave] readiness stage=native_driver_missing candidates=" .. #values .. "\n")
    end
    check(valid(selected), "world_unavailable"); return selected
end
local function byte(class, name, offset)
    check(type(RegisterCustomProperty) == "function" and type(PropertyTypes) == "table", "api_unavailable")
    RegisterCustomProperty({ Name = "LgsmWorldSave" .. name, Type = PropertyTypes.ByteProperty,
        BelongsToClass = class, OffsetInternal = offset })
end
local inspect = build_save_target({ check = check, valid = valid, same = same, signature = signature,
    cdo = cdo, unique = unique, byte = byte })
local function fingerprint()
    -- Host also pins and SHA-256 checks the complete image and fixed loader.
    -- These file bytes prevent old queued requests reading new-build offsets.
    local file = io.open(root .. "\\" .. SAVE_CONFIG.project .. "\\Binaries\\Win64\\" .. SAVE_CONFIG.executable, "rb")
    check(file ~= nil, "api_unavailable")
    local ok, result = pcall(function()
        for _, site in ipairs(SAVE_CONFIG.fingerprint) do
            local expected = site[2]:gsub("..", function(hex) return string.char(tonumber(hex, 16)) end)
            check(file:seek("set", site[1]) == site[1] and file:read(#expected) == expected, "api_unavailable")
        end
    end)
    file:close(); if not ok then error(result, 0) end
end
local function world_context(operation)
    trace_probe(operation, "world_context_begin")
    if SAVE_CONFIG.launch_identity then
        check(type(launch_token) == "string" and #launch_token == 41
            and launch_token:match("^LGSM_UWS_[0-9a-f]+$"), "api_unavailable")
    end
    fingerprint()
    check(type(IsInGameThread) == "function" and IsInGameThread(), "dispatcher_unavailable")
    local world
    if SAVE_CONFIG.world_from_netdriver == true then
        local driver = unique("NetDriver", function(value)
            local candidate = value.World
            return value.NetDriverName:ToString() == "GameNetDriver" and valid(candidate)
                and same(candidate.NetDriver, value) and not valid(value.ServerConnection)
        end)
        world = driver.World
    else
        world = unique("World", function(value)
            local driver = value.NetDriver
            return valid(driver) and driver.NetDriverName:ToString() == "GameNetDriver"
                and same(driver.World, value) and not valid(driver.ServerConnection)
        end)
    end
    local gs, gm, instance = world.GameState, world.AuthorityGameMode, world.OwningGameInstance
    -- These dedicated servers do not replicate this GameState bit. Their fixed
    -- native policies must prove authoritative loaded-world readiness instead.
    local native_readiness = SAVE_CONFIG.native_world_readiness == true
        and ((SAVE_CONFIG.project == "SCUM" and SAVE_CONFIG.executable == "SCUMServer.exe")
            or (SAVE_CONFIG.project == "Pal" and SAVE_CONFIG.executable == "PalServer-Win64-Shipping-Cmd.exe")
            or (SAVE_CONFIG.project == "HumanitZServer" and SAVE_CONFIG.executable == "HumanitZServer-Win64-Shipping.exe")
            or (SAVE_CONFIG.project == "Astro" and SAVE_CONFIG.executable == "AstroServer-Win64-Shipping.exe"))
    if SAVE_CONFIG.project == "Astro" and not (valid(gs) and valid(gm) and valid(instance)
        and (native_readiness or gs.bReplicatedHasBegunPlay == true)) then
        print("[LgsmWorldSave] readiness stage=world_state gs=" .. tostring(valid(gs))
            .. " gm=" .. tostring(valid(gm)) .. " instance=" .. tostring(valid(instance))
            .. " begun=" .. tostring(valid(gs) and gs.bReplicatedHasBegunPlay == true) .. "\n")
    end
    check(valid(gs) and valid(gm) and valid(instance)
        and (native_readiness or gs.bReplicatedHasBegunPlay == true), "world_unavailable")
    local observe = traced_probe(operation) and function(stage) trace_probe(operation, stage) end or nil
    local context = inspect(world, instance, observe)
    if not current or not same(current.world, world) or not same(current.instance, instance)
        or not same(current.persistence, context.persistence) or current.save_name ~= context.save_name
        or (current.state and not same(current.state, context.state)) then
        context.token = token(); current = context
    else context.token = current.token; current = context end
    trace_probe(operation, "world_context_end")
    return context
end
local function hex(value) return #value == 32 and value:match("^[0-9a-f]+$") end
local function request()
    local file = io.open(directory .. "request.json", "rb"); if not file then return nil end
    local body = file:read(1025); file:close(); if not body or #body > 1024 then return nil end
    local id, time, action, run, world = body:match('^%s*{%s*"protocol"%s*:%s*1%s*,%s*"request_id"%s*:%s*"([0-9a-f]+)"%s*,%s*"requested_at"%s*:%s*(%d+)%s*,%s*"action"%s*:%s*"([a-z]+)"%s*,%s*"boot_id"%s*:%s*"([0-9a-f]*)"%s*,%s*"world_token"%s*:%s*"([0-9a-f]*)"%s*}%s*$')
    if not id or not hex(id) or #time > 12 or (#time > 1 and time:sub(1,1) == "0") then return nil end
    time = tonumber(time); local age = os.time() - time
    if age < -2 or age > 10 then return nil end
    if action == "saveprobe" then if run ~= "" or world ~= "" then return nil end
    elseif action == "save" then if not hex(run) or not hex(world) then return nil end
    else return nil end
    return { request_id = id, requested_at = time, action = action, boot_id = run, world_token = world }
end
local known = { api_unavailable=true, world_unavailable=true, world_ambiguous=true, save_busy=true,
    target_changed=true, request_expired=true, dispatcher_timeout=true, dispatcher_unavailable=true }
local function fail(operation, code)
    trace_probe(operation, "reject")
    local invoked = operation.invoked == true
    outgoing = { request = operation.request, ready = invoked, invoked = invoked,
        error = invoked and "save_call_unknown" or (known[code] and code or "api_unavailable") }
end
local function fresh(request, seconds) local age = os.time() - request.requested_at; return age >= -2 and age <= seconds end
-- Queue admission and owned native execution are different lifecycles. The
-- short queue gate must not expire an already-entered snapshot or readback.
local function in_queue(operation)
    return operation.phase == "queued" or operation.phase == "preparing"
end
local function operation_fresh(operation)
    return fresh(operation.request, (operation.entered or operation.completion) and 30 or 3)
end
local function schedule()
    local operation, mark = pending, generation
    dispatching, operation.queued_at, operation.phase = operation, os.time(), "queued"
    trace_probe(operation, "queued")
    local ok = pcall(function()
        ExecuteInGameThread(function()
            trace_probe(operation, (generation ~= mark or pending ~= operation) and "callback_entered_stale" or "callback_entered")
            if generation ~= mark or pending ~= operation then
                if dispatching == operation then dispatching = nil end
                return
            end
            local success, result = pcall(function()
                local function validate()
                    check(generation == mark and pending == operation and operation_fresh(operation), "request_expired")
                    local elapsed = os.time() - operation.queued_at
                    check(elapsed >= 0 and (not in_queue(operation) or elapsed < 3), "dispatcher_timeout")
                end
                -- Every EngineTick dispatch, including a completion poll, has
                -- the same queue admission limit. Only its running work may
                -- consume the remainder of the original completion budget.
                validate()
                operation.phase = operation.completion and "observing" or "preparing"
                if operation.completion then
                    local context = operation.context
                    check(type(context.current) == "function" and context.current(), "target_changed")
                    local complete, confirmed, native_error = operation.completion(validate)
                    validate(); check(context.current(), "target_changed")
                    if not complete then operation.phase = "pending"; return nil end
                    return { request = operation.request, ready = true, invoked = true, call_returned = true,
                        world_token = context.token, write_confirmed = confirmed == true,
                        error = native_error == "save_failed" and "save_failed" or nil,
                        native_process_id = context.native_process_id, native_creation_time = context.native_creation_time }
                end
                validate(); local context = world_context(operation); validate()
                if operation.request.action == "saveprobe" then return { request = operation.request, ready = true, world_token = context.token,
                    native_process_id = context.native_process_id, native_creation_time = context.native_creation_time } end
                check(operation.request.boot_id == boot and operation.request.world_token == context.token, "target_changed")
                -- Mark before the final gate. A watchdog before the mark blocks
                -- the gate; after it, any receipt stays conservatively unknown.
                operation.invoked = true; validate()
                local function validate_target()
                    validate()
                    local latest = world_context()
                    check(latest.token == context.token, "target_changed")
                    validate()
                end
                local function enter_native()
                    -- The policy calls this once, after publishing its native
                    -- frame and immediately before the first native entry.
                    -- Admission still uses the short gate. The native bridge
                    -- independently checks that original frame before effect.
                    check(not operation.entered, "request_expired")
                    validate_target()
                    operation.entered, operation.phase = true, "executing"
                end
                local confirmed, native_error = context.invoke(validate_target, enter_native)
                if type(confirmed) == "function" then
                    operation.completion, operation.context, operation.phase = confirmed, context, "pending"
                    return nil
                end
                validate()
                return { request = operation.request, ready = true, invoked = true,
                    call_returned = true, world_token = context.token, write_confirmed = confirmed == true,
                    error = native_error == "save_failed" and "save_failed" or nil,
                    native_process_id = context.native_process_id, native_creation_time = context.native_creation_time }
            end)
            if dispatching == operation then dispatching = nil end
            if generation ~= mark or pending ~= operation then return end
            if success and result == nil and operation.completion then return end
            if success then outgoing = result else fail(operation, result) end
            pending = nil
        end, EGameThreadMethod.EngineTick)
    end)
    if not ok then
        if dispatching == operation then dispatching = nil end
        if generation == mark and pending == operation then
            pending = nil; generation = generation + 1; fail(operation, "dispatcher_unavailable")
        end
    end
end
local function encode(response)
    local req = response.request
    local function quoted(value) return '"' .. value .. '"' end -- closed ASCII identities/error codes only
    local identity = ""
    if SAVE_CONFIG.launch_identity then identity = ',"launch_token":' .. quoted(launch_token or "") end
    if response.native_process_id ~= nil or response.native_creation_time ~= nil then
        local pid, created = response.native_process_id, response.native_creation_time
        check(type(pid) == "number" and pid % 1 == 0 and pid > 0 and pid <= 0xffffffff
            and type(created) == "string" and #created <= 20 and created:match("^[1-9][0-9]*$"), "api_unavailable")
        identity = identity .. ',"native_process_id":' .. string.format("%.0f", pid) .. ',"native_creation_time":' .. quoted(created)
    end
    return '{"protocol":1,"request_id":' .. quoted(req.request_id) .. ',"timestamp":' .. os.time()
        .. ',"action":' .. quoted(req.action) .. ',"boot_id":' .. quoted(boot)
        .. ',"world_token":' .. quoted(response.world_token or req.world_token)
        .. ',"ready":' .. tostring(response.ready == true) .. ',"invoked":' .. tostring(response.invoked == true)
        .. ',"call_returned":' .. tostring(response.call_returned == true)
        .. ',"write_confirmed":' .. tostring(response.write_confirmed == true) .. identity .. ',"error":'
        .. (response.error and quoted(response.error) or 'null') .. '}\n'
end
local function flush()
    if not outgoing then return end
    local response = outgoing; local temporary = directory .. "response.json.tmp"
    local file = io.open(temporary, "wb"); if not file then return end
    local written, closed = file:write(encode(response)), file:close()
    if not written or not closed then return end
    local removed, _, code = os.remove(directory .. "response.json")
    if not removed and code ~= 2 then return end
    if os.rename(temporary, directory .. "response.json") and outgoing == response then outgoing = nil end
end
LoopAsync(250, function()
    flush(); local time = os.time()
    if pending and (not fresh(pending.request, (pending.entered or pending.completion) and 30 or 5)
        or (dispatching == pending and in_queue(pending) and time - pending.queued_at >= 3)) then
        local expired = pending
        fail(expired, "dispatcher_timeout"); pending = nil; generation = generation + 1
        if dispatching == expired then dispatching = nil end
    end
    if pending and not dispatching then schedule() end
    local req = request(); if not req then return false end
    local count = 0
    for id, seen in pairs(recent) do if time - seen > 32 then recent[id] = nil else count = count + 1 end end
    if recent[req.request_id] or count >= 512 then return false end
    recent[req.request_id] = time
    if pending or dispatching then fail({request=req}, "dispatcher_unavailable"); return false end
    if type(ExecuteInGameThread) ~= "function" or type(EGameThreadMethod) ~= "table" or EGameThreadMethod.EngineTick == nil then
        fail({request=req}, "dispatcher_unavailable"); return false
    end
    generation = generation + 1; pending = { request = req }; schedule(); return false
end)
