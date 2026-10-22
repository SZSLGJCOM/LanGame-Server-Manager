-- The native reflected entry owns save readiness and its asynchronous queue.
-- A returned void call acknowledges a request; it cannot confirm a disk write.
local SAVE_CONFIG = { project = "Moria", executable = "MoriaServer-Win64-Shipping.exe", launch_identity = true }
local function build_save_target(api)
    return function(world, instance)
        api.check(instance:IsA("/Script/Moria.MoriaGameInstance"), "world_unavailable")
        local manager = api.unique("MorSaveGameManager", function(value)
            return value:IsA("/Script/Moria.MorSaveGameManager") and api.same(value:GetOuter(), instance)
        end)
        local state = manager.CurrentWorldState:Get()
        api.check(api.valid(state) and state:IsA("/Script/Moria.MorSaveSystemWorldState")
            and api.same(state:GetOuter().OwningWorld, world) and state.bAutoSaveEnabled == true, "world_unavailable")
        local save = api.signature("/Script/Moria.MorUIFunctionLibrary:SaveCurrentWorld", 0x4022401,
            {{"WorldContext", "ObjectProperty"}})
        local cdo = api.cdo("/Script/Moria.MorUIFunctionLibrary")
        return { world = world, instance = instance, persistence = manager, state = state,
            invoke = function(_, enter_native)
                enter_native()
                save(cdo, world)
                return false
            end }
    end
end
