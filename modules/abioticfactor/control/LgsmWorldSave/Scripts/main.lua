-- Build 24343458. Factory returns a registered async action; Activate queues
-- the world's native save. Completion is deliberately not inferred from logs.
local SAVE_CONFIG = { project = "AbioticFactor", executable = "AbioticFactorServer-Win64-Shipping.exe", launch_identity = true,
    fingerprint = { {488, "b8c3e42b"}, {560, "00c03c08"},
        {67643792, "405356574883ec30488bf94883c158e85c0824fd84c0750eb202488d0ddf3693"},
        {67772320, "48895c24104889742418574881ecc0000000488b05473281034833c448898424"},
        {67773972, "885e70e8341ee6fc33d248896c2420"},
        {67636019, "48897b6048897b6840887b7048897b784889"} } }
local function build_save_target(api)
    local registered = false
    return function(world, instance)
        api.check(instance:IsA("/Script/AbioticFactor.AbioticGameInstance"), "world_unavailable")
        if not registered then
            api.byte("/Script/AbioticFactor.AbioticGameInstance", "Saving", 0x2e0)
            api.byte("/Script/AbioticFactor.SaveQueueSubsystem", "InFlight", 0x50)
            api.byte("/Script/AbioticFactor.SaveCurrentWorld", "Succeeded", 0x70)
            api.check(PropertyTypes.Int64Property ~= nil, "api_unavailable")
            RegisterCustomProperty({ Name = "LgsmWorldSaveCompletionHandle", Type = PropertyTypes.Int64Property,
                BelongsToClass = "/Script/AbioticFactor.SaveCurrentWorld", OffsetInternal = 0x60 })
            RegisterCustomProperty({ Name = "LgsmWorldSaveQueueCount", Type = PropertyTypes.IntProperty,
                BelongsToClass = "/Script/AbioticFactor.SaveQueueSubsystem", OffsetInternal = 0x48 })
            registered = true
        end
        local subsystem = api.unique("SaveQueueSubsystem", function(value)
            return value:IsA("/Script/AbioticFactor.SaveQueueSubsystem") and api.same(value:GetOuter(), world)
        end)
        api.check(instance.LgsmWorldSaveSaving == 0 and subsystem.LgsmWorldSaveInFlight == 0
            and subsystem.LgsmWorldSaveQueueCount == 0, "save_busy")
        local name = instance.ActiveWorldSaveName:ToString()
        api.check(type(name) == "string" and #name > 0 and #name <= 256, "world_unavailable")
        local factory = api.signature("/Script/AbioticFactor.SaveCurrentWorld:SaveCurrentWorld", 0x4022401,
            {{"WorldContextObject", "ObjectProperty"}, {"ReturnValue", "ObjectProperty"}})
        local activate = api.signature("/Script/Engine.BlueprintAsyncActionBase:Activate", 0x4020402, {})
        local cdo = api.cdo("/Script/AbioticFactor.SaveCurrentWorld")
        local function current()
            local live = api.unique("World", function(candidate)
                local driver = candidate.NetDriver
                return api.valid(driver) and driver.NetDriverName:ToString() == "GameNetDriver"
                    and api.same(driver.World, candidate) and not api.valid(driver.ServerConnection)
            end)
            return api.valid(world) and api.valid(instance) and api.valid(subsystem)
                and api.same(live, world)
                and api.same(world.OwningGameInstance, instance) and api.same(subsystem:GetOuter(), world)
                and api.valid(world.GameState) and world.GameState.bReplicatedHasBegunPlay == true
                and instance.ActiveWorldSaveName:ToString() == name
                and api.valid(world.NetDriver) and api.same(world.NetDriver.World, world)
                and not api.valid(world.NetDriver.ServerConnection)
        end
        return { world = world, instance = instance, persistence = subsystem, save_name = name, current = current,
            invoke = function(validate)
                validate()
                local action = factory(cdo, world)
                api.check(api.valid(action) and action:IsA("/Script/AbioticFactor.SaveCurrentWorld"), "save_call_unknown")
                api.check(action.LgsmWorldSaveSucceeded == 0 and action.LgsmWorldSaveCompletionHandle == 0, "save_call_unknown")
                validate() -- Factory may release the VM lock; never activate a stale request.
                activate(action)
                -- This exact newly-created action owns the completion. However,
                -- the standard worker ignores SaveDataToSlot's bool result, so
                -- its bSaveSuccess cannot confirm a successful file write.
                -- A failed worker is known failure; success stays unconfirmed.
                return function(validate_completion)
                    validate_completion(); api.check(current() and api.valid(action), "target_changed")
                    local succeeded, handle = action.LgsmWorldSaveSucceeded, action.LgsmWorldSaveCompletionHandle
                    api.check((succeeded == 0 or succeeded == 1) and type(handle) == "number" and handle >= 0, "save_call_unknown")
                    if succeeded == 1 then return true, false end
                    if handle ~= 0 then return true, false, "save_failed" end
                    return false, false
                end
            end }
    end
end
