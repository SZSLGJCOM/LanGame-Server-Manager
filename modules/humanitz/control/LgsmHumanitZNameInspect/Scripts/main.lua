-- Read-only follow-up metadata for the fixed runtime Script snapshot.
-- No game calls, hooks, FName construction, property values or save commands.
local run = os.getenv("LGSM_HUMANITZ_INSPECT_RUN") or ""
local function nonce(value) return type(value) == "string" and #value == 32 and value:match("^[0-9a-f]+$") ~= nil end
if not nonce(run) then print("[LgsmHumanitZNameInspect] Missing diagnostic identity.\n"); return end
local source = debug.getinfo(1, "S").source:gsub("^@", "")
local root = source:match("^(.+)[/\\]HumanitZServer[/\\]Binaries[/\\]Win64[/\\]ue4ss[/\\]Mods[/\\]LgsmHumanitZNameInspect[/\\]Scripts[/\\]")
if not root then print("[LgsmHumanitZNameInspect] Invalid diagnostic placement.\n"); return end
local directory = root .. "\\langame_humanitz_inspect\\"
local active, recent, accepted = false, {}, 0
local fault, progress = {}, {}
local issues = { bounded_metadata=true,game_thread_unavailable=true,api_missing=true,
    function_class_unavailable=true,object_outer_unavailable=true,name_index_unavailable=true,
    property_offset_unavailable=true,address_unavailable=true,output_limit=true,
    output_already_exists=true,io_failed=true,metadata_name_unavailable=true,
    function_flags_unavailable=true,owner_class_unavailable=true }
local function classify(value)
    if type(value) ~= "string" then return "api_error" end
    if issues[value] then return value end
    if value:find("attempt to call",1,true) then return "call_error" end
    if value:find("attempt to index",1,true) then return "index_error" end
    if value:lower():find("overload",1,true) then return "signature_error" end
    if value:find("bad argument",1,true) then return "argument_error" end
    return "api_error"
end
local function observed(stage, operation)
    local ok, first, second = pcall(operation)
    if not ok then
        if not fault.stage then fault={stage=stage,issue=classify(first),kind=type(first)} end
        error("inspect_stage_failed",0)
    end
    return first, second
end
local function check(value, code) if not value then error(code,0) end end
local function valid(value) return value ~= nil and value:IsValid() == true end
local function quote(value)
    check(type(value) == "string" and #value <= 2048, "bounded_metadata")
    return '"' .. value:gsub('[%z\1-\31\\"]', function(c) return string.format("\\u%04x", c:byte()) end) .. '"'
end
local function address(value)
    local result=value:GetAddress()
    check(math.type(result)=="integer" and result>0,"address_unavailable")
    return string.format("%d",result)
end
local function name(value)
    local result=value:GetFullName()
    check(type(result)=="string" and #result<=2048,"metadata_name_unavailable")
    return result
end
local function family(value)
    return value:find("BP_HMZRCon.BP_HMZRCon_C", 1, true)
        or value:find("BP_GameModeSave.BP_GameModeSave_C", 1, true)
        or value:find("BP_GameInstanceWithSettings.BP_GameInstanceWithSettings_C", 1, true)
end
local function collect(id)
    observed("game_thread",function()
        check(type(IsInGameThread)=="function","api_missing")
        check(IsInGameThread()==true,"game_thread_unavailable")
    end)
    local function_class = observed("function_class",function()
        local value=StaticFindObject("/Script/CoreUObject.Function")
        check(valid(value),"function_class_unavailable");return value
    end)
    local functions, objects, classes, seen_classes, visited, property_count, complete = {}, {}, {}, {}, 0, 0, true
    local function properties_of(object,category)
        local properties, properties_complete = {}, true
        observed(category.."_enumeration",function() object:ForEachProperty(function(property)
            if #properties >= 8192 or property_count >= 65536 then
                complete, properties_complete = false, false; return true
            end
            property_count = property_count + 1
            progress.properties=property_count
            local offset = observed("field_offset",function()
                local result=property:GetOffset_Internal()
                check(math.type(result)=="integer" and result>=0 and result<=16*1024*1024,"property_offset_unavailable")
                return result
            end)
            local field_name=observed("field_name",function() return name(property) end)
            local field_address=observed("field_descriptor",function() return address(property) end)
            properties[#properties + 1] = '{"name":' .. quote(field_name)
                .. ',"address":' .. quote(field_address) .. ',"offset":' .. offset .. '}'
        end) end)
        return properties, properties_complete
    end
    observed("object_enumeration",function() ForEachUObject(function(object)
        visited = visited + 1
        progress.visited=visited
        if visited > 524288 then complete = false; return end
        if not observed("object_validity",function() return valid(object) end) then return end
        if observed("function_type",function() return object:IsA(function_class) end) then
            if #functions >= 65536 then complete = false; return end
            local function_name=observed("function_name",function() return name(object) end)
            local outer=observed("function_outer",function()
                local value=object:GetOuter();check(valid(value),"object_outer_unavailable");return value
            end)
            local owner=observed("function_owner",function() return name(outer) end)
            local properties, properties_complete = {}, true
            if family(owner) then
                properties, properties_complete = observed("function_fields",function() return properties_of(object,"function_fields") end)
                local outer_id = observed("owner_address",function() return address(outer) end)
                if not seen_classes[outer_id] then
                    seen_classes[outer_id] = true
                    -- GetOuter deliberately returns a plain UObject wrapper in
                    -- this pinned loader. StaticFindObject auto-constructs the
                    -- real UClass wrapper; it only looks up this existing owner.
                    local owner_class=observed("owner_class_lookup",function()
                        local path=owner:match("^BlueprintGeneratedClass (/.+)$")
                        check(path~=nil,"owner_class_unavailable")
                        local value=StaticFindObject(path)
                        check(valid(value) and value:IsAnyClass() and address(value)==outer_id and name(value)==owner,"owner_class_unavailable")
                        return value
                    end)
                    local fields, fields_complete = observed("class_fields",function() return properties_of(owner_class,"class_fields") end)
                    classes[#classes + 1] = '{"name":' .. quote(owner) .. ',"address":' .. quote(outer_id)
                        .. ',"properties_complete":' .. tostring(fields_complete) .. ',"properties":[' .. table.concat(fields, ",") .. ']}'
                end
            end
            local fname=observed("get_fname",function() return object:GetFName() end)
            local index=observed("comparison_index",function()
                local result=fname:GetComparisonIndex()
                check(math.type(result)=="integer" and result>=0 and result<=0xffffffff,"name_index_unavailable")
                return result
            end)
            local function_address=observed("function_address",function() return address(object) end)
            local flags=observed("function_flags",function()
                local result=object:GetFunctionFlags()
                check(math.type(result)=="integer" and result>=0 and result<=0xffffffff,"function_flags_unavailable")
                return result
            end)
            functions[#functions + 1] = '{"name":' .. quote(function_name) .. ',"address":' .. quote(function_address)
                .. ',"comparison_index":' .. index .. ',"flags":' .. flags .. ',"owner":' .. quote(owner)
                .. ',"properties_requested":' .. tostring(family(owner) ~= nil)
                .. ',"properties_complete":' .. tostring(properties_complete) .. ',"properties":[' .. table.concat(properties, ",") .. ']}'
            progress.functions=#functions
        else
            observed("object_metadata",function()
            local class = object:GetClass()
            if valid(class) and family(name(class)) then
                if #objects >= 128 then complete = false; return end
                local outer = object:GetOuter()
                objects[#objects + 1] = '{"class":' .. quote(class:GetFullName()) .. ',"address":' .. quote(address(object))
                    .. ',"default":' .. tostring(object:GetFullName():find("Default__", 1, true) ~= nil)
                    .. ',"outer_address":' .. quote(valid(outer) and address(outer) or "")
                    .. ',"outer_class":' .. quote(valid(outer) and outer:GetClass():GetFullName() or "") .. '}'
            end
            end)
        end
    end) end)
    table.sort(functions); table.sort(objects); table.sort(classes)
    local body = '{"schema":1,"module":"humanitz","stage":"name_metadata_only","run":' .. quote(run)
        .. ',"nonce":' .. quote(id) .. ',"complete":' .. tostring(complete) .. ',"visited":' .. visited
        .. ',"property_count":' .. property_count .. ',"functions":[' .. table.concat(functions, ",")
        .. '],"objects":[' .. table.concat(objects, ",") .. '],"classes":[' .. table.concat(classes, ",") .. ']}\n'
    observed("output_size",function() check(#body<=32*1024*1024,"output_limit") end)
    local output = directory .. "names-" .. id .. ".json"
    observed("output_lookup",function()
        local existing=io.open(output,"rb")
        if existing then existing:close();error("output_already_exists",0) end
    end)
    local file=observed("output_open",function()
        local result=io.open(output..".tmp","wb");check(result~=nil,"io_failed");return result
    end)
    observed("output_write",function() check(file:write(body),"io_failed") end)
    observed("output_close",function() check(file:close(),"io_failed") end)
    observed("output_publish",function() check(os.rename(output..".tmp",output),"io_failed") end)
    print("[LgsmHumanitZNameInspect] Read-only name metadata published.\n")
end
local function failed(id)
    local stage,issue,kind=fault.stage or "collect",fault.issue or "api_error",fault.kind or "unknown"
    print("[LgsmHumanitZNameInspect] Read-only name metadata failed stage="..stage.." issue="..issue.." kind="..kind..".\n")
    local body='{"schema":1,"module":"humanitz","stage":"name_metadata_failed","run":'..quote(run)..',"nonce":'..quote(id)
        ..',"failure_stage":'..quote(stage)..',"issue":'..quote(issue)..',"error_kind":'..quote(kind)
        ..',"visited":'..(progress.visited or 0)..',"function_count":'..(progress.functions or 0)..',"property_count":'..(progress.properties or 0)..'}\n'
    local output=directory.."name-error-"..id..".json"
    local existing=io.open(output,"rb");if existing then existing:close();return end
    local file=io.open(output..".tmp","wb");if not file then return end
    local written,closed=file:write(body),file:close()
    if written and closed then os.rename(output..".tmp",output) end
end
LoopAsync(250, function()
    if active or accepted >= 8 then return end
    local file = io.open(directory .. "names-request.txt", "rb")
    if not file then return end
    local body = file:read(160); file:close()
    local target, id
    if body then target, id = body:match("^schema=1\nrun=([0-9a-f]+)\nnonce=([0-9a-f]+)\n$") end
    if target ~= run or not nonce(id) or recent[id] then return end
    active, recent[id], accepted = true, true, accepted + 1
    local queued = pcall(ExecuteInGameThread, function()
        fault,progress={},{}
        local ok = pcall(collect, id)
        if not ok then pcall(failed,id) end
        active = false
    end)
    if not queued then active = false end
end)
print("[LgsmHumanitZNameInspect] Diagnostic ready; waiting for an owned request.\n")
