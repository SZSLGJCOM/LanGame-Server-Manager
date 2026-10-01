const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const { initializeDontStarveWorldSettings: initialize, applyDontStarveWorldSettingsPatch: apply,
  getDontStarveWorldScriptMode: scriptMode } = require("../src/views/settings/modules/dontstarve-world-lua.ts");
const { parseLuaDataTable: parse, luaScalar, luaChildTable, patchLuaDataTable: patchTable } =
  require("../src/views/settings/modules/dontstarve-lua-data.ts");
const schema = require("../../../modules/dontstarve/schema.json");
const fixtures = require("./fixtures/dontstarve-world-lua.json");

function defaults(shard) {
  return Object.fromEntries(Object.entries(schema.properties)
    .filter(([, property]) => [`${shard}gen`, `${shard}settings`].includes(property["x-lsgm-section"]))
    .map(([key, property]) => [key, property.default]));
}

for (const fixture of fixtures.cases) {
  test(fixture.name, () => {
    const original = structuredClone(fixture.settings);
    let expected = { ...original, ...Object.assign({}, ...fixture.projectedShards.map(defaults)), ...fixture.expected };
    let current = initialize(original);
    assert.deepEqual(current, expected);
    assert.deepEqual(original, fixture.settings, "input is immutable");
    assert.deepEqual(initialize(current), current, "projection is idempotent");
    for (const shard of ["master", "caves"]) assert.equal(scriptMode(current, shard), fixture.script[shard]);
    for (const step of fixture.steps) {
      const before = structuredClone(current);
      const next = apply(current, step.patch);
      expected = { ...expected, ...step.expected };
      if (step.raw !== undefined) expected.master_worldgenoverride_lua = step.raw;
      if (step.extra !== undefined) expected.master_world_overrides_extra = step.extra;
      assert.deepEqual(next, expected);
      assert.deepEqual(current, before, "each patch preserves its expected-state snapshot");
      assert.deepEqual(initialize(next), next, "the next autosave uses the same normalized state");
      current = next;
    }
  });
}

test("adjacent closing braces accept new top-level and nested fields without duplicate separators", () => {
  const source = 'return {override_enabled=true,overrides={day="default"}}';
  const updates = new Map([["settings_preset", "ENDLESS"], ["overrides", new Map([["weather", "rare"]])]]);
  const output = patchTable(source, parse(source), updates);
  const parsed = parse(output);
  assert.ok(parsed, output);
  assert.equal(luaScalar(parsed, "settings_preset"), "ENDLESS");
  assert.equal(luaScalar(luaChildTable(parsed, "overrides"), "day"), "default");
  assert.equal(luaScalar(luaChildTable(parsed, "overrides"), "weather"), "rare");
});

test("comments, bracket keys, long strings and unknown arrays survive a targeted edit", () => {
  const raw = "--[=[header]=]\r\nreturn { override_enabled = true,\r\n overrides = { ['world_size'] = [=[huge]=], -- leave intact\r\n unknown = { false; [7]='kept'; [=[raw { -- text }]=] },\r\n }, custom = 'top-level' } -- tail";
  const next = apply({ master_worldgenoverride_lua: raw }, { master_world_size: "small" });
  assert.equal(next.master_worldgenoverride_lua, raw.replace("[=[huge]=]", '"small"'));
  assert.equal(next.master_world_size, "small");
  assert.equal(scriptMode(next, "master"), false);
});

test("active raw ignores existing extra while guided edits update its matching known key", () => {
  const raw = "return {override_enabled=true,overrides={weather='rare'}}";
  const extra = "weather='always', custom={enabled=false},";
  const current = initialize({ master_worldgenoverride_lua: raw, master_world_overrides_extra: extra });
  assert.equal(current.master_weather, "rare");
  assert.equal(current.master_world_overrides_extra, extra);
  const next = apply(current, { master_weather: "never" });
  assert.equal(next.master_worldgenoverride_lua, raw.replace("'rare'", '"never"'));
  assert.equal(next.master_world_overrides_extra, extra.replace("'always'", '"never"'));
});

test("new raw and active extra input take precedence over simultaneous guided changes", () => {
  const raw = "return {override_enabled=true,overrides={weather='rare'}}";
  const next = apply({ master_weather: "always" }, { master_worldgenoverride_lua: raw, master_weather: "never" });
  assert.equal(next.master_weather, "rare");
  assert.equal(next.master_worldgenoverride_lua, raw);
  const extra = apply({}, { master_world_overrides_extra: "weather='always',", master_weather: "never" });
  assert.equal(extra.master_weather, "always");
});

test("schema raw defaults remain generation mode including Windows newlines", () => {
  const raw = schema.properties.master_worldgenoverride_lua.default.replace(/\n/g, "\r\n");
  const next = apply({ master_worldgenoverride_lua: raw }, { master_weather: "rare" });
  assert.equal(next.master_worldgenoverride_lua, raw);
  assert.equal(next.master_weather, "rare");
  assert.equal(next.master_world_overrides_extra, undefined);
  assert.equal(scriptMode(next, "master"), false);
});

test("changed default values become explicit overrides for non-default presets", () => {
  const current = { master_settings_preset: "RELAXED", master_day: "onlynight" };
  const next = apply(current, { master_day: "default" });
  assert.equal(luaScalar(parse(next.master_world_overrides_extra, true), "day"), "default");
  assert.deepEqual(initialize(next), next);
  assert.equal(apply({ master_day: "onlynight" }, { master_day: "default" }).master_world_overrides_extra, undefined);
});

test("missing overrides and explicit nil accept guided insertion", () => {
  for (const suffix of ["", ",overrides=nil"]) {
    const next = apply({ master_worldgenoverride_lua: `return {override_enabled=true${suffix}}` }, { master_weather: "rare" });
    const parsed = parse(next.master_worldgenoverride_lua);
    assert.ok(parsed);
    assert.equal(luaScalar(luaChildTable(parsed, "overrides"), "weather"), "rare");
  }
  const next = initialize({ master_world_overrides_extra: "world_size=nil", master_world_size: "huge" });
  assert.equal(next.master_world_size, "huge");
});

test("script mode is confined to the affected shard and leaves source bytes untouched", () => {
  for (const raw of [
    "local size='huge'; return {overrides={world_size=size}}",
    "return {override_enabled=false,overrides={weather='rare'}}",
    "return {overrides={weather='rare'}}",
    "return {override_enabled=true,overrides=false}",
    "return {override_enabled=true,overrides={weather={}}}",
    "return {override_enabled=true,preset='not a preset',overrides={}}"
  ]) {
    const current = initialize({ master_worldgenoverride_lua: raw });
    assert.equal(scriptMode(current, "master"), true, raw);
    assert.equal(scriptMode(current, "caves"), false);
    const next = apply(current, { cluster_name: "Preserved", caves_weather: "always" });
    assert.equal(next.master_worldgenoverride_lua, raw);
    assert.equal(next.caves_weather, "always");
    assert.equal(next.cluster_name, "Preserved");
  }
});

test("pure data recognizes common Lua escapes and finite decimal numbers", () => {
  const parsed = parse(String.raw`return { ['wea\116her']='ra\x72e', message='a\n\z   b', n=- 1.25e2 }`);
  assert.ok(parsed);
  assert.equal(luaScalar(parsed, "weather"), "rare");
  assert.equal(luaScalar(parsed, "message"), "a\nb");
  assert.equal(luaScalar(parsed, "n"), -125);
  assert.equal(luaScalar(parse(String.raw`return {text='\195\169'}`), "text"), "é");
  assert.equal(luaScalar(parse(String.raw`return {text='\239\187\191data'}`), "text"), "\ufeffdata");
  assert.equal(luaScalar(parse("return {text='\\z\u00a0data'}"), "text"), "\u00a0data");
});

test("expressions, duplicate keys and unsupported syntax cannot execute or become guided tables", () => {
  for (const source of [
    "return { day = function() error('executed') end }",
    "return {day = os.execute('ignored')}", "return {day = 'a' .. 'b'}",
    "return {day = 'a', ['day']='b'}", "return {1,[1]=2}", "return {}; return {}",
    "return {n=1e999}", "return {n=0xff}", "return {x='\\u{41}'}",
    "return\u00a0{}", "return {x='\\255'}", "return {a=1 --[[unclosed}"
  ]) assert.equal(parse(source), null, source);
  const source = "return {override_enabled=true,overrides={['__proto__']={polluted=true},weather='rare'}}";
  assert.equal(initialize({ master_worldgenoverride_lua: source }).master_weather, "rare");
  assert.equal({}.polluted, undefined);
});

test("input bytes, nesting and total entries have deterministic limits", () => {
  const prefix = "return {}--";
  assert.ok(parse(prefix + "x".repeat(128 * 1024 - prefix.length)));
  assert.equal(parse(prefix + "x".repeat(128 * 1024)), null);
  assert.equal(parse(prefix + "界".repeat(44 * 1024)), null);
  assert.ok(parse("return " + "{".repeat(32) + "1" + "}".repeat(32)));
  assert.equal(parse("return " + "{".repeat(33) + "1" + "}".repeat(33)), null);
  assert.ok(parse("return {" + "1,".repeat(8192) + "}"));
  assert.equal(parse("return {" + "1,".repeat(8193) + "}"), null);
});
