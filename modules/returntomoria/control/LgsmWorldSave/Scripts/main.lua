-- Build 21872765. Exact signatures and read-only offsets come from the shipped
-- matching PDB; no arbitrary native addresses are called.
local SAVE_CONFIG = { project = "Moria", executable = "MoriaServer-Win64-Shipping.exe", launch_identity = true,
    fingerprint = { {568, "22928b69"}, {640, "00102c08"},
        {34010096, "40534883ec30488bd9e8a2944e004885c07476488bcb48c744242000000000c7"},
        {39130464, "48895c241048897c241855488bec4883ec6080797000488bd90f841e02000080"} } }
local function build_save_target(api)
    local registered, native, counter, identity = false, nil, 0, nil
    local source = debug.getinfo(1, "S").source:gsub("^@", "")
    local root = source:match("^(.+)[/\\]Moria[/\\]Binaries[/\\]Win64[/\\]ue4ss[/\\]Mods[/\\]LgsmWorldSave[/\\]Scripts[/\\]")
    api.check(root ~= nil, "api_unavailable")
    local directory = root .. "\\langame_world_save\\"
    local function bridge(operation, manager, instance, world, validate)
        if not native then
            native = package.loadlib(root .. "\\Moria\\Binaries\\Win64\\ue4ss\\Mods\\LgsmWorldSave\\native\\LgsmMoriaSaveControl.dll", "luaopen_lgsm_moria_save")
            api.check(type(native) == "function", "api_unavailable")
        end
        counter = counter + 1
        local id = string.format("%08x%08x%08x%08x", math.random(0, 0x7fffffff), math.random(0, 0x7fffffff), math.random(0, 0x7fffffff), counter % 0x7fffffff)
        local body = "schema=1\nnonce=" .. id .. "\nop=" .. operation .. "\nrequested_at=" .. os.time()
            .. "\nproxy_address=" .. string.format("%d", manager:GetAddress())
            .. "\ninstance_address=" .. string.format("%d", instance:GetAddress())
            .. "\nworld_address=" .. string.format("%d", world:GetAddress()) .. "\n"
        local temporary = directory .. "native-request." .. id .. ".tmp"
        local file = io.open(temporary, "wb"); api.check(file ~= nil, "api_unavailable")
        local written, closed = file:write(body), file:close(); api.check(written and closed, "api_unavailable")
        local removed, _, code = os.remove(directory .. "native-request.txt"); api.check(removed or code == 2, "api_unavailable")
        api.check(os.rename(temporary, directory .. "native-request.txt"), "api_unavailable")
        validate(); native()
        local response = io.open(directory .. "native-response.txt", "rb"); api.check(response ~= nil, "save_call_unknown")
        local text = response:read(513); response:close(); api.check(text and #text <= 512, "save_call_unknown")
        local received, op, status, pid, creation = text:match("^schema=1\nnonce=([0-9a-f]+)\nop=([a-z]+)\nstatus=([a-z]+)\npid=(%d+)\ncreation_time=(%d+)\n$")
        api.check(received == id and op == operation and type(pid) == "string" and #pid <= 10 and tonumber(pid) > 0
            and type(creation) == "string" and #creation <= 20 and creation:sub(1, 1) ~= "0", "save_call_unknown")
        return status, tonumber(pid), creation
    end
    local function inspect(world, instance)
        api.check(instance:IsA("/Script/Moria.MoriaGameInstance"), "world_unavailable")
        if not registered then
            for _, entry in ipairs({ {"State", 0x60}, {"Initialized", 0x70}, {"Enabled", 0x71},
                {"Restoring", 0x72}, {"Quicksave", 0x73} }) do
                api.byte("/Script/Moria.MorSaveGameManager", entry[1], entry[2])
            end
            api.byte("/Script/Moria.MorSaveSystemWorldState", "WorldReady", 0x621)
            registered = true
        end
        local manager = api.unique("MorSaveGameManager", function(value)
            return value:IsA("/Script/Moria.MorSaveGameManager") and api.same(value:GetOuter(), instance)
        end)
        api.check(manager.LgsmWorldSaveInitialized == 1 and manager.LgsmWorldSaveEnabled == 1
            and manager.LgsmWorldSaveRestoring == 0 and manager.LgsmWorldSaveQuicksave == 0, "world_unavailable")
        api.check(manager.LgsmWorldSaveState == 0, "save_busy")
        local state = manager.CurrentWorldState:Get()
        api.check(api.valid(state) and state:IsA("/Script/Moria.MorSaveSystemWorldState")
            and api.same(state:GetOuter().OwningWorld, world) and state.LgsmWorldSaveWorldReady == 1
            and state.bAutoSaveEnabled == true, "world_unavailable")
        api.signature("/Script/Moria.MorUIFunctionLibrary:SaveCurrentWorld", 0x4022401,
            {{"WorldContext", "ObjectProperty"}})
        api.cdo("/Script/Moria.MorUIFunctionLibrary")
        if not identity then
            local ready, pid, creation = bridge("probe", manager, instance, world, function() end)
            api.check(ready == "ready", "api_unavailable")
            -- Freeze this process identity. Later final-gate re-inspections must
            -- not overwrite an already-published save with another probe.
            identity = {pid, creation}
        end
        local pid, creation = identity[1], identity[2]
        return { world = world, instance = instance, persistence = manager, state = state,
            native_process_id = pid, native_creation_time = creation,
            invoke = function(validate)
                local status, saved_pid, saved_creation = bridge("save", manager, instance, world, validate)
                api.check(saved_pid == pid and saved_creation == creation, "save_call_unknown")
                validate()
                api.check(status == "confirmed" or status == "returned" or status == "failed", "save_call_unknown")
                return status == "confirmed", status == "failed" and "save_failed" or nil
            end }
    end
    return inspect
end
