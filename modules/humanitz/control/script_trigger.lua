-- Optional separate diagnostic mod: LgsmHumanitZScriptInspect/Scripts/main.lua.
-- It triggers only the read-only native Script copier, never a game/save API.
local run = os.getenv("LGSM_HUMANITZ_INSPECT_RUN") or ""
local function nonce(text) return type(text)=="string" and #text==32 and text:match("^[0-9a-f]+$") ~= nil end
if not nonce(run) then return end
local source=debug.getinfo(1,"S").source:gsub("^@","")
local root=source:match("^(.+)[/\\]HumanitZServer[/\\]Binaries[/\\]Win64[/\\]ue4ss[/\\]Mods[/\\]LgsmHumanitZScriptInspect[/\\]Scripts[/\\]")
if not root then return end
local path=root.."\\langame_humanitz_inspect\\scripts-request.txt"
local recent,active,count={},false,0
LoopAsync(250,function()
    if active or count>=8 then return end
    local file=io.open(path,"rb");if not file then return end
    local body=file:read(65537);file:close()
    if not body or #body>65536 then return end
    local target,id=body:match("^schema=1\nrun=([0-9a-f]+)\nnonce=([0-9a-f]+)\nfunction=")
    if target~=run or not nonce(id) or recent[id] then return end
    recent[id],active,count=true,true,count+1
    local scheduled=pcall(ExecuteInGameThread,function()
        local library=root.."\\HumanitZServer\\Binaries\\Win64\\ue4ss\\Mods\\LgsmHumanitZInspect\\native\\LgsmHumanitZInspect.dll"
        local ok=pcall(function() local invoke=assert(package.loadlib(library,"luaopen_lgsm_humanitz_inspect"));invoke() end)
        print(ok and "[LgsmHumanitZScriptInspect] Read-only Script copy attempted.\n"
            or "[LgsmHumanitZScriptInspect] Read-only Script helper unavailable.\n")
        active=false
    end)
    if not scheduled then active=false end
end)
