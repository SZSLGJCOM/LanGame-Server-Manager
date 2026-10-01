const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");
for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    module._compile(transpileTypeScript(source, filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("", module.filename);

const { dontStarveSettingsDefinition } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "modules",
  "dontstarve.ts"
));
const schema = JSON.parse(fs.readFileSync(path.join(
  desktopRoot,
  "..",
  "..",
  "modules",
  "dontstarve",
  "schema.json"
), "utf8"));
const moduleToml = fs.readFileSync(path.join(
  desktopRoot,
  "..",
  "..",
  "modules",
  "dontstarve",
  "module.toml"
), "utf8");
const { applyDontStarveOperationalPatch } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "modules",
  "dontstarve-validation.ts"
));

const context = {
  locale: "en-US",
  t: (_key, _params, fallback) => fallback ?? "missing translation"
};

function defaults() {
  return Object.fromEntries(Object.entries(schema.properties)
    .filter(([, property]) => Object.hasOwn(property, "default"))
    .map(([key, property]) => [key, structuredClone(property.default)]));
}

function issues(settings) {
  return dontStarveSettingsDefinition.getSettingsValidationIssues(settings, context);
}

test("DST defaults and a raw-only shard file remain valid", () => {
  const baseline = defaults();
  assert.deepEqual(issues(baseline), []);

  baseline.master_worldgenoverride_lua = "return { override_enabled = true, overrides = { day = 'onlyday' } }";
  assert.deepEqual(issues(baseline), [], "a raw escape hatch is valid when guided fields stay at defaults");
});

test("legacy game mode cannot conflict with a different explicit playstyle", () => {
  const settings = { ...defaults(), game_mode: "endless", master_settings_preset: "RELAXED" };
  assert.ok(issues(settings).some((issue) => issue.reason === "conflicting-playstyle"));
  assert.equal(issues({ ...settings, game_mode: "survival" }).length, 0);
  assert.equal(issues({ ...settings, master_settings_preset: "ENDLESS", master_worldgen_preset: "ENDLESS" }).length, 0);
});

test("changing a playstyle preset removes the legacy mode conflict in the same edit", () => {
  assert.deepEqual(applyDontStarveOperationalPatch({ game_mode: "endless" }, { master_settings_preset: "RELAXED" }),
    { master_settings_preset: "RELAXED", game_mode: "survival" });
  assert.deepEqual(applyDontStarveOperationalPatch(defaults(), { game_mode: "wilderness" }),
    { game_mode: "wilderness", master_settings_preset: "WILDERNESS", master_worldgen_preset: "WILDERNESS" });
});

test("editable raw Master presets participate in playstyle validation and normalization", () => {
  const raw = "return { override_enabled = true, settings_preset = 'RELAXED', worldgen_preset = 'RELAXED', overrides = {} }";
  const current = { ...defaults(), game_mode: "endless", master_worldgenoverride_lua: raw };
  assert.ok(issues(current).some((entry) => entry.reason === "conflicting-playstyle"));

  const presetPatch = dontStarveSettingsDefinition.applySettingsPatch(current, { master_settings_preset: "RELAXED" });
  assert.equal(presetPatch.game_mode, "survival");
  assert.deepEqual(issues(presetPatch).filter((entry) => entry.reason === "conflicting-playstyle"), []);

  const modePatch = dontStarveSettingsDefinition.applySettingsPatch(current, { game_mode: "wilderness" });
  assert.equal(modePatch.game_mode, "wilderness");
  assert.match(modePatch.master_worldgenoverride_lua, /settings_preset = "WILDERNESS"/u);
  assert.match(modePatch.master_worldgenoverride_lua, /worldgen_preset = "WILDERNESS"/u);
  assert.deepEqual(issues(modePatch).filter((entry) => entry.reason === "conflicting-playstyle"), []);
});

test("raw Master preset fallbacks are checked while opaque scripts stay authoritative", () => {
  const baseline = { ...defaults(), game_mode: "endless" };
  const fallback = { ...baseline, master_worldgenoverride_lua:
    "return { override_enabled = true, preset = 'RELAXED', overrides = {} }" };
  assert.ok(issues(fallback).some((entry) => entry.reason === "conflicting-playstyle"));
  assert.deepEqual(issues({ ...baseline, master_worldgenoverride_lua:
    "return { override_enabled = true, overrides = {} }" }).filter((entry) => entry.reason === "conflicting-playstyle"), []);
  assert.deepEqual(issues({ ...baseline, master_worldgenoverride_lua:
    "return make_world()" }).filter((entry) => entry.reason === "conflicting-playstyle"), []);
});

test("DST data collection opt-out and online mode are normalized as one operation", () => {
  const baseline = defaults();
  assert.deepEqual(
    applyDontStarveOperationalPatch(baseline, { disable_data_collection: true }),
    { disable_data_collection: true, offline_cluster: true }
  );
  assert.deepEqual(
    applyDontStarveOperationalPatch(
      { ...baseline, disable_data_collection: true, offline_cluster: true },
      { offline_cluster: false }
    ),
    { offline_cluster: false, disable_data_collection: false }
  );
});

test("DST launches Master with an instance-owned shard UGC directory", () => {
  assert.match(
    moduleToml,
    /"-ugc_directory",\s*"\{\{paths\.data_dir\}\}\/ugc\/Master"/u
  );
  assert.doesNotMatch(moduleToml, /clusters\/main\/ugc/u);
});

test("DST rejects raw modoverrides combined with typed lists or structured options per shard", () => {
  for (const shard of ["master", "caves"]) {
    const withOptions = defaults();
    withOptions.enable_caves = true;
    withOptions[`${shard}_modoverrides_lua`] = "return { ['workshop-123456789'] = { enabled = true } }";
    withOptions[`${shard}_mod_configuration_options`] = {
      "123456789": { difficulty: "hard" }
    };
    assert.ok(issues(withOptions).some((entry) =>
      entry.fieldKey === `${shard}_modoverrides_lua` && entry.reason === "raw-structured-conflict"
    ));

    const withEnabledList = defaults();
    withEnabledList.enable_caves = true;
    withEnabledList[`${shard}_modoverrides_lua`] = "return { ['workshop-123456789'] = { enabled = true } }";
    withEnabledList[`${shard}_enabled_workshop_mod_ids`] = "123456789";
    assert.ok(issues(withEnabledList).some((entry) =>
      entry.fieldKey === `${shard}_modoverrides_lua` && entry.reason === "raw-structured-conflict"
    ));
  }
});

test("DST world input sources no longer block saving other instance settings", () => {
  const cases = [
    ["master", "master_day", "onlyday"],
    ["caves", "caves_weather", "often"]
  ];
  for (const [shard, guidedKey, guidedValue] of cases) {
    const changedGuided = defaults();
    changedGuided[`${shard}_worldgenoverride_lua`] = "return { override_enabled = true, overrides = {} }";
    changedGuided[guidedKey] = guidedValue;
    assert.deepEqual(issues(changedGuided), []);

    const extraLines = defaults();
    extraLines[`${shard}_worldgenoverride_lua`] = "return { override_enabled = true, overrides = {} }";
    extraLines[`${shard}_world_overrides_extra`] = "day = 'onlyday',";
    assert.deepEqual(issues(extraLines), []);
  }
});

test("DST structured Mod options reject every non-primitive leaf", () => {
  const invalidValues = [null, ["nested"], { nested: true }, Number.POSITIVE_INFINITY];
  for (const invalidValue of invalidValues) {
    const settings = defaults();
    settings.master_mod_configuration_options = {
      "123456789": { invalid: invalidValue }
    };
    assert.ok(issues(settings).some((entry) =>
      entry.fieldKey === "master_mod_configuration_options" &&
      entry.reason === "invalid-mod-option-value"
    ));
  }

  const valid = defaults();
  valid.enable_caves = true;
  valid.caves_mod_configuration_options = {
    "123456789": { text: "value", count: 3.5, enabled: false }
  };
  assert.deepEqual(issues(valid), []);
});

test("disabled Caves preserve their world and Mod settings without blocking unrelated saves", () => {
  const baseline = defaults();
  baseline.enable_caves = false;
  assert.deepEqual(issues(baseline), []);

  baseline.caves_mod_configuration_options = { "123456789": { enabled: true } };
  baseline.caves_modoverrides_lua = "return { ['workshop-123456789'] = { enabled = true } }";
  baseline.caves_weather = "often";
  const preserved = structuredClone(baseline);
  assert.deepEqual(issues(baseline), []);
  assert.deepEqual(baseline, preserved);
  baseline.enable_caves = true;
  assert.ok(issues(baseline).some((entry) => entry.reason === "raw-structured-conflict"),
    "enabling Caves restores validation of the active Mod sources");
});

test("DST preset choices and raw data remain independent between shards", () => {
  for (const shard of ["master", "caves"]) {
    for (const kind of ["settings", "worldgen"]) {
      const settings = defaults();
      settings.enable_caves = true;
      settings[`${shard}_${kind}_preset`] = shard === "master" ? "ENDLESS" : "custom_cave";
      assert.deepEqual(issues(settings), []);
      const otherShard = shard === "master" ? "caves" : "master";
      settings[`${otherShard}_worldgenoverride_lua`] = "return { override_enabled = true, overrides = {} }";
      assert.deepEqual(issues(settings), [], "the other shard's raw file remains independent");
      settings[`${shard}_worldgenoverride_lua`] = "return { override_enabled = true, overrides = {} }";
      assert.deepEqual(issues(settings), [], "the module input normalizer handles world source synchronization");
    }
  }
});

test("DST can retain a custom Caves preset while Caves are disabled", () => {
  for (const kind of ["settings", "worldgen"]) {
    const settings = defaults();
    settings.enable_caves = false;
    settings[`caves_${kind}_preset`] = "custom_cave";
    assert.deepEqual(issues(settings), []);
    assert.equal(settings[`caves_${kind}_preset`], "custom_cave");
  }
});

test("DST preset inheritance is explained before the fields without duplicating existing add-ons", () => {
  const { listConfigurationSpecializedRenderers } = require(path.join(
    desktopRoot, "src", "views", "settings", "module-registry.ts"
  ));
  const { getDontStarvePresetNotice } = require(path.join(
    desktopRoot, "src", "views", "settings", "modules", "dontstarve-preset-notice.tsx"
  ));
  const { EN_US_DONT_STARVE_MESSAGES: en } = require(path.join(
    desktopRoot, "src", "i18n", "games", "dontstarve.en.ts"
  ));
  const { ZH_CN_DONT_STARVE_MESSAGES: zh } = require(path.join(
    desktopRoot, "src", "i18n", "games", "dontstarve.zh-cn.ts"
  ));
  const translate = (catalog) => (key, params, fallback) => (catalog[key] ?? fallback)
    .replace(/\{(\w+)\}/gu, (match, key) => params?.[key] ?? match);

  for (const section of ["mastergen", "mastersettings", "cavesgen", "cavessettings"]) {
    const entries = listConfigurationSpecializedRenderers(dontStarveSettingsDefinition, section);
    const before = entries.filter((entry) => entry.placement === "before-fields");
    const after = entries.filter((entry) => (entry.placement ?? "after-fields") === "after-fields");
    assert.equal(before.length, 1);
    assert.equal(new Set([...before, ...after].map((entry) => entry.id)).size, entries.length);
    assert.equal(after.length, 0, "save import belongs to Maintenance, not world configuration");
    assert.equal(getDontStarvePresetNotice(defaults(), section, translate(en)), null);

    let settings = defaults();
    settings[section.startsWith("master") ? "master_settings_preset" : "caves_settings_preset"] = "custom_rules";
    const snapshot = structuredClone(settings);
    for (const catalog of [en, zh]) {
      const text = getDontStarvePresetNotice(settings, section, translate(catalog));
      assert.match(text, /custom_rules/u);
      assert.match(text, /inherit|沿用/u);
      assert.match(text, /Adjusting a control|调整控件/u);
      assert.doesNotMatch(text, /Advanced|高级/u);
    }
    assert.deepEqual(settings, snapshot);
    const rawKey = section.startsWith("master") ? "master_worldgenoverride_lua" : "caves_worldgenoverride_lua";
    settings = { ...settings, [rawKey]: "return { override_enabled = true, overrides = {} }" };
    assert.match(getDontStarvePresetNotice(settings, section, translate(en)), /form initially shows base defaults/u,
      "a plain raw table distinguishes displayed base defaults from the active preset values");
    settings = { ...settings, [rawKey]: "return make_world()" };
    assert.equal(getDontStarvePresetNotice(settings, section, translate(en)), null,
      "a dynamic script cannot claim that guided preset fields describe the active world");
  }
  assert.equal(zh["settings.schema.dontstarve.disable_data_collection.title"], "禁用数据收集");
  assert.match(zh["settings.schema.dontstarve.disable_data_collection.description"], /仅支持离线/u);
});
