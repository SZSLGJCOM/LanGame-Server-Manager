-- Diagnostic only. No gameplay functions, hooks, save or RCON commands are called.
-- The operator creates the owned exchange directory in a physical test copy.
local run = os.getenv("LGSM_HUMANITZ_INSPECT_RUN") or ""
local function nonce(value) return type(value) == "string" and #value == 32 and value:match("^[0-9a-f]+$") ~= nil end
if not nonce(run) then print("[LgsmHumanitZInspect] Missing owned diagnostic launch identity.\n"); return end
local source = debug.getinfo(1, "S").source:gsub("^@", "")
local root = source:match("^(.+)[/\\]HumanitZServer[/\\]Binaries[/\\]Win64[/\\]ue4ss[/\\]Mods[/\\]LgsmHumanitZInspect[/\\]Scripts[/\\]")
if not root then print("[LgsmHumanitZInspect] Invalid diagnostic placement.\n"); return end
local directory = root .. "\\langame_humanitz_inspect\\"
local active, recent, accepted = false, {}, 0
local function valid(object) return object ~= nil and object:IsValid() == true end
local function quoted(value)
    return '"' .. tostring(value):gsub('[%z\1-\31\\"]', function(character)
        return string.format("\\u%04x", character:byte())
    end) .. '"'
end
local function address(object) return string.format("%d", object:GetAddress()) end
local function collect(id)
    local functions, objects, truncated, visited = {}, {}, false, 0
    local function_class = StaticFindObject("/Script/CoreUObject.Function")
    if not valid(function_class) then error("function_class_unavailable", 0) end
    ForEachUObject(function(object)
        visited = visited + 1
        if visited > 524288 then truncated = true; return end
        if not valid(object) then return end
        if object:IsA(function_class) then
            local name = object:GetFullName()
            if name:find("Function /Game/", 1, true) == 1 or name:find("BPDataReceived", 1, true)
                or name:find("SaveGameToSlot", 1, true) then
                if #functions >= 16384 or #name > 1024 then truncated = true; return end
                local properties = {}
                object:ForEachProperty(function(property)
                    if #properties >= 128 then truncated = true; return true end
                    properties[#properties + 1] = quoted(property:GetFullName())
                end)
                functions[#functions + 1] = '{"name":' .. quoted(name) .. ',"address":' .. quoted(address(object))
                    .. ',"flags":' .. object:GetFunctionFlags() .. ',"owner":' .. quoted(object:GetOuter():GetFullName())
                    .. ',"properties":[' .. table.concat(properties, ",") .. ']}'
            end
        else
            local class = object:GetClass()
            if valid(class) then
                local name = class:GetFullName()
                if name:find("HMZRcon", 1, true) or name:find("HMZGameMode", 1, true)
                    or name:find("HMZGameInstance", 1, true) then
                    if #objects >= 128 then truncated = true; return end
                    objects[#objects + 1] = '{"class":' .. quoted(name) .. ',"address":' .. quoted(address(object)) .. '}'
                end
            end
        end
    end)
    table.sort(functions); table.sort(objects)
    local body = '{"schema":1,"module":"humanitz","stage":"reflection_only","run":' .. quoted(run)
        .. ',"nonce":' .. quoted(id) .. ',"complete":' .. tostring(not truncated) .. ',"visited":' .. visited
        .. ',"functions":[' .. table.concat(functions, ",") .. '],"objects":[' .. table.concat(objects, ",") .. ']}\n'
    if #body > 16 * 1024 * 1024 then error("diagnostic_output_limit", 0) end
    local file = assert(io.open(directory .. "reflection.json.tmp", "wb"))
    assert(file:write(body)); assert(file:close())
    local removed, _, code = os.remove(directory .. "reflection.json")
    if not removed and code ~= 2 then error("diagnostic_output_unavailable", 0) end
    assert(os.rename(directory .. "reflection.json.tmp", directory .. "reflection.json"))
    print("[LgsmHumanitZInspect] Read-only reflection snapshot published.\n")
end
LoopAsync(250, function()
    if active or accepted >= 8 then return end
    local file = io.open(directory .. "request.txt", "rb")
    if not file then return end
    local body = file:read(160); file:close()
    local target, id
    if body then target, id = body:match("^schema=1\nrun=([0-9a-f]+)\nnonce=([0-9a-f]+)\n$") end
    if target ~= run or not nonce(id) or recent[id] then return end
    active, recent[id], accepted = true, true, accepted + 1
    local scheduled = pcall(ExecuteInGameThread, function()
        local ok = pcall(collect, id)
        if not ok then print("[LgsmHumanitZInspect] Read-only reflection snapshot failed.\n") end
        active = false
    end)
    if not scheduled then active = false end
end)
print("[LgsmHumanitZInspect] Diagnostic ready; waiting for an owned request.\n")
