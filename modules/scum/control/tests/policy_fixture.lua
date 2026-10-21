-- The real SCUM owned native policy/protocol with deterministic native,
-- and file boundaries. This fixture never loads a DLL or touches game data.
local source, game = assert(LGSM_TEST_SOURCE), assert(LGSM_TEST_GAME)
assert(game == "scum")
local conan = false
local project = "SCUM"
local dll = conan and "LgsmConanSaveControl.dll" or "LgsmScumSaveControl.dll"
local export = conan and "luaopen_lgsm_conan_save" or "luaopen_lgsm_scum_save"
local native_class = conan and "WorldPersistenceComponent" or "EntitySystem"
local native_path = conan and "Class /Script/UE4Dreamworld.WorldPersistenceComponent" or "Class /Script/SCUM.EntitySystem"
local base = "D:/fixture\\langame_world_save\\"
local function object(id, class)
    return { IsValid=function(self) return self.valid ~= false end, GetAddress=function() return id end,
        IsA=function(_, expected) return class == expected end }
end
local function text(value) return {ToString=function() return value end} end
local invalid = {IsValid=function() return false end}
local function harness(missing_launch)
    local s={now=1000, files={}, queue={}, serial=0, calls=0, probes=0, random=0, polls=0}
    s.instance=object(1,"/Script/SCUM.ConZGameInstance")
    s.gs=object(3,"/Script/SCUM.ConZGameState");s.gs.bReplicatedHasBegunPlay=true
    s.gm=object(5,"/Script/SCUM.ConZGameMode")
    s.driver=object(4,"driver");s.driver.NetDriverName=text("GameNetDriver");s.driver.ServerConnection=invalid
    function s.replace_world()
        s.world=object(s.world and 22 or 2,"world")
        s.world.GetFullName=function() return s.foreign_map and "World /Game/Entry.Entry" or "World /Game/ConZ_Files/Maps/The_Island/The_Island.The_Island" end
        s.world.GameState=s.gs;s.world.AuthorityGameMode=s.gm;s.world.OwningGameInstance=s.instance
        s.world.NetDriver=s.driver;s.driver.World=s.world
    end
    s.replace_world()
    s.proxy=object(6,"proxy")
    s.proxy.GetClass=function() return {GetFullName=function() return s.wrong_class and "Class Foreign" or native_path end} end
    s.proxy.GetOuter=function() return s.foreign and object(99,"foreign") or (conan and s.gm or s.instance) end
    s.proxy.IsA=function(_,expected) return not s.wrong_super and expected=="/Script/Pal.PalSaveGameManager" end
    s.proxy.GetWorld=function() return s.foreign_world and object(98,"foreign") or s.world end
    local env=setmetatable({}, {__index=_G})
    env.debug={getinfo=function() return {source="@D:/fixture/"..project.."/Binaries/Win64/ue4ss/Mods/LgsmWorldSave/Scripts/main.lua"} end}
    env.math=setmetatable({random=function() s.random=s.random+1;return s.random end},{__index=math})
    env.os={getenv=function(key) assert(key=="LGSM_UNREAL_WORLD_SAVE_RUN");if missing_launch then return nil end;return "LGSM_UWS_"..string.rep("a",32) end,time=function() return s.now end,remove=function(path)
        if s.files[path] then s.files[path]=nil;return true end return nil,"missing",2
    end,rename=function(a,b)
        s.files[b]=s.files[a];s.files[a]=nil
        if b==base.."native-request.txt" and s.on_publish then s.on_publish() end
        return true
    end}
    env.io={open=function(path,mode)
        if path:sub(-4)==".exe" then return {close=function() return true end} end
        if mode=="rb" then
            if not s.files[path] then return nil end
            return {read=function(_,n) return s.files[path]:sub(1,n) end,close=function() return true end}
        end
        return {write=function(_,body) s.files[path]=body;return true end,close=function() return true end}
    end}
    env.package={loadlib=function(path,entry)
        assert(path=="D:/fixture\\"..project.."\\Binaries\\Win64\\ue4ss\\Mods\\LgsmWorldSave\\native\\"..dll)
        if entry == "luaopen_lgsm_scum_arm" then return function() s.armed = true end end
        assert(entry==export)
        if s.no_native then return nil end
        return function()
            assert(s.in_game,"native entry outside EngineTick")
            local req=assert(s.files[base.."native-request.txt"])
            local id,op,time,proxy,instance,world=req:match("^schema=1\nnonce=([0-9a-f]+)\nop=([a-z]+)\nrequested_at=(%d+)\nproxy_address=(%d+)\ninstance_address=(%d+)\nworld_address=(%d+)\n$")
            assert(id and proxy=="6" and instance==(conan and "5" or "1") and world==tostring(s.world:GetAddress()))
            local status="ready"
            if op=="probe" then s.probes=s.probes+1;if s.bad_native then status="rejected" end
            else
                assert(op=="save")
                if s.save_frame then
                    assert(req == s.save_frame, "poll changed original native frame/nonce/timestamp")
                    s.polls=s.polls+1
                    if s.on_poll then s.on_poll() end
                    status=s.poll_status or "confirmed"
                elseif s.now-tonumber(time)>3 then status="rejected" else
                    s.calls=s.calls+1;s.save_frame=req
                    if s.on_call then s.on_call() end
                    if s.throw then error("native failure") end
                    status=s.native_false and "unknown" or (s.false_confirmation and "returned" or s.initial_status or "pending")
                end
            end
            s.files[base.."native-response.txt"]="schema=1\nnonce="..(s.wrong_nonce and string.rep("f",32) or id).."\nop="..op.."\nstatus="..status.."\npid="..(s.replaced_pid and "12" or "11").."\ncreation_time=123456789012345678\n"
        end
    end}
    env.IsInGameThread=function() return s.in_game==true end
    env.EGameThreadMethod={EngineTick=1}
    env.ExecuteInGameThread=function(fn,method) assert(method==1);s.queue[#s.queue+1]=fn end
    env.LoopAsync=function(_,fn) s.tick=fn end
    env.FindAllOf=function(kind)
        if kind=="World" then error("These policies must select the driver, not scan asset worlds") end
        if kind=="NetDriver" then return s.ambiguous and {s.driver,s.driver} or {s.driver} end
        if kind==native_class then return s.duplicate_target and {s.proxy,s.proxy} or {s.proxy} end
        return {}
    end
    assert(load(source,"@fixed-native-save","t",env))()
    function s.dispatch() local fn=table.remove(s.queue,1);if fn then s.in_game=true;fn();s.in_game=false end end
    function s.write(action,boot,world)
        s.serial=s.serial+1;s.id=string.format("%032x",s.serial)
        s.files[base.."request.json"]=string.format('{"protocol":1,"request_id":"%s","requested_at":%d,"action":"%s","boot_id":"%s","world_token":"%s"}',s.id,s.now,action,boot or "",world or "")
    end
    function s.receipt()
        local body=assert(s.files[base.."response.json"],"missing receipt")
        assert(body:find('"request_id":"'..s.id..'"',1,true),body)
        local function flag(name) return body:find('"'..name..'":true',1,true)~=nil end
        return {ready=flag("ready"),invoked=flag("invoked"),returned=flag("call_returned"),confirmed=flag("write_confirmed"),
            boot=body:match('"boot_id":"([0-9a-f]+)"'),world=body:match('"world_token":"([0-9a-f]+)"'),
            error=body:match('"error":"([a-z_]+)"'),pid=body:match('"native_process_id":(%d+)'),creation=body:match('"native_creation_time":"(%d+)"')}
    end
    function s.exchange(action,boot,world)
        s.write(action,boot,world);s.tick();s.dispatch();s.tick()
        for _=1,5 do
            local reply=s.files[base.."response.json"]
            if reply and reply:find('"request_id":"'..s.id..'"',1,true) then return s.receipt() end
            s.dispatch();s.tick()
        end
        error("operation did not finish in fixture")
    end
    function s.probe() local r=s.exchange("saveprobe");assert(r.ready and not r.invoked and not r.confirmed);return r end
    function s.save(r) return s.exchange("save",r.boot,r.world) end
    return s
end
local count=0
local function test(name,fn) local ok,err=pcall(fn);assert(ok,name..": "..tostring(err));count=count+1 end
test("probe returns native identity without saving",function() local s=harness();local r=s.probe();assert(s.calls==0 and s.probes==1 and r.pid=="11" and r.creation=="123456789012345678") end)
test("owned disk snapshot completion is confirmed exactly once",function() local s=harness();local r=s.save(s.probe());assert(r.confirmed and r.invoked and r.returned and not r.error and s.calls==1 and s.polls==1 and s.probes==1);s.tick();s.dispatch();assert(s.calls==1) end)
test("false remains unknown",function() local s=harness();local p=s.probe();s.native_false=true;local r=s.save(p);assert(r.invoked and not r.confirmed and r.error=="save_call_unknown" and s.calls==1) end)
test("native exception remains unknown",function() local s=harness();local p=s.probe();s.throw=true;local r=s.save(p);assert(r.invoked and not r.confirmed and s.calls==1) end)
test("wrong native nonce remains unknown",function() local s=harness();local p=s.probe();s.wrong_nonce=true;assert(not s.save(p).confirmed and s.calls==1) end)
test("different native identity remains unknown",function() local s=harness();local p=s.probe();s.replaced_pid=true;assert(not s.save(p).confirmed and s.calls==1) end)
test("authority loaded game does not depend on a client replication bit",function() local s=harness();s.gs.bReplicatedHasBegunPlay=false;local r=s.probe();assert(r.ready and s.probes==1 and s.calls==0) end)
test("title or login game mode cannot save",function() local s=harness();s.gm.IsA=function() return false end;local r=s.exchange("saveprobe");assert(not r.ready and not r.invoked and s.calls==0 and s.probes==0) end)
test("foreign game state cannot save",function() local s=harness();s.gs.IsA=function() return false end;assert(not s.exchange("saveprobe").ready and s.probes==0) end)
test("foreign proxy cannot save",function() local s=harness();s.foreign=true;local r=s.exchange("saveprobe");assert(not r.ready and s.calls==0 and s.probes==0) end)
test("wrong native class cannot save",function() local s=harness();s.wrong_class=true;assert(not s.exchange("saveprobe").ready and s.probes==0) end)

test("ambiguous worlds cannot save",function() local s=harness();s.ambiguous=true;assert(not s.exchange("saveprobe").ready and s.calls==0) end)
test("native contract rejects unsupported image",function() local s=harness();s.bad_native=true;assert(not s.exchange("saveprobe").ready and s.calls==0) end)
test("timeout while queued cannot invoke later",function() local s=harness();local p=s.probe();s.write("save",p.boot,p.world);s.tick();s.now=s.now+4;s.tick();s.dispatch();s.tick();assert(not s.receipt().invoked and s.calls==0) end)
test("timeout after publication blocks native entry",function() local s=harness();local p=s.probe();s.on_publish=function() s.on_publish=nil;s.now=s.now+4;s.tick() end;local r=s.save(p);assert(r.invoked and not r.confirmed and r.error=="save_call_unknown" and s.calls==0) end)
test("watchdog within native preserves unknown",function() local s=harness();local p=s.probe();s.on_call=function() s.now=s.now+(conan and 4 or 31);s.tick() end;local r=s.save(p);assert(r.invoked and not r.returned and not r.confirmed and s.calls==1);s.tick();s.dispatch();assert(s.calls==1) end)
test("world changes within native preserves unknown",function() local s=harness();local p=s.probe();s.on_call=s.replace_world;local r=s.save(p);assert(r.invoked and not r.confirmed and s.calls==1) end)
test("old boot blocks native entry",function() local s=harness();local p=s.probe();p.boot=string.rep("f",32);assert(not s.save(p).invoked and s.calls==0) end)
test("extra fields and moderation cannot invoke",function() local s=harness();s.write("kick");s.tick();s.dispatch();s.tick();assert(s.calls==0 and not s.files[base.."response.json"]);s.write("saveprobe");s.files[base.."request.json"]=s.files[base.."request.json"]:gsub('}$',',"extra":true}');s.tick();s.dispatch();s.tick();assert(s.calls==0 and not s.files[base.."response.json"]) end)
test("expired replay never resends",function() local s=harness();s.save(s.probe());s.now=s.now+40;s.tick();s.dispatch();s.tick();assert(s.calls==1) end)
test("client net driver cannot save",function() local s=harness();s.driver.ServerConnection=s.instance;assert(not s.exchange("saveprobe").ready and s.calls==0) end)
test("ambiguous native owner cannot save",function() local s=harness();s.duplicate_target=true;assert(not s.exchange("saveprobe").ready and s.calls==0) end)
test("missing loader cannot save",function() local s=harness();s.no_native=true;assert(not s.exchange("saveprobe").ready and s.calls==0) end)
test("missing launch identity cannot save",function() local s=harness(true);assert(not s.exchange("saveprobe").ready and s.calls==0) end)
test("bare returned receipt cannot assert disk completion",function() local s=harness();local p=s.probe();s.false_confirmation=true;local r=s.save(p);assert(r.invoked and not r.confirmed and r.error=="save_call_unknown" and s.calls==1) end)
test("all repeated pending polls retain the original native frame",function()
    local s=harness();local p=s.probe();s.on_poll=function() if s.polls<3 then s.poll_status="pending" else s.poll_status="confirmed" end end
    local r=s.save(p);assert(r.confirmed and s.calls==1 and s.polls==3)
end)
test("delayed native disk snapshot completion uses original timestamp",function()
    local s=harness();local p=s.probe();s.write("save",p.boot,p.world);s.tick();s.dispatch()
    s.now=1010;s.tick();s.dispatch();s.tick();local r=s.receipt()
    assert(r.confirmed and s.calls==1 and s.polls==1 and s.save_frame:find("requested_at=1000",1,true))
end)
test("unknown native status is never confirmation",function()
    local s=harness();local p=s.probe();s.poll_status="mystery";local r=s.save(p)
    assert(not r.confirmed and r.invoked and r.error=="save_call_unknown" and s.calls==1)
end)
test("native write failure is terminal and never confirmed",function()
    local s=harness();local p=s.probe();s.poll_status="failed";local r=s.save(p)
    assert(not r.confirmed and r.invoked and r.returned and r.error=="save_failed" and s.calls==1 and s.polls==1)
end)
test("observer is armed before the first request",function()
    local s=harness();assert(s.armed and s.calls==0 and s.probes==0)
end)
test("world changed during poll stays unknown",function()
    local s=harness();local p=s.probe();s.on_poll=s.replace_world;local r=s.save(p)
    assert(not r.confirmed and r.error=="save_call_unknown" and s.calls==1)
end)
test("changed process while pending cannot confirm",function()
    local s=harness();local p=s.probe();s.on_poll=function() s.replaced_pid=true end;local r=s.save(p)
    assert(not r.confirmed and r.error=="save_call_unknown" and s.calls==1)
end)
test("late pending completion never resends",function()
    local s=harness();local p=s.probe();s.on_poll=function() s.now=1031 end;local r=s.save(p)
    assert(not r.confirmed and s.calls==1);s.tick();s.dispatch();assert(s.calls==1)
end)
test("entered snapshot crosses queue deadline without losing its owned completion",function()
    local s=harness();local p=s.probe()
    s.on_call=function() s.now=s.now+3;s.tick() end
    local r=s.save(p)
    assert(r.confirmed and r.returned and not r.error and s.calls==1 and s.polls==1)
end)
test("observation execution crosses queue deadline within the original completion budget",function()
    local s=harness();local p=s.probe()
    s.on_poll=function() s.now=s.now+3;s.tick() end
    local r=s.save(p)
    assert(r.confirmed and r.returned and not r.error and s.calls==1 and s.polls==1)
end)
test("expired queued callback cannot clear a later operation dispatch",function()
    local s=harness();local p=s.probe()
    s.write("save",p.boot,p.world);s.tick();s.now=s.now+3;s.tick();s.tick()
    s.write("saveprobe");s.tick();assert(#s.queue==2)
    s.dispatch();s.tick();assert(#s.queue==1,"old callback cleared the new dispatch owner")
    s.dispatch();s.tick();local r=s.receipt();assert(r.ready and not r.invoked and s.calls==0)
end)
test("late entered return cannot overwrite a newer queued operation",function()
    local s=harness();local p=s.probe()
    s.on_call=function()
        s.now=s.now+31;s.tick();s.tick();s.write("saveprobe");s.tick()
        assert(#s.queue==1)
    end
    s.write("save",p.boot,p.world);s.tick();s.dispatch()
    s.tick();assert(#s.queue==1,"late native return released a newer dispatch")
    s.dispatch();s.tick();local r=s.receipt();assert(r.ready and not r.invoked and s.calls==1)
end)
test("completion poll still rejects expired EngineTick queue admission",function()
    local s=harness();local p=s.probe();s.write("save",p.boot,p.world);s.tick();s.dispatch()
    s.tick();s.now=s.now+3;s.tick();s.dispatch();s.tick();local r=s.receipt()
    assert(r.invoked and not r.confirmed and r.error=="save_call_unknown" and s.calls==1 and s.polls==0)
end)
-- Feed actual production-encoder output back through the Rust host parser.
test("transition map cannot save",function() local s=harness();s.foreign_map=true;assert(not s.exchange("saveprobe").ready and s.probes==0) end)
test("foreign native world cannot save",function() local s=harness();s.foreign_world=true;assert(not s.exchange("saveprobe").ready and s.probes==0) end)
local contract = harness()
local ready = contract.probe()
LGSM_TEST_PROBE_REQUEST = contract.files[base.."request.json"]
LGSM_TEST_PROBE_REPLY = contract.files[base.."response.json"]
-- The host's positive sample follows the native pending -> confirmed boundary.
-- Unknown/error receipts are exercised separately above and remain failures.
local saved = contract.save(ready)
assert(saved.confirmed and saved.returned and not saved.error and contract.calls==1 and contract.polls==1)
LGSM_TEST_SAVE_REQUEST = contract.files[base.."request.json"]
LGSM_TEST_SAVE_REPLY = contract.files[base.."response.json"]
return count
