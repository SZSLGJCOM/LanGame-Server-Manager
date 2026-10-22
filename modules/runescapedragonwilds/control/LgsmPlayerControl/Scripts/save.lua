-- SaveGame is a native reflected request with no completion receipt. Invoke it
-- once on EngineTick; a returned call never claims that disk writing completed.
local function build_world_save(api)
    local current
    local run = os.getenv("LGSM_DRAGON_SAVE_RUN") or ""
    local function is_request(request)
        return request.action == "saveprobe" or request.action == "save"
    end
    local function request(content)
        local id, timestamp, action, boot, token = content:match(
            '^%s*{%s*"protocol"%s*:%s*1%s*,%s*"request_id"%s*:%s*"([0-9a-f]+)"%s*,%s*"requested_at"%s*:%s*(%d+)%s*,%s*"action"%s*:%s*"([a-z]+)"%s*,%s*"boot_id"%s*:%s*"([0-9a-f]*)"%s*,%s*"world_token"%s*:%s*"([0-9a-f]*)"%s*}%s*$')
        local function hex(value) return value and #value == 32 end
        if not id or not hex(id) or #timestamp > 12
            or (#timestamp > 1 and timestamp:sub(1, 1) == "0") then return nil end
        local age = os.time() - tonumber(timestamp)
        if age < -2 or age > 10 then return nil end
        if action == "saveprobe" then
            if boot ~= "" or token ~= "" then return nil end
        elseif action == "save" then
            if not hex(boot) or not hex(token) then return nil end
        else return nil end
        return { request_id = id, requested_at = tonumber(timestamp), action = action,
            boot_id = boot, world_token = token }
    end
    local function inspect()
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
        require_condition(not busy, "save_busy")
        if not current or not same(current.world, world) or not same(current.persistence, persistence)
            or not same(current.spud, spud) or not same(current.instance, instance) then
            current = { world = world, persistence = persistence, spud = spud, instance = instance,
                token = api.random_token() }
        end
        return current, save
    end
    local function execute(operation, validate_current, mark_invoked)
        api.require_condition(#run == 32 and run:match("^[0-9a-f]+$"), "api_unavailable")
        local context, save = inspect()
        validate_current()
        if operation.action == "saveprobe" then
            return { request = operation, ready = true, world_token = context.token }
        end
        api.require_condition(operation.boot_id == api.boot_id and operation.world_token == context.token, "target_changed")
        validate_current(true)
        mark_invoked()
        save(context.persistence, false)
        validate_current()
        return { request = operation, ready = true, world_token = context.token,
            invoked = true, call_returned = true }
    end
    local function encode(response, quoted)
        local operation = response.request
        return '{"protocol":1,"request_id":' .. quoted(operation.request_id) .. ',"timestamp":' .. os.time()
            .. ',"action":' .. quoted(operation.action) .. ',"boot_id":' .. quoted(api.boot_id)
            .. ',"world_token":' .. quoted(response.world_token or operation.world_token or "")
            .. ',"ready":' .. tostring(response.ready == true) .. ',"invoked":' .. tostring(response.invoked == true)
            .. ',"call_returned":' .. tostring(response.call_returned == true)
            .. ',"write_confirmed":false,"run":' .. quoted(run)
            .. ',"error":' .. (response.error and quoted(response.error) or 'null') .. '}\n'
    end
    return { request = request, is_request = is_request, execute = execute, encode = encode }
end
