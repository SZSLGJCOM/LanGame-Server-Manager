-- Factory + Activate is the native current-world request. The game's queue
-- owns busy handling; returning from Activate is not a storage result.
local SAVE_CONFIG = { project = "AbioticFactor", executable = "AbioticFactorServer-Win64-Shipping.exe", launch_identity = true }
local function build_save_target(api)
    return function(world, instance)
        api.check(instance:IsA("/Script/AbioticFactor.AbioticGameInstance"), "world_unavailable")
        local subsystem = api.unique("SaveQueueSubsystem", function(value)
            return value:IsA("/Script/AbioticFactor.SaveQueueSubsystem") and api.same(value:GetOuter(), world)
        end)
        local name = instance.ActiveWorldSaveName:ToString()
        api.check(type(name) == "string" and #name > 0 and #name <= 256, "world_unavailable")
        local factory = api.signature("/Script/AbioticFactor.SaveCurrentWorld:SaveCurrentWorld", 0x4022401,
            {{"WorldContextObject", "ObjectProperty"}, {"ReturnValue", "ObjectProperty"}})
        local activate = api.signature("/Script/Engine.BlueprintAsyncActionBase:Activate", 0x4020402, {})
        local cdo = api.cdo("/Script/AbioticFactor.SaveCurrentWorld")
        return { world = world, instance = instance, persistence = subsystem, save_name = name,
            invoke = function(validate, enter_native)
                validate()
                local action = factory(cdo, world)
                api.check(api.valid(action) and action:IsA("/Script/AbioticFactor.SaveCurrentWorld"), "save_call_unknown")
                -- The factory may release the VM lock. Recheck the original
                -- world/request gate immediately before activating it once.
                enter_native()
                activate(action)
                return false
            end }
    end
end
