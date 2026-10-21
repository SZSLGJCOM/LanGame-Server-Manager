-- Execute the authoritative shared dispatcher against dedicated-server world
-- boundaries. Only the exact HumanitZ native policy may replace the replicated
-- GameState bit; the dispatcher must still refuse every other project/image.
local protocol = assert(LGSM_TEST_SOURCE)
local checks = 0
local function test(project, executable, native, expected)
    local state = { inspected = 0 }
    local function object(id)
        return {IsValid=function() return true end, GetAddress=function() return id end}
    end
    local world, driver = object(1), object(2)
    world.GameState, world.AuthorityGameMode, world.OwningGameInstance = object(3), object(4), object(5)
    world.GameState.bReplicatedHasBegunPlay = false
    world.NetDriver, driver.World = driver, world
    driver.NetDriverName = {ToString=function() return "GameNetDriver" end}
    local body = '{"protocol":1,"request_id":"'..string.rep('a',32)..'","requested_at":100,"action":"saveprobe","boot_id":"","world_token":""}'
    local environment = setmetatable({
        SAVE_CONFIG={project=project,executable=executable,launch_identity=true,world_from_netdriver=true,native_world_readiness=native,fingerprint={}},
        debug={getinfo=function() return {source="@D:/fixture/"..project.."/Binaries/Win64/ue4ss/Mods/LgsmWorldSave/Scripts/main.lua"} end},
        os={getenv=function() return "LGSM_UWS_"..string.rep('b',32) end,time=function() return 100 end,remove=function() return nil,nil,2 end,
            rename=function() state.reply=state.written; return true end},
        io={open=function(path,mode)
            if mode=="rb" then return {read=function() return body end,close=function() return true end} end
            return {write=function(_,value) state.written=value; return true end,close=function() return true end}
        end},
        FindAllOf=function(class) assert(class=="NetDriver"); return {driver} end,
        IsInGameThread=function() return true end,
        ExecuteInGameThread=function(callback) callback() end,EGameThreadMethod={EngineTick=1},
        LoopAsync=function(_,callback) state.loop=callback end,print=function() end,
        build_save_target=function() return function()
            state.inspected=state.inspected+1
            return {world=world,instance=world.OwningGameInstance,persistence=world.OwningGameInstance,save_name="world"}
        end end,
    }, {__index=_G})
    assert(load(protocol,"@shared-world-save","t",environment))()
    state.loop();state.loop()
    checks=checks+1;assert(state.reply:find('"ready":'..tostring(expected),1,true),state.reply)
    checks=checks+1;assert(state.inspected==(expected and 1 or 0),"unexpected native-policy admission")
end
test("HumanitZServer","HumanitZServer-Win64-Shipping.exe",true,true)
test("HumanitZServer","Other-Win64-Shipping.exe",true,false)
test("ForeignProject","HumanitZServer-Win64-Shipping.exe",true,false)
test("HumanitZServer","HumanitZServer-Win64-Shipping.exe",false,false)
return checks
