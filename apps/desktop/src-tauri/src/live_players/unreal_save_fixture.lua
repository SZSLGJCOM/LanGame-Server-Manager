-- Trusted, deterministic fixture. No native DLL, real filesystem, or server is
-- loaded. Only I/O, UObject reflection and the EngineTick boundary are mocked.
local source, game = assert(LGSM_TEST_SOURCE), assert(LGSM_TEST_GAME)
local moria = game == "moria"
local project = moria and "Moria" or "AbioticFactor"
local class = moria and "/Script/Moria.MoriaGameInstance" or "/Script/AbioticFactor.AbioticGameInstance"
local base = "D:/fixture\\langame_world_save\\"
local invalid = { IsValid=function() return false end }
local function text(value) return { ToString=function() return value end } end
local function object(id, kind)
    return { IsValid=function(self) return self.valid ~= false end, GetAddress=function() return id end,
        IsA=function(_, expected) return expected == kind end }
end
local function harness(missing_launch)
    local s = { now=1000, files={}, queue={}, calls=0, factories=0, serial=0, random=0,
        clock=0, binary_reads=0, properties={} }
    s.environment_token="LGSM_UWS_"..string.rep("a",32)
    if missing_launch then s.environment_token=nil end
    s.instance = object(1, class); s.gs = object(3, "gs"); s.gs.bReplicatedHasBegunPlay = true
    s.driver = object(4, "driver"); s.driver.NetDriverName = text("GameNetDriver"); s.driver.ServerConnection = invalid
    function s.replace_world()
        s.world = object(s.world and 22 or 2, "/Script/Engine.World")
        s.world.OwningGameInstance, s.world.GameState, s.world.AuthorityGameMode = s.instance, s.gs, object(5,"gm")
        s.world.NetDriver, s.driver.World = s.driver, s.world
    end
    s.replace_world()
    s.manager = object(6, "/Script/Moria.MorSaveGameManager")
    s.manager.GetOuter=function() return s.instance end
    s.manager.LgsmWorldSaveInitialized, s.manager.LgsmWorldSaveEnabled = 1, 1
    s.manager.LgsmWorldSaveRestoring, s.manager.LgsmWorldSaveQuicksave, s.manager.LgsmWorldSaveState = 0, 0, 0
    s.worldstate = object(7, "/Script/Moria.MorSaveSystemWorldState")
    s.worldstate.GetOuter=function() return {OwningWorld=s.world} end
    s.worldstate.LgsmWorldSaveWorldReady, s.worldstate.bAutoSaveEnabled = 1, true
    s.manager.CurrentWorldState = {Get=function() return s.worldstate end}
    s.subsystem = object(8, "/Script/AbioticFactor.SaveQueueSubsystem")
    s.subsystem.GetOuter=function() return s.world end
    s.subsystem.LgsmWorldSaveInFlight, s.subsystem.LgsmWorldSaveQueueCount = 0, 0
    s.instance.LgsmWorldSaveSaving, s.instance.ActiveWorldSaveName = 0, text("CurrentWorld")
    local env = setmetatable({}, {__index=_G})
    env.debug={ getinfo=function() return {source="@D:/fixture/"..project.."/Binaries/Win64/ue4ss/Mods/LgsmWorldSave/Scripts/main.lua"} end }
    env.math=setmetatable({random=function() s.random=s.random+1; return s.random end},{__index=math})
    env.os={ getenv=function(key) assert(key=="LGSM_UNREAL_WORLD_SAVE_RUN");return s.environment_token end, time=function()
        s.clock=s.clock+1
        if s.on_time then s.on_time(s.clock) end
        return s.now
    end, remove=function(path) if s.files[path] then s.files[path]=nil; return true end return nil,"missing",2 end,
        rename=function(a,b) s.files[b]=s.files[a];s.files[a]=nil;return true end }
    local sites={}
    for offset,hex in source:gmatch('{(%d+), "([0-9a-f]+)"}') do
        sites[tonumber(offset)]=hex:gsub("..",function(v) return string.char(tonumber(v,16)) end)
    end
    local site_count=0;for _ in pairs(sites) do site_count=site_count+1 end;assert(site_count==(moria and 4 or 6))
    env.io={ open=function(path, mode)
        if path:sub(-4)==".exe" then
            local position=0
            return {seek=function(_,how,offset) assert(how=="set");position=offset;return offset end,
                read=function(_,count) s.binary_reads=s.binary_reads+1;return s.bad_binary and "" or sites[position] end,
                close=function() return true end}
        end
        if mode=="rb" then
            if not s.files[path] then return nil end
            return {read=function(_,maximum) return s.files[path]:sub(1,maximum) end,close=function() return true end}
        end
        return {write=function(_,body) s.files[path]=body;return true end,close=function() return true end}
    end }
    env.IsInGameThread=function() return s.in_game == true end
    env.EGameThreadMethod={EngineTick=1}
    env.ExecuteInGameThread=function(fn, method) assert(method==1);s.queue[#s.queue+1]=fn end
    env.LoopAsync=function(_,fn) s.tick=fn end
    env.PropertyTypes={ByteProperty=1,IntProperty=2,Int64Property=3}
    env.RegisterCustomProperty=function(info)
        assert(info.Name:sub(1,13)=="LgsmWorldSave")
        s.properties[info.Name]=info
    end
    env.FindAllOf=function(kind)
        if kind=="World" then return s.ambiguous and {s.world,s.world} or {s.world} end
        if kind=="MorSaveGameManager" then return {s.manager} end
        if kind=="SaveQueueSubsystem" then return {s.subsystem} end
        return {}
    end
    local function native(path, flags, params, callback)
        local fn=object(100,path)
        fn.GetFunctionFlags=function() return s.bad_signature and 0 or flags end
        fn.ForEachProperty=function(_,visit)
            for _,entry in ipairs(params) do
                visit({GetFName=function() return text(entry[1]) end,
                    GetClass=function() return {GetFName=function() return text(entry[2]) end} end})
            end
        end
        return setmetatable(fn,{__call=function(_,...) return callback(...) end})
    end
    local functions={}
    local function effect()
        assert(s.in_game);s.calls=s.calls+1
        if s.on_call then s.on_call() end
        if s.throw then error("native failure") end
    end
    functions["/Script/Moria.MorUIFunctionLibrary:SaveCurrentWorld"]=native("save",0x4022401,
        {{"WorldContext","ObjectProperty"}},function(_,world) assert(world==s.world);effect() end)
    functions["/Script/AbioticFactor.SaveCurrentWorld:SaveCurrentWorld"]=native("factory",0x4022401,
        {{"WorldContextObject","ObjectProperty"},{"ReturnValue","ObjectProperty"}},function(_,world)
            assert(s.in_game and world==s.world);s.factories=s.factories+1
            if s.on_factory then s.on_factory() end
            s.action=object(101,"/Script/AbioticFactor.SaveCurrentWorld")
            s.action.LgsmWorldSaveSucceeded=0;s.action.LgsmWorldSaveCompletionHandle=0
            return s.action
        end)
    functions["/Script/Engine.BlueprintAsyncActionBase:Activate"]=native("activate",0x4020402,{},function()
        effect()
        if not s.defer_completion then s.action.LgsmWorldSaveSucceeded=s.native_failed and 0 or 1;s.action.LgsmWorldSaveCompletionHandle=1234 end
    end)
    env.package={loadlib=function(path,export)
        local name="LgsmMoriaSaveControl.dll"
        assert(moria and export=="luaopen_lgsm_moria_save" and path:sub(-#name)==name)
        return function()
            local body=assert(s.files[base.."native-request.txt"])
            local id,op=body:match("^schema=1\nnonce=([0-9a-f]+)\nop=([a-z]+)\n")
            assert(id and (op=="probe" or op=="save"))
            local status="ready"
            if op=="save" then effect();status=s.native_failed and "failed" or "confirmed" end
            s.files[base.."native-response.txt"]="schema=1\nnonce="..id.."\nop="..op.."\nstatus="..status.."\npid=11\ncreation_time=123456789012345678\n"
        end
    end}
    env.StaticFindObject=function(path)
        if functions[path] then return functions[path] end
        local result=object(200,path);result.GetCDO=function() return object(201,path) end;return result
    end
    assert(load(source,"@bundled-native-save","t",env))()
    function s.dispatch()
        local fn=table.remove(s.queue,1);if fn then s.in_game=true;fn();s.in_game=false end
    end
    function s.write(action,run,world)
        s.serial=s.serial+1;s.id=string.format("%032x",s.serial)
        s.files[base.."request.json"]=string.format('{"protocol":1,"request_id":"%s","requested_at":%d,"action":"%s","boot_id":"%s","world_token":"%s"}',s.id,s.now,action,run or "",world or "")
    end
    function s.receipt()
        local body=assert(s.files[base.."response.json"],"missing receipt")
        assert(body:find('"request_id":"'..s.id..'"',1,true),body)
        local function flag(key) return body:find('"'..key..'":true',1,true)~=nil end
        return {ready=flag("ready"),invoked=flag("invoked"),returned=flag("call_returned"),
            confirmed=flag("write_confirmed"),
            boot=body:match('"boot_id":"([0-9a-f]+)"'),world=body:match('"world_token":"([0-9a-f]+)"'),
            launch=body:match('"launch_token":"([A-Z_0-9a-f]*)"'),
            error=body:match('"error":"([a-z_]+)"')}
    end
    function s.exchange(action,run,world) s.write(action,run,world);s.tick();s.dispatch();s.tick();s.dispatch();s.tick();return s.receipt() end
    function s.probe() local reply=s.exchange("saveprobe");assert(reply.ready and not reply.invoked and not reply.returned);return reply end
    function s.save(reply) return s.exchange("save",reply.boot,reply.world) end
    return s
end
local count=0
local function test(name,fn) local ok,err=pcall(fn);assert(ok,name..": "..tostring(err));count=count+1 end
test("preflight has no effect",function() local s=harness();s.probe();assert(s.calls==0 and s.factories==0 and s.binary_reads==(moria and 4 or 6)) end)
test("one native save uses only its own result",function() local s=harness();local r=s.save(s.probe());assert(r.invoked and r.returned and r.confirmed==moria and not r.error and s.calls==1,s.files[base.."response.json"].." calls="..s.calls);s.tick();s.dispatch();assert(s.calls==1) end)
test("native completed failure is not success",function() local s=harness();local p=s.probe();s.native_failed=true;local r=s.save(p);assert(r.invoked and r.returned and not r.confirmed and r.error=="save_failed" and s.calls==1) end)
test("native exception is unknown",function() local s=harness();local p=s.probe();s.throw=true;local r=s.save(p);assert(r.invoked and not r.returned and r.error=="save_call_unknown" and s.calls==1) end)
test("busy is rejected",function() local s=harness();local p=s.probe();s.manager.LgsmWorldSaveState=1;s.subsystem.LgsmWorldSaveInFlight=1;local r=s.save(p);assert(not r.invoked and r.error=="save_busy" and s.calls==0) end)
test("queue busy is rejected",function() local s=harness();local p=s.probe();s.manager.LgsmWorldSaveState=1;s.subsystem.LgsmWorldSaveQueueCount=1;assert(not s.save(p).invoked and s.calls==0) end)
test("world replacement is rejected",function() local s=harness();local p=s.probe();s.replace_world();local r=s.save(p);assert(not r.invoked and r.error=="target_changed" and s.calls==0) end)
test("boot replacement is rejected",function() local s=harness();local p=s.probe();p.boot=string.rep("f",32);assert(s.save(p).error=="target_changed" and s.calls==0) end)
test("world token replacement is rejected",function() local s=harness();local p=s.probe();p.world=string.rep("f",32);assert(s.save(p).error=="target_changed" and s.calls==0) end)
test("signature drift is rejected",function() local s=harness();local p=s.probe();s.bad_signature=true;assert(not s.save(p).invoked and s.calls==0) end)
test("binary drift is rejected",function() local s=harness();local p=s.probe();s.bad_binary=true;assert(not s.save(p).invoked and s.calls==0) end)
test("not ready is rejected",function() local s=harness();local p=s.probe();s.gs.bReplicatedHasBegunPlay=false;assert(not s.save(p).invoked and s.calls==0) end)
test("ambiguous world is rejected",function() local s=harness();local p=s.probe();s.ambiguous=true;assert(s.save(p).error=="world_ambiguous" and s.calls==0) end)
test("wrong game instance is rejected",function() local s=harness();local p=s.probe();s.instance.IsA=function() return false end;assert(not s.save(p).invoked and s.calls==0) end)
test("server connection is rejected",function() local s=harness();local p=s.probe();s.driver.ServerConnection=s.instance;assert(not s.save(p).invoked and s.calls==0) end)
test("pre-call timeout cannot invoke later",function() local s=harness();local p=s.probe();s.write("save",p.boot,p.world);s.tick();s.now=s.now+3;s.tick();s.dispatch();s.tick();local r=s.receipt();assert(not r.invoked and s.calls==0) end)
test("timeout inside native stays unknown",function() local s=harness();local p=s.probe();s.on_call=function() s.now=s.now+3;s.tick() end;local r=s.save(p);assert(r.invoked and not r.returned and r.error=="save_call_unknown" and s.calls==1);s.tick();s.dispatch();assert(s.calls==1) end)
test("timeout at final gate stays unknown",function()
    local s=harness();local p=s.probe();s.write("save",p.boot,p.world);s.tick();s.clock=0
    s.on_time=function(n) if n==7 then s.on_time=nil;s.now=s.now+3;s.tick() end end
    s.dispatch();s.tick();local r=s.receipt();assert(r.invoked and r.error=="save_call_unknown" and s.calls==0)
end)
test("unknown moderation action cannot invoke",function() local s=harness();s.write("kick");s.tick();s.dispatch();s.tick();assert(s.calls==0 and not s.files[base.."response.json"]) end)
test("extra fields cannot invoke",function() local s=harness();s.write("saveprobe");s.files[base.."request.json"]=s.files[base.."request.json"]:gsub('}$',',"extra":true}');s.tick();s.dispatch();s.tick();assert(s.calls==0 and not s.files[base.."response.json"]) end)
test("replay remains inert after TTL",function() local s=harness();s.save(s.probe());s.now=s.now+40;s.tick();s.dispatch();s.tick();assert(s.calls==1) end)
test("missing launch environment rejects before any effect",function() local s=harness(true);local r=s.exchange("saveprobe");assert(not r.ready and not r.invoked and s.calls==0) end)
test("launch identity is frozen at module initialization",function() local s=harness();local p=s.probe();local token=s.environment_token;s.environment_token="LGSM_UWS_"..string.rep("b",32);local r=s.save(p);assert(r.launch==token and r.invoked and s.calls==1) end)
if moria then
    test("restore blocks native save",function() local s=harness();local p=s.probe();s.manager.LgsmWorldSaveRestoring=1;assert(not s.save(p).invoked and s.calls==0) end)
    test("disabled autosave is not overridden",function() local s=harness();local p=s.probe();s.worldstate.bAutoSaveEnabled=false;assert(not s.save(p).invoked and s.calls==0) end)
    test("worldstate readiness is checked",function() local s=harness();local p=s.probe();s.worldstate.LgsmWorldSaveWorldReady=0;assert(not s.save(p).invoked and s.calls==0) end)
else
    test("factory is activated once",function() local s=harness();s.save(s.probe());assert(s.factories==1 and s.calls==1) end)
    test("native busy is checked",function() local s=harness();local p=s.probe();s.instance.LgsmWorldSaveSaving=1;assert(not s.save(p).invoked and s.factories==0) end)
    test("world replacement inside factory blocks activation",function() local s=harness();local p=s.probe();s.on_factory=s.replace_world;local r=s.save(p);assert(r.invoked and r.error=="save_call_unknown" and s.factories==1 and s.calls==0) end)
    test("queued action is not completion",function()
        local s=harness();local p=s.probe();s.defer_completion=true;s.write("save",p.boot,p.world)
        s.tick();s.dispatch();s.tick();s.dispatch();s.tick();assert(not s.files[base.."response.json"]:find('"request_id":"'..s.id..'"',1,true) and s.calls==1)
        s.action.LgsmWorldSaveSucceeded=1;s.action.LgsmWorldSaveCompletionHandle=1234
        s.dispatch();s.tick();local r=s.receipt();assert(r.returned and not r.confirmed and not r.error and s.calls==1)
    end)
    test("unrelated global queue idle cannot complete an action",function()
        local s=harness();local p=s.probe();s.defer_completion=true;s.write("save",p.boot,p.world);s.tick();s.dispatch();s.tick()
        s.instance.LgsmWorldSaveSaving=0;s.subsystem.LgsmWorldSaveInFlight=0
        s.dispatch();s.tick();assert(s.calls==1 and not s.files[base.."response.json"]:find('"request_id":"'..s.id..'"',1,true))
        s.now=s.now+31;s.tick();s.dispatch();s.tick();local r=s.receipt();assert(not r.confirmed and r.error=="save_call_unknown" and s.calls==1)
    end)
    test("world replacement during completion remains unknown",function()
        local s=harness();local p=s.probe();s.defer_completion=true;s.write("save",p.boot,p.world);s.tick();s.dispatch();s.tick()
        s.replace_world();s.action.LgsmWorldSaveSucceeded=1;s.dispatch();s.tick();local r=s.receipt();assert(not r.confirmed and r.error=="save_call_unknown" and s.calls==1)
    end)
end
return count
