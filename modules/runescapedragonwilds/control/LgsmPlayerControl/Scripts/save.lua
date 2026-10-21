-- Prepended to the owned main.lua payload. Native Save dispatches once; Query
-- only reads that nonce. Completion requires the exact root, every owned
-- terminal writer, publication readback and final callback return.
local function build_world_save(api)
    local current, native_function
    local run = os.getenv("LGSM_DRAGON_SAVE_RUN") or ""
    local function is_request(request)
        return request.action == "saveprobe" or request.action == "save" or request.action == "savequery"
    end
    local function request(content)
        local id, timestamp, action, boot, token = content:match(
            '^%s*{%s*"protocol"%s*:%s*1%s*,%s*"request_id"%s*:%s*"([0-9a-f]+)"%s*,%s*"requested_at"%s*:%s*(%d+)%s*,%s*"action"%s*:%s*"([a-z]+)"%s*,%s*"boot_id"%s*:%s*"([0-9a-f]*)"%s*,%s*"world_token"%s*:%s*"([0-9a-f]*)"%s*}%s*$')
        local save_id
        if not id then
            id, timestamp, action, boot, token, save_id = content:match(
                '^%s*{%s*"protocol"%s*:%s*1%s*,%s*"request_id"%s*:%s*"([0-9a-f]+)"%s*,%s*"requested_at"%s*:%s*(%d+)%s*,%s*"action"%s*:%s*"(savequery)"%s*,%s*"boot_id"%s*:%s*"([0-9a-f]+)"%s*,%s*"world_token"%s*:%s*"([0-9a-f]+)"%s*,%s*"save_id"%s*:%s*"([0-9a-f]+)"%s*}%s*$')
        end
        local function hex(value) return value and #value == 32 end
        if not id or not hex(id) or #timestamp > 12
            or (#timestamp > 1 and timestamp:sub(1, 1) == "0") then return nil end
        local age = os.time() - tonumber(timestamp)
        if age < -2 or age > 10 then return nil end
        if action == "saveprobe" then
            if boot ~= "" or token ~= "" then return nil end
        elseif action == "save" or action == "savequery" then
            if not hex(boot) or not hex(token) or (action == "savequery" and not hex(save_id)) then return nil end
        else return nil end
        return { request_id = id, requested_at = tonumber(timestamp), action = action,
            boot_id = boot, world_token = token, save_id = save_id }
    end
    local function inspect(query)
        local require_condition, valid, same = api.require_condition, api.valid, api.same
        require_condition(IsInGameThread(), "dispatcher_unavailable")
        local drivers = FindAllOf("RedpointEOSNetDriver")
        require_condition(type(drivers) == "table" and #drivers <= 16, "world_unavailable")
        local driver
        for _, value in ipairs(drivers) do
            if valid(value) and value.NetDriverName:ToString() == "GameNetDriver" then
                require_condition(driver == nil, "world_ambiguous"); driver = value
            end
        end
        require_condition(valid(driver), "world_unavailable")
        local world = driver.World
        require_condition(valid(world) and world:GetFullName() == "World /Game/Maps/World/L_World.L_World"
            and same(world.NetDriver, driver) and not valid(driver.ServerConnection), "world_unavailable")
        local gs, gm, instance = world.GameState, world.AuthorityGameMode, world.OwningGameInstance
        require_condition(valid(instance) and valid(gs) and gs:IsA("/Script/Dominion.DominionGameStateBase")
            and valid(gm) and gm:IsA("/Script/Dominion.DominionGameMode")
            and gs.bReplicatedHasBegunPlay == true, "world_unavailable")
        local function subsystem(name, class)
            local candidates, selected = FindAllOf(name), nil
            require_condition(type(candidates) == "table" and #candidates <= 4, "api_unavailable")
            for _, value in ipairs(candidates) do
                if valid(value) and value:GetClass():GetFullName() == "Class " .. class
                    and same(value:GetOuter(), instance) then
                    require_condition(selected == nil, "api_unavailable"); selected = value
                end
            end
            require_condition(valid(selected), "api_unavailable")
            return selected
        end
        local persistence = subsystem("PersistenceSubsystem", "/Script/Dominion.PersistenceSubsystem")
        local spud = subsystem("SpudSubsystem", "/Script/SPUD.SpudSubsystem")
        local save = api.signature("/Script/Dominion.PersistenceSubsystem:SaveGame", 0x4020401,
            {{ "bAdditionalLogging", "BoolProperty" }})
        local saving = api.signature("/Script/SPUD.SpudSubsystem:IsSavingGame", 0x54020401,
            {{ "ReturnValue", "BoolProperty" }})
        local busy = saving(spud)
        require_condition(type(busy) == "boolean", "api_unavailable")
        require_condition(query or not busy, "save_busy")
        if not current or not same(current.world, world) or not same(current.persistence, persistence)
            or not same(current.spud, spud) or not same(current.instance, instance) then
            current = { world = world, persistence = persistence, spud = spud, instance = instance,
                token = api.random_token() }
        end
        return current, save
    end
    local issues = { none = true, save_busy = true, native_profile_unavailable = true,
        native_identity_unavailable = true, native_target_changed = true, native_target_unavailable = true,
        native_provider_unsupported = true, native_slot_unavailable = true, native_write_failed = true,
        native_completion_timeout = true, native_closure_unavailable = true, native_run_changed = true,
        native_nonce_conflict = true, native_request_unknown = true, native_request_capacity = true }
    local statuses = { ready = true, dispatching = true, completed = true, failed = true,
        rejected = true, unknown = true }
    local function exchange(context, operation, nonce, validate_current)
        api.require_condition(#run == 32 and run:match("^[0-9a-f]+$"), "api_unavailable")
        if not native_function then
            local dll = api.root .. "\\RSDragonwilds\\Binaries\\Win64\\ue4ss\\Mods\\LgsmPlayerControl\\native\\LgsmPlayerControl.dll"
            native_function = package.loadlib(dll, "luaopen_lgsm_dragonwilds_save")
        end
        api.require_condition(type(native_function) == "function", "api_unavailable")
        local path = api.directory .. "native-save-response.txt"
        local removed, _, code = os.remove(path)
        api.require_condition(removed or code == 2, "effect_unconfirmed")
        local file = io.open(api.directory .. "native-save-request.txt", "wb")
        api.require_condition(file ~= nil, "effect_unconfirmed")
        local body = "schema=1\nrun=" .. run .. "\nnonce=" .. nonce .. "\nop=" .. operation
            .. "\npersistence=" .. context.persistence:GetAddress() .. "\nspud=" .. context.spud:GetAddress()
            .. "\ninstance=" .. context.instance:GetAddress() .. "\nworld=" .. context.world:GetAddress() .. "\n"
        local written, closed = file:write(body), file:close()
        api.require_condition(written and closed, "effect_unconfirmed")
        validate_current(operation == "save")
        native_function()
        file = io.open(path, "rb"); api.require_condition(file ~= nil, "effect_unconfirmed")
        local response = file:read(1025); file:close()
        api.require_condition(response and #response <= 1024, "effect_unconfirmed")
        local response_run, response_nonce, response_op, pid, creation, status, invoked, returned, issue = response:match(
            "^schema=1\nrun=([0-9a-f]+)\nnonce=([0-9a-f]+)\nop=([a-z]+)\npid=(%d+)\ncreation_time=(%d+)\nstatus=([a-z]+)\ninvoked=([a-z]+)\nreturned=([a-z]+)\nissue=([a-z_]+)\n$")
        api.require_condition(response_run == run and response_nonce == nonce and response_op == operation
            and pid and #pid <= 10 and pid:sub(1, 1) ~= "0" and creation and #creation <= 20
            and creation:sub(1, 1) ~= "0" and statuses[status] and issues[issue]
            and (invoked == "true" or invoked == "false") and (returned == "true" or returned == "false")
            and (status ~= "completed" or (operation ~= "probe" and invoked == "true" and returned == "true" and issue == "none"))
            and (status ~= "ready" or (operation == "probe" and invoked == "false" and returned == "false" and issue == "none"))
            and (status ~= "rejected" or (invoked == "false" and returned == "false" and issue ~= "none"))
            and (status ~= "dispatching" or (operation ~= "probe" and invoked == "true" and issue == "none"))
            and (status ~= "failed" or (invoked == "true" and issue == "native_write_failed"))
            and (status ~= "unknown" or issue ~= "none"), "effect_unconfirmed")
        return { status = status, invoked = invoked == "true", returned = returned == "true",
            issue = issue, pid = pid, creation = creation }
    end
    local function execute(operation, validate_current, native, mark_invoked)
        api.require_condition(native ~= nil, "api_unavailable")
        local context = inspect(operation.action == "savequery")
        validate_current()
        if operation.action ~= "saveprobe" then
            api.require_condition(operation.boot_id == api.boot_id and operation.world_token == context.token, "target_changed")
        end
        local native_op = operation.action == "saveprobe" and "probe" or operation.action == "savequery" and "query" or "save"
        if native_op == "save" then validate_current(true); mark_invoked() end
        local result = exchange(context, native_op, operation.save_id or operation.request_id, validate_current)
        return { request = operation, ready = result.status ~= "rejected", world_token = context.token,
            invoked = result.invoked, call_returned = result.returned, write_confirmed = result.status == "completed",
            native_status = result.status, native_pid = result.pid, native_creation_time = result.creation,
            run = run, error = result.issue ~= "none" and result.issue or nil }
    end
    local function encode(response, quoted)
        local operation = response.request
        return '{"protocol":1,"request_id":' .. quoted(operation.request_id) .. ',"timestamp":' .. os.time()
            .. ',"action":' .. quoted(operation.action) .. ',"boot_id":' .. quoted(api.boot_id)
            .. ',"save_id":' .. quoted(operation.save_id or "")
            .. ',"world_token":' .. quoted(response.world_token or operation.world_token or "")
            .. ',"ready":' .. tostring(response.ready == true) .. ',"invoked":' .. tostring(response.invoked == true)
            .. ',"call_returned":' .. tostring(response.call_returned == true)
            .. ',"write_confirmed":' .. tostring(response.write_confirmed == true)
            .. ',"native_status":' .. quoted(response.native_status or "unknown")
            .. ',"native_pid":' .. quoted(response.native_pid or "")
            .. ',"native_creation_time":' .. quoted(response.native_creation_time or "")
            .. ',"run":' .. quoted(response.run or run)
            .. ',"error":' .. (response.error and quoted(response.error) or 'null') .. '}\n'
    end
    return { request = request, is_request = is_request, execute = execute, encode = encode }
end
