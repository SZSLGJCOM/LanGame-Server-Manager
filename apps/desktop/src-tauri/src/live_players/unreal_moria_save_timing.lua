-- Included only by test + native-test-console, never by a production payload.
-- Read-only observations bind one manager/world and preserve the original save
-- invocation and receipt. The private run identity is never printed.
do
    local original_builder = build_save_target
    local source = debug.getinfo(1, "S").source:gsub("^@", "")
    local root = source:match("^(.+)[/\\]Moria[/\\]Binaries[/\\]Win64[/\\]ue4ss[/\\]Mods[/\\]LgsmWorldSave[/\\]Scripts[/\\]")
    local run = type(os.getenv) == "function" and os.getenv("LGSM_UNREAL_WORLD_SAVE_RUN") or nil
    local records, bound, failed = {}, nil, nil
    local function finite(value)
        return type(value) == "number" and value == value and value > -math.huge and value < math.huge
    end
    local function publish(record)
        if not root or type(run) ~= "string" or #run ~= 41 or not run:match("^LGSM_UWS_[0-9a-f]+$") then return end
        if #records >= 64 then records[64] = record else records[#records + 1] = record end
        local path = root .. "\\langame_world_save\\moria-test-timing.jsonl"
        local file = io.open(path .. ".tmp", "wb")
        if not file then return end
        local written, closed = file:write(table.concat(records, "\n") .. "\n"), file:close()
        if not written or not closed then return end
        local removed, _, code = os.remove(path)
        if not removed and code ~= 2 then return end
        os.rename(path .. ".tmp", path)
    end
    build_save_target = function(api)
        local original = original_builder(api)
        local function sample(target)
            local data = target.persistence.SaveGameData
            if not api.valid(data) or not data:IsA("/Script/Moria.MorSaveGameData") then
                error("reflection_unavailable", 0)
            end
            local objects = {target.world, target.instance, target.persistence, target.state, data}
            if bound then
                for index, object in ipairs(objects) do
                    if not api.same(bound[index], object) then error("world_changed", 0) end
                end
            else bound = objects end
            local params = {{"WorldContextObject", "ObjectProperty"}, {"ReturnValue", "FloatProperty"}}
            local cdo = api.cdo("/Script/Engine.GameplayStatics")
            local function read_clock(name)
                return api.signature("/Script/Engine.GameplayStatics:" .. name, 0x14022401, params)(cdo, target.world)
            end
            local interval, game, real, dilation = data.AutoSaveTime,
                read_clock("GetTimeSeconds"), read_clock("GetRealTimeSeconds"), read_clock("GetGlobalTimeDilation")
            if not finite(interval) or interval <= 0 or not finite(game) or game < 0
                or not finite(real) or real < game or not finite(dilation) or dilation ~= 1 then
                error("clock_unusable", 0)
            end
            return interval, game, real, dilation
        end
        local function observe(stage, target)
            -- This diagnostic must never turn a native request into a failed or
            -- repeated save. Missing/invalid evidence instead fails the oracle.
            pcall(function()
                local ok, interval, game, real, dilation = pcall(sample, target)
                if #records >= 63 then failed = "observation_limit" end
                if not ok then
                    local allowed = {reflection_unavailable=true, world_changed=true, clock_unusable=true}
                    failed = allowed[interval] and interval or "reflection_unavailable"
                end
                local record = string.format('{"run":"%s","sequence":%d,"stage":"%s",', run or "", #records + 1, stage)
                if failed then
                    record = record .. '"valid":false,"cause":"' .. failed .. '"}'
                else
                    record = record .. string.format('"valid":true,"interval":%.17g,"game_seconds":%.17g,"real_seconds":%.17g,"dilation":%.17g}',
                        interval, game, real, dilation)
                end
                publish(record)
            end)
        end
        return function(world, instance, ...)
            local target = original(world, instance, ...)
            observe("inspect", target)
            local invoke = target.invoke
            target.invoke = function(...)
                observe("before", target)
                local result = table.pack(invoke(...))
                observe("after", target)
                return table.unpack(result, 1, result.n)
            end
            return target
        end
    end
end
