-- Conan Exiles Enhanced verified profiles. Native world-persistence flush only.
-- The separate native bridge pins complete image/loader hashes and its ABI.
local SAVE_CONFIG = { project = "ConanSandbox", executable = "ConanSandboxServer-Win64-Shipping.exe", launch_identity = true, world_from_netdriver = true, fingerprint = {} }
local function build_save_target(api)
    local source = debug.getinfo(1, "S").source:gsub("^@", "")
    local root = source:match("^(.+)[/\\]ConanSandbox[/\\]Binaries[/\\]Win64[/\\]ue4ss[/\\]Mods[/\\]LgsmWorldSave[/\\]Scripts[/\\]")
    api.check(root ~= nil, "api_unavailable")
    local directory = root .. "\\langame_world_save\\"
    local native
    local counter = 0
    local checked_target, native_pid, native_creation
    local function next_nonce()
        counter = counter + 1
        return string.format("%08x%08x%08x%08x", math.random(0, 0x7fffffff), math.random(0, 0x7fffffff),
            math.random(0, 0x7fffffff), counter % 0x7fffffff)
    end
    local function bridge(operation, proxy, instance, world, validate, frame, enter_native)
        if not native then
            local loaded = package.loadlib(root .. "\\ConanSandbox\\Binaries\\Win64\\ue4ss\\Mods\\LgsmWorldSave\\native\\LgsmConanSaveControl.dll", "luaopen_lgsm_conan_save")
            api.check(type(loaded) == "function", "api_unavailable"); native = loaded
        end
        if not frame then
            local id = next_nonce()
            frame = { id = id, body = "schema=1\nnonce=" .. id .. "\nop=" .. operation .. "\nrequested_at=" .. os.time()
                .. "\nproxy_address=" .. string.format("%d", proxy:GetAddress())
                .. "\ninstance_address=" .. string.format("%d", instance:GetAddress()) .. "\nworld_address=" .. string.format("%d", world:GetAddress()) .. "\n" }
        end
        -- Poll the identical original save frame. Fresh timestamps/nonces would
        -- admit another save instead of observing its owned native callback.
        local id, body = frame.id, frame.body
        -- Publish by rename; removing an old leaf never writes through it.
        local temporary = directory .. "native-request." .. id .. ".tmp"
        local file = io.open(temporary, "wb"); api.check(file ~= nil, "api_unavailable")
        local written, closed = file:write(body), file:close()
        api.check(written and closed, "api_unavailable")
        local removed, _, code = os.remove(directory .. "native-request.txt")
        api.check(removed or code == 2, "api_unavailable")
        api.check(os.rename(temporary, directory .. "native-request.txt"), "api_unavailable")
        validate() -- No native entry may follow a cancelled or stale closure.
        if enter_native then enter_native() end
        native()
        local response = io.open(directory .. "native-response.txt", "rb")
        api.check(response ~= nil, "save_call_unknown")
        local text = response:read(513); response:close()
        api.check(text and #text <= 512, "save_call_unknown")
        local received, op, status, pid, creation = text:match("^schema=1\nnonce=([0-9a-f]+)\nop=([a-z]+)\nstatus=([a-z]+)\npid=(%d+)\ncreation_time=(%d+)\n$")
        api.check(received == id and op == operation and type(pid) == "string" and #pid <= 10 and tonumber(pid) > 0
            and type(creation) == "string" and #creation <= 20 and creation:sub(1, 1) ~= "0", "save_call_unknown")
        return status, tonumber(pid), creation, frame
    end
    local function target(world, instance)
        api.check(api.valid(world.GameState) and api.valid(world.AuthorityGameMode)
            and api.same(world.OwningGameInstance, instance), "world_unavailable")
        return api.unique("WorldPersistenceComponent", function(value)
            return value:GetClass():GetFullName() == "Class /Script/UE4Dreamworld.WorldPersistenceComponent"
                and api.same(value:GetOuter(), world.AuthorityGameMode)
        end)
    end
    return function(world, instance)
        local proxy = target(world, instance)
        local game_mode = world.AuthorityGameMode
        if not api.same(checked_target, proxy) then
            local status, pid, creation = bridge("probe", proxy, world.AuthorityGameMode, world, function() end)
            api.check(status == "ready", "api_unavailable")
            checked_target, native_pid, native_creation = proxy, pid, creation
        end
        local expected_pid, expected_creation = native_pid, native_creation
        local function current()
            local driver = world.NetDriver
            return api.valid(world) and api.valid(instance) and api.valid(game_mode) and api.valid(proxy)
                and api.valid(driver) and driver.NetDriverName:ToString() == "GameNetDriver"
                and api.same(driver.World, world) and not api.valid(driver.ServerConnection)
                and api.same(world.AuthorityGameMode, game_mode) and api.same(world.OwningGameInstance, instance)
                and api.valid(world.GameState) and world.GameState.bReplicatedHasBegunPlay == true
                and api.same(proxy, target(world, instance))
        end
        local function terminal(status)
            api.check(status == "pending" or status == "confirmed" or status == "failed" or status == "unknown", "save_call_unknown")
            return status ~= "pending", status == "confirmed", status == "failed" and "save_failed" or nil
        end
        return { world = world, instance = instance, persistence = proxy, save_name = world:GetFullName(),
            native_process_id = expected_pid, native_creation_time = expected_creation, current = current,
            invoke = function(validate, enter_native)
                validate()
                api.check(current(), "save_call_unknown")
                local status, pid, creation, frame = bridge("save", proxy, game_mode, world, validate, nil, enter_native)
                api.check(pid == expected_pid and creation == expected_creation, "save_call_unknown")
                validate(); api.check(current(), "save_call_unknown")
                local complete, confirmed, native_error = terminal(status)
                if complete then return confirmed, native_error end
                return function(check_current)
                    check_current(); api.check(current(), "save_call_unknown")
                    local observed, observed_pid, observed_creation = bridge("save", proxy, game_mode, world, check_current, frame)
                    api.check(observed_pid == expected_pid and observed_creation == expected_creation, "save_call_unknown")
                    check_current(); api.check(current(), "save_call_unknown")
                    return terminal(observed)
                end
            end }
    end
end
