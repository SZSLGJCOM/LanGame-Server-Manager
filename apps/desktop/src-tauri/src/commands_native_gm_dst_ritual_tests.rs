// Execute the production frontend's Lua, replacing only the native entity API.
// No copy of the cleanup predicate is maintained in this fixture.
fn cleanup_command(lua: &mlua::Lua) -> String {
    let source = include_str!("../../src/views/servers/gm-tools-dst.ts");
    let expression = source
        .split_once("const DST_CLEAR_GROUND_ITEMS = ")
        .expect("production cleanup constant")
        .1
        .trim()
        .strip_suffix(';')
        .expect("cleanup constant must remain the final string expression");
    // These single-quoted literals have the same escapes in TypeScript and Lua.
    // A change to the declaration's shape must fail instead of testing stale Lua.
    let expression = expression
        .lines()
        .map(|line| match line.trim_start().strip_prefix("+ ") {
            Some(literal) => format!(".. {literal}"),
            None => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    lua.load(format!("return {expression}"))
        .eval()
        .expect("evaluate the actual cleanup command literals")
}

fn fixture() -> mlua::Lua {
    let lua = mlua::Lua::new();
    lua.load(
        r#"
        Ents = {}
        function entity(id, tags, options)
            options = options or {}
            local inst = { components = { inventoryitem = { owner = options.owner } },
                marking = options.marking, removed = false }
            inst.entity = { GetParent = function() return options.parent end }
            function inst:IsValid() return not options.invalid end
            function inst:IsInLimbo() return options.limbo == true end
            function inst:HasOneOfTags(query)
                for _, tag in ipairs(query) do
                    if tags[tag] then return true end
                end
                return false
            end
            function inst:Remove()
                self.removed = true
                if self.marking then self.marking.detached = true end
            end
            Ents[id] = inst
            return inst
        end
        entity('log', {})
        entity('twigs', {})
        -- 759671 organs keep inventoryitem, nil owner and nil parent in both states.
        for _, organ in ipairs({'rocky', 'bat', 'worm'}) do
            entity('loose_ritual_'..organ, {ritualitem = true})
            entity('placed_ritual_'..organ, {ritualitem = true}, {marking = {}})
        end
        entity('mod_ritual', {ritualitem = true})
        for _, tag in ipairs({'irreplaceable', 'nonpotatable', 'heavy', 'trap', 'mine', 'player'}) do
            entity(tag, {[tag] = true})
        end
        for _, component in ipairs({'equippable', 'container', 'health'}) do
            entity(component, {}).components[component] = {}
        end
        entity('held', {}, {owner = {}})
        entity('parented', {}, {parent = {}})
        entity('limbo', {}, {limbo = true})
        entity('invalid', {}, {invalid = true})
        entity('not_inventory', {}).components.inventoryitem = nil
        entity('no_components', {}).components = nil
        function print(message) last_message = message end
        "#,
    )
    .exec()
    .expect("construct native entity boundary fixture");
    lua
}

const VERIFY: &str = r#"
    for id, inst in pairs(Ents) do
        local should_remove = id == 'log' or id == 'twigs'
        assert(inst.removed == should_remove, 'unexpected removal state for '..id)
        assert(not inst.marking or not inst.marking.detached, 'ritual was detached: '..id)
    end
    assert(last_message == '[LGSM] Removed 2 dropped items', 'incorrect removal receipt')
    return true
"#;

#[test]
fn ground_cleanup_preserves_loose_and_placed_ritual_offerings() {
    let lua = fixture();
    let command = cleanup_command(&lua);
    assert!(command.len() <= 512, "native console command byte limit");
    lua.load(command)
        .exec()
        .expect("execute production cleanup");
    assert!(lua.load(VERIFY).eval::<bool>().expect("cleanup invariants"));
}

#[test]
fn ritual_fixture_rejects_the_previous_cleanup_behavior() {
    let lua = fixture();
    let command = cleanup_command(&lua);
    let previous = command.replacen("\"ritualitem\",", "", 1);
    assert_ne!(
        previous, command,
        "negative control must remove the new guard"
    );
    lua.load(previous)
        .exec()
        .expect("execute the previous cleanup behavior");
    let error = lua
        .load(VERIFY)
        .eval::<bool>()
        .expect_err("old behavior must fail");
    assert!(error.to_string().contains("ritual"), "{error}");
}
