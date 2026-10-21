-- Windrose Steam build 24913903. Native SaveBackupsSync has a bool receipt.
-- The separate native bridge pins complete image/loader hashes and its ABI.
local SAVE_CONFIG = { project = "R5", executable = "WindroseServer-Win64-Shipping.exe", fingerprint = {} }
local function build_save_target(api)
    local source = debug.getinfo(1, "S").source:gsub("^@", "")
    local root = source:match("^(.+)[/\\]R5[/\\]Binaries[/\\]Win64[/\\]ue4ss[/\\]Mods[/\\]LgsmWorldSave[/\\]Scripts[/\\]")
    api.check(root ~= nil, "api_unavailable")
    local directory = root .. "\\langame_world_save\\"
    local native
    local counter = 0
    local checked_proxy, native_pid, native_creation
    local function next_nonce()
        counter = counter + 1
        return string.format("%08x%08x%08x%08x", math.random(0, 0x7fffffff), math.random(0, 0x7fffffff),
            math.random(0, 0x7fffffff), counter % 0x7fffffff)
    end
    local function bridge(operation, proxy, instance, validate)
        if not native then
            local loaded = package.loadlib(root .. "\\R5\\Binaries\\Win64\\ue4ss\\Mods\\LgsmWorldSave\\native\\LgsmWindroseSaveControl.dll", "luaopen_lgsm_windrose_save")
            api.check(type(loaded) == "function", "api_unavailable"); native = loaded
        end
        local id = next_nonce()
        local body = "schema=1\nnonce=" .. id .. "\nop=" .. operation .. "\nrequested_at=" .. os.time()
            .. "\nproxy_address=" .. string.format("%d", proxy:GetAddress())
            .. "\ninstance_address=" .. string.format("%d", instance:GetAddress()) .. "\n"
        -- Publish by rename; removing an old leaf never writes through it.
        local temporary = directory .. "native-request." .. id .. ".tmp"
        local file = io.open(temporary, "wb"); api.check(file ~= nil, "api_unavailable")
        local written, closed = file:write(body), file:close()
        api.check(written and closed, "api_unavailable")
        local removed, _, code = os.remove(directory .. "native-request.txt")
        api.check(removed or code == 2, "api_unavailable")
        api.check(os.rename(temporary, directory .. "native-request.txt"), "api_unavailable")
        validate() -- No native entry may follow a cancelled or stale closure.
        native()
        local response = io.open(directory .. "native-response.txt", "rb")
        api.check(response ~= nil, "save_call_unknown")
        local text = response:read(513); response:close()
        api.check(text and #text <= 512, "save_call_unknown")
        local received, op, status, pid, creation = text:match("^schema=1\nnonce=([0-9a-f]+)\nop=([a-z]+)\nstatus=([a-z]+)\npid=(%d+)\ncreation_time=(%d+)\n$")
        api.check(received == id and op == operation and #pid <= 10 and tonumber(pid) > 0
            and #creation <= 20 and creation:sub(1, 1) ~= "0", "save_call_unknown")
        return status, tonumber(pid), creation
    end
    local function target(world, instance)
        api.check(world:GetFullName() == "World /Game/Maps/GYM/Genlandia/GenlandiaMulty.GenlandiaMulty"
            and world.GameState:IsA("/Script/R5.R5GameState") and world.AuthorityGameMode:IsA("/Script/R5.R5GameMode")
            and instance:IsA("/Script/R5.R5GameInstance"), "world_unavailable")
        local proxy = api.unique("R5CoopProxyServer", function(value)
            return value:GetClass():GetFullName() == "Class /Script/R5CoopProxy.R5CoopProxyServer"
                and api.same(value:GetOuter(), instance)
        end)
        local collection = api.unique("R5CoopRootDocumentsCollection", function(value)
            local island = value:GetOuter()
            return api.valid(island) and island:IsA("/Script/R5CoopProxy.R5CoopIslandCollection") and api.same(island:GetOuter(), proxy)
        end)
        local backups, found = FindAllOf("R5CoopRootDocumentBackup"), false
        api.check(type(backups) == "table" and #backups <= 64, "world_unavailable")
        for _, value in ipairs(backups) do
            if api.valid(value) then
                local document = value:GetOuter()
                if api.valid(document) and document:IsA("/Script/R5CoopProxy.R5CoopRootDocument")
                    and api.same(document:GetOuter(), collection) then found = true end
            end
        end
        api.check(found, "world_unavailable")
        return proxy, collection
    end
    return function(world, instance)
        local proxy, backup = target(world, instance)
        if not api.same(checked_proxy, proxy) then
            local status, pid, creation = bridge("probe", proxy, instance, function() end)
            api.check(status == "ready", "api_unavailable")
            checked_proxy, native_pid, native_creation = proxy, pid, creation
        end
        return { world = world, instance = instance, persistence = proxy, state = backup, save_name = "native_checkpoint",
            native_process_id = native_pid, native_creation_time = native_creation,
            invoke = function(validate)
                validate()
                local status, pid, creation = bridge("save", proxy, instance, validate)
                api.check(pid == native_pid and creation == native_creation, "save_call_unknown")
                validate()
                local driver = world.NetDriver
                api.check(api.valid(driver) and driver.NetDriverName:ToString() == "GameNetDriver"
                    and api.same(driver.World, world) and not api.valid(driver.ServerConnection)
                    and api.same(world.OwningGameInstance, instance) and world.GameState.bReplicatedHasBegunPlay == true, "save_call_unknown")
                local current_proxy, current_backup = target(world, instance)
                api.check(api.same(proxy, current_proxy) and api.same(backup, current_backup), "save_call_unknown")
                api.check(status == "confirmed", "save_call_unknown")
                return true
            end }
    end
end
