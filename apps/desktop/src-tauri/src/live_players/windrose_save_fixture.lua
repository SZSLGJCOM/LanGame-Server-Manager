-- The real bundled Windrose policy/protocol with deterministic engine, native,
-- and file boundaries. This fixture never loads a DLL or touches game data.
local source = assert(LGSM_TEST_SOURCE)
local base = "D:/fixture\\langame_world_save\\"
local function object(id, class)
    return { IsValid=function(self) return self.valid ~= false end, GetAddress=function() return id end,
        IsA=function(_, expected) return class == expected end }
end
local function text(value) return {ToString=function() return value end} end
local invalid = {IsValid=function() return false end}
local function harness()
    local s={now=1000, files={}, queue={}, serial=0, calls=0, probes=0, random=0}
    s.instance=object(1,"/Script/R5.R5GameInstance")
    s.gs=object(3,"/Script/R5.R5GameState");s.gs.bReplicatedHasBegunPlay=true
    s.driver=object(4,"driver");s.driver.NetDriverName=text("GameNetDriver");s.driver.ServerConnection=invalid
    s.world=object(2,"world");s.world.GetFullName=function() return s.lobby and "World Lobby" or "World /Game/Maps/GYM/Genlandia/GenlandiaMulty.GenlandiaMulty" end
    s.world.GameState=s.gs;s.world.AuthorityGameMode=object(5,"/Script/R5.R5GameMode");s.world.OwningGameInstance=s.instance
    s.world.NetDriver=s.driver;s.driver.World=s.world
    s.proxy=object(6,"proxy");s.proxy.GetClass=function() return {GetFullName=function() return "Class /Script/R5CoopProxy.R5CoopProxyServer" end} end
    s.proxy.GetOuter=function() return s.foreign and object(99,"foreign") or s.instance end
    s.island=object(7,"/Script/R5CoopProxy.R5CoopIslandCollection");s.island.GetOuter=function() return s.proxy end
    s.collection=object(8,"collection");s.collection.GetOuter=function() return s.island end
    s.document=object(9,"/Script/R5CoopProxy.R5CoopRootDocument");s.document.GetOuter=function() return s.collection end
    s.backup=object(10,"backup");s.backup.GetOuter=function() return s.document end
    local env=setmetatable({}, {__index=_G})
    env.debug={getinfo=function() return {source="@D:/fixture/R5/Binaries/Win64/ue4ss/Mods/LgsmWorldSave/Scripts/main.lua"} end}
    env.math=setmetatable({random=function() s.random=s.random+1;return s.random end},{__index=math})
    env.os={time=function() return s.now end,remove=function(path)
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
        assert(path=="D:/fixture\\R5\\Binaries\\Win64\\ue4ss\\Mods\\LgsmWorldSave\\native\\LgsmWindroseSaveControl.dll")
        assert(entry=="luaopen_lgsm_windrose_save")
        if s.no_native then return nil end
        return function()
            assert(s.in_game,"native entry outside EngineTick")
            local req=assert(s.files[base.."native-request.txt"])
            local id,op,time,proxy,instance=req:match("^schema=1\nnonce=([0-9a-f]+)\nop=([a-z]+)\nrequested_at=(%d+)\nproxy_address=(%d+)\ninstance_address=(%d+)\n$")
            assert(id and proxy=="6" and instance=="1")
            local status="ready"
            if op=="probe" then s.probes=s.probes+1;if s.bad_native then status="rejected" end
            else
                assert(op=="save")
                if s.now-tonumber(time)>3 then status="rejected" else
                    s.calls=s.calls+1
                    if s.on_call then s.on_call() end
                    if s.throw then error("native failure") end
                    status=s.native_false and "unknown" or "confirmed"
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
        if kind=="World" then return s.ambiguous and {s.world,s.world} or {s.world} end
        if kind=="R5CoopProxyServer" then return {s.proxy} end
        if kind=="R5CoopRootDocumentsCollection" then return {s.collection} end
        if kind=="R5CoopRootDocumentBackup" then return s.missing_backup and {} or {s.backup,s.backup} end
        return {}
    end
    assert(load(source,"@windrose-save","t",env))()
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
    function s.exchange(action,boot,world) s.write(action,boot,world);s.tick();s.dispatch();s.tick();return s.receipt() end
    function s.probe() local r=s.exchange("saveprobe");assert(r.ready and not r.invoked and not r.confirmed);return r end
    function s.save(r) return s.exchange("save",r.boot,r.world) end
    return s
end
local count=0
local function test(name,fn) local ok,err=pcall(fn);assert(ok,name..": "..tostring(err));count=count+1 end
test("probe returns native identity without saving",function() local s=harness();local r=s.probe();assert(s.calls==0 and s.probes==1 and r.pid=="11" and r.creation=="123456789012345678") end)
test("true checkpoints exactly once",function() local s=harness();local r=s.save(s.probe());assert(r.confirmed and r.invoked and r.returned and s.calls==1 and s.probes==1);s.tick();s.dispatch();assert(s.calls==1) end)
test("false remains unknown",function() local s=harness();local p=s.probe();s.native_false=true;local r=s.save(p);assert(r.invoked and not r.confirmed and r.error=="save_call_unknown" and s.calls==1) end)
test("native exception remains unknown",function() local s=harness();local p=s.probe();s.throw=true;local r=s.save(p);assert(r.invoked and not r.confirmed and s.calls==1) end)
test("wrong native nonce remains unknown",function() local s=harness();local p=s.probe();s.wrong_nonce=true;assert(not s.save(p).confirmed and s.calls==1) end)
test("different native identity remains unknown",function() local s=harness();local p=s.probe();s.replaced_pid=true;assert(not s.save(p).confirmed and s.calls==1) end)
test("lobby cannot save",function() local s=harness();s.lobby=true;local r=s.exchange("saveprobe");assert(not r.ready and not r.invoked and s.calls==0 and s.probes==0) end)
test("foreign proxy cannot save",function() local s=harness();s.foreign=true;local r=s.exchange("saveprobe");assert(not r.ready and s.calls==0 and s.probes==0) end)
test("missing document cannot save",function() local s=harness();s.missing_backup=true;assert(not s.exchange("saveprobe").ready and s.probes==0) end)
test("ambiguous worlds cannot save",function() local s=harness();s.ambiguous=true;assert(not s.exchange("saveprobe").ready and s.calls==0) end)
test("native contract rejects unsupported image",function() local s=harness();s.bad_native=true;assert(not s.exchange("saveprobe").ready and s.calls==0) end)
test("timeout while queued cannot invoke later",function() local s=harness();local p=s.probe();s.write("save",p.boot,p.world);s.tick();s.now=s.now+4;s.tick();s.dispatch();s.tick();assert(not s.receipt().invoked and s.calls==0) end)
test("timeout after publication blocks native entry",function() local s=harness();local p=s.probe();s.on_publish=function() s.on_publish=nil;s.now=s.now+4;s.tick() end;local r=s.save(p);assert(r.invoked and not r.confirmed and r.error=="save_call_unknown" and s.calls==0) end)
test("watchdog within native preserves unknown",function() local s=harness();local p=s.probe();s.on_call=function() s.now=s.now+4;s.tick() end;local r=s.save(p);assert(r.invoked and not r.returned and not r.confirmed and s.calls==1);s.tick();s.dispatch();assert(s.calls==1) end)
test("world changes within native preserves unknown",function() local s=harness();local p=s.probe();s.on_call=function() s.lobby=true end;local r=s.save(p);assert(r.invoked and not r.confirmed and s.calls==1) end)
test("old boot blocks native entry",function() local s=harness();local p=s.probe();p.boot=string.rep("f",32);assert(not s.save(p).invoked and s.calls==0) end)
test("extra fields and moderation cannot invoke",function() local s=harness();s.write("kick");s.tick();s.dispatch();s.tick();assert(s.calls==0 and not s.files[base.."response.json"]);s.write("saveprobe");s.files[base.."request.json"]=s.files[base.."request.json"]:gsub('}$',',"extra":true}');s.tick();s.dispatch();s.tick();assert(s.calls==0 and not s.files[base.."response.json"]) end)
test("expired replay never resends",function() local s=harness();s.save(s.probe());s.now=s.now+40;s.tick();s.dispatch();s.tick();assert(s.calls==1) end)
return count
