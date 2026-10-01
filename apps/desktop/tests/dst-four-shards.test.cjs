const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("", module.filename);
const translate = (key, params, fallback) => (fallback ?? key)
  .replace(/\{(\w+)\}/g, (match, name) => params?.[name] ?? match);
const { DONTSTARVE_SHARDS: shards } = require("../src/views/settings/modules/dontstarve-shards.ts");
const { dontStarveSettingsDefinition: definition } = require("../src/views/settings/modules/dontstarve.ts");
const { parseGuidedSettingsSchema } = require("../src/views/settings/guided-settings.ts");
const { buildConfiguredEntries, buildConfigurableEntries, buildEnabledRows } = require("../src/views/servers/mod-workbench-model.ts");
const { buildDstModEnablementPlan, buildModSettingsApplyPlan, buildModSettingsRemovePlan } = require("../src/views/servers/mod-workbench-plans.ts");
const { hasDstRawModOverrides } = require("../src/views/servers/mod-workbench-dst-policy.ts");
const id = "1467214795", dependency = "3435352667";
const context = { locale: "en-US", t: translate };
const schema = fs.readFileSync(path.resolve(__dirname, "../../../modules/dontstarve/schema.json"), "utf8");
const fields = parseGuidedSettingsSchema({ summary: { id: "dontstarve", name: "DST" }, schema_json: schema },
  context.locale, context.t).fields;

test("four-shard Mod ownership includes Islands-only enablement and Volcano-only saved options", () => {
  const settings = { shard_layout: "island_adventures", islands_enabled_workshop_mod_ids: id,
    volcano_mod_configuration_options: { [dependency]: { devmode: false } } };
  const entries = buildConfiguredEntries("dontstarve", settings);
  const rows = buildEnabledRows(buildConfigurableEntries("dontstarve", entries));
  assert.deepEqual(rows.map((row) => [row.id, row.entry.key]), [[id, "dst-enabled"], [dependency, "dst-disabled"]]);
});

test("four-shard enable, download and removal plans preserve independent options and unrelated settings", () => {
  const settings = { shard_layout: "island_adventures", shared_workshop_mod_ids: id, cluster_name: "Keep",
    islands_mod_configuration_options: { [id]: { difficulty: 2 } }, volcano_mod_configuration_options: { [id]: { difficulty: 3 } } };
  const before = structuredClone(settings);
  const enabled = buildDstModEnablementPlan(settings, [id], true);
  for (const shard of shards) assert.equal(enabled[`${shard}_enabled_workshop_mod_ids`], id);
  const disabled = buildDstModEnablementPlan(enabled, [id], false);
  for (const shard of shards) assert.equal(disabled[`${shard}_enabled_workshop_mod_ids`], "");
  const item = { id: dependency, status: "resolved", item_kind: "item", consumer_app_id: 322330, children: [] };
  const added = buildModSettingsApplyPlan("dontstarve", enabled, [dependency], { [dependency]: item }, 322330).nextSettings;
  for (const shard of shards) assert.equal(added[`${shard}_enabled_workshop_mod_ids`], `${id}\n${dependency}`);
  const removed = buildModSettingsRemovePlan("dontstarve", added, [id], {}, 322330, null).nextSettings;
  for (const shard of shards) assert.equal(removed[`${shard}_enabled_workshop_mod_ids`], dependency);
  for (const next of [enabled, disabled, added, removed]) {
    assert.deepEqual(next.islands_mod_configuration_options, settings.islands_mod_configuration_options);
    assert.deepEqual(next.volcano_mod_configuration_options, settings.volcano_mod_configuration_options);
    assert.equal(next.cluster_name, "Keep");
  }
  assert.deepEqual(settings, before);
});

test("standard synchronization preserves inactive Island Adventures settings", () => {
  const settings = { shard_layout: "standard", shared_workshop_mod_ids: id,
    islands_enabled_workshop_mod_ids: dependency, volcano_mod_configuration_options: { [id]: { difficulty: 3 } },
    volcano_modoverrides_lua: "return make_volcano_mods()" };
  assert.equal(hasDstRawModOverrides(settings), false);
  const next = buildDstModEnablementPlan(settings, [id], true);
  assert.equal(next.master_enabled_workshop_mod_ids, id);
  assert.equal(next.caves_enabled_workshop_mod_ids, id);
  assert.equal(next.islands_enabled_workshop_mod_ids, dependency);
  assert.deepEqual(next.volcano_mod_configuration_options, settings.volcano_mod_configuration_options);
  assert.equal(next.volcano_modoverrides_lua, settings.volcano_modoverrides_lua);
  assert.equal(hasDstRawModOverrides({ ...settings, shard_layout: "island_adventures" }), true);
});

test("settings expose layout and expert world files while four-shard Mod management belongs to Mods", () => {
  const keys = new Set(fields.map((field) => field.key));
  assert.ok(keys.has("shard_layout"));
  for (const shard of shards) {
    assert.equal(keys.has(`${shard}_enabled_workshop_mod_ids`), false);
    assert.equal(keys.has(`${shard}_mod_configuration_options`), false);
    assert.ok(keys.has(`${shard}_modoverrides_lua`));
  }
  for (const shard of ["islands", "volcano"]) {
    const field = fields.find((field) => field.key === `${shard}_worldgenoverride_lua`);
    assert.equal(field.sectionId, "advanced");
    assert.equal(definition.isFieldDisabled(field, { shard_layout: "standard" }), true);
    assert.equal(definition.isFieldDisabled(field, { shard_layout: "island_adventures" }), false);
  }
  const caveField = fields.find((field) => field.key === "caves_worldgenoverride_lua");
  assert.equal(definition.isFieldDisabled(caveField, { shard_layout: "island_adventures", enable_caves: false }), false);
  assert.equal(definition.getEnumOptionLabel("shard_layout", "island_adventures", context.locale, translate),
    "Island Adventures · four shards");
});

test("four-shard validation checks Islands, Volcano and required Caves without validating inactive extra shards", () => {
  for (const shard of ["caves", "islands", "volcano"]) {
    const settings = { shard_layout: "island_adventures", enable_caves: false,
      [`${shard}_enabled_workshop_mod_ids`]: id, [`${shard}_modoverrides_lua`]: "return make_mods()" };
    assert.ok(definition.getSettingsValidationIssues(settings, context)
      .some((issue) => issue.fieldKey === `${shard}_modoverrides_lua`));
  }
  const inactive = { shard_layout: "standard", islands_mod_configuration_options: { [id]: { invalid: {} } } };
  assert.deepEqual(definition.getSettingsValidationIssues(inactive, context), []);
});

function panelHarness() {
  const filename = path.resolve(__dirname, "../src/views/servers/DstModConfigPanel.tsx");
  const loaded = new Module(filename, module);
  loaded.filename = filename;
  const requireFromFile = Module.createRequire(filename);
  loaded.require = (name) => name === "react" ? { ...React, useId: () => "four-shards", useMemo: (read) => read() }
    : name === "../../i18n" ? { useI18n: () => ({ t: translate }) } : requireFromFile(name);
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  return loaded.exports.DstModConfigPanel;
}
function nodes(node) {
  return React.isValidElement(node) ? [node, ...React.Children.toArray(node.props.children).flatMap(nodes)] : [];
}

test("editing all four shards synchronizes the selected Mod and preserves other Mod options", () => {
  const Panel = panelHarness();
  const settings = { shard_layout: "island_adventures", shared_workshop_mod_ids: id };
  for (const [index, shard] of shards.entries()) {
    settings[`${shard}_enabled_workshop_mod_ids`] = id;
    settings[`${shard}_mod_configuration_options`] = { [id]: { difficulty: index + 2 }, [dependency]: { retained: false } };
  }
  let updated;
  const rendered = Panel({ settings, selectedModId: id, loadingSpecs: false,
    configurationSpecs: [{ mod_id: id, status: "loaded", options: [{ name: "difficulty", label: "Difficulty",
      default_value: { kind: "number", value: 1 }, options: [] }] }], onSettingsChange(value) { updated = value; } });
  const option = nodes(rendered).find((node) => node.props.option?.name === "difficulty");
  option.props.onNumberChange("8");
  for (const shard of shards) assert.deepEqual(updated[`${shard}_mod_configuration_options`],
    { [id]: { difficulty: 8 }, [dependency]: { retained: false } });
  const reset = nodes(rendered).find((node) => node.type === "button" && node.props.children === "Restore all defaults");
  reset.props.onClick();
  for (const shard of shards) assert.deepEqual(updated[`${shard}_mod_configuration_options`], { [dependency]: { retained: false } });
  assert.equal(settings.volcano_mod_configuration_options[id].difficulty, 5);
});

test("imported source Lua reports literal native enablement and read-only options without rewriting nested values", () => {
  const raw = `return { ["workshop-${id}"] = { enabled = true, configuration_options = {
    difficulty = 8, toggle = false, nested = { keep = "original" } } }, ["workshop-${dependency}"] = { enabled = false } }`;
  const settings = { shard_layout: "island_adventures", shared_workshop_mod_ids: `${id}\n${dependency}`,
    islands_modoverrides_lua: raw };
  const snapshot = structuredClone(settings);
  const rows = buildEnabledRows(buildConfigurableEntries("dontstarve", buildConfiguredEntries("dontstarve", settings)));
  assert.deepEqual(rows.map((row) => [row.id, row.entry.key]), [[id, "dst-enabled"], [dependency, "dst-disabled"]]);
  const Panel = panelHarness();
  const projected = { master_modoverrides_lua: raw, caves_modoverrides_lua: raw };
  const rendered = Panel({ settings: projected, selectedModId: id, loadingSpecs: false,
    configurationSpecs: [{ mod_id: id, status: "loaded", options: [{ name: "difficulty", label: "Difficulty",
      default_value: { kind: "number", value: 1 }, options: [] }] }],
    onSettingsChange() { assert.fail("source Lua must remain authoritative"); } });
  const option = nodes(rendered).find((node) => node.props.option?.name === "difficulty");
  assert.equal(option.props.explicitValue, 8);
  assert.equal(option.props.editable, false);
  assert.equal(nodes(rendered).some((node) => node.props.children?.includes?.("This Mod is disabled")), false);
  assert.deepEqual(settings, snapshot);
});
