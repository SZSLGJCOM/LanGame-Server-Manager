const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const root = path.resolve(__dirname, "..", "..", "..");
const schema = JSON.parse(fs.readFileSync(path.join(root, "modules", "dontstarve", "schema.json"), "utf8"));
const nativeInventory = require("../../../modules/dontstarve/world-options.json");
const inventorySource = fs.readFileSync(path.join(
  root,
  "crates",
  "app-storage",
  "src",
  "templates_render_dst_inventory.rs"
), "utf8");

function rendererEntries(constantName) {
  const block = inventorySource.match(new RegExp(
    `const ${constantName}:[\\s\\S]*?= &\\[([\\s\\S]*?)\\n\\];`
  ));
  assert.ok(block, `${constantName} must exist`);
  return Array.from(
    block[1].matchAll(/\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*,\s*"([^"]+)"\s*,?\s*\)/gu),
    ([, settingKey, nativeKey, defaultValue]) => ({ settingKey, nativeKey, defaultValue })
  );
}

function rendererInventory(constantName) {
  return rendererEntries(constantName).map((entry) => entry.nativeKey);
}

test("DST world fields and renderer match the verified native inventory and defaults", () => {
  // Native ordinary options use "default"; these explicit exceptions preserve
  // the shipped world defaults independently of the schema and renderer.
  const defaults = {
    world_healthpenalty: "always", world_ghostenabled: "always", world_ghostsanitydrain: "always",
    world_portalresurection: "none", world_spawnmode: "fixed", world_basicresource_regrowth: "none",
    world_lessdamagetaken: "none", master_wanderingtrader_enabled: "always",
    master_ocean_seastack: "ocean_default", master_ocean_waterplant: "ocean_default",
    caves_start_location: "caves", caves_task_set: "cave_default", caves_acidrain_enabled: "always"
  };
  const rendered = {
    forest: [...rendererEntries("DST_SHARED_OVERRIDE_ENTRIES"), ...rendererEntries("DST_MASTER_OVERRIDE_ENTRIES")],
    cave: rendererEntries("DST_CAVES_OVERRIDE_ENTRIES")
  };
  const expectedRendered = { forest: [], cave: [] };
  for (const [section, category, location] of [
    ["mastergen", "worldgen", "forest"], ["mastersettings", "settings", "forest"],
    ["cavesgen", "worldgen", "cave"], ["cavessettings", "settings", "cave"]
  ]) {
    const expected = nativeInventory.options.filter((option) => option.category === category &&
      option.locations.includes(location) && (location === "forest" || !option.masterControlled));
    const fields = Object.entries(schema.properties).filter(([, property]) =>
      property["x-lsgm-section"] === section && property["x-lsgm-source-key"]?.startsWith("overrides."));
    const nativeKey = (property) => property["x-lsgm-source-key"].slice("overrides.".length);
    assert.deepEqual(fields.map(([, property]) => nativeKey(property)).sort(),
      expected.map((option) => option.key).sort(), `${section}: native option coverage`);
    for (const [settingKey, property] of fields) {
      const key = nativeKey(property);
      const option = expected.find((entry) => entry.key === key);
      assert.equal(property.type, "string", settingKey);
      assert.equal(property.default, defaults[settingKey] ?? "default", `${settingKey}: default`);
      // Native task/start registries have no stable enumeration order.
      const choices = ["task_set", "start_location"].includes(key)
        ? (values) => [...values].sort() : (values) => values;
      assert.deepEqual(choices(property.enum), choices(option.values[location]), `${settingKey}: enum`);
      assert.ok(property.enum.includes(property.default), `${settingKey}: default is selectable`);
      expectedRendered[location].push({ settingKey, nativeKey: key, defaultValue: property.default });
    }
  }
  const bySetting = (left, right) => left.settingKey.localeCompare(right.settingKey, "en");
  for (const location of ["forest", "cave"]) {
    assert.deepEqual(rendered[location].sort(bySetting), expectedRendered[location].sort(bySetting),
      `${location}: renderer binding and defaults`);
  }
});

test("Caves generation uses only the native cave task set and start location", () => {
  for (const [key, expected] of [["caves_task_set", "cave_default"], ["caves_start_location", "caves"]]) {
    assert.equal(schema.properties[key].default, expected, key);
    assert.deepEqual(schema.properties[key].enum, [expected], key);
  }
});

test("DST shard output excludes synthetic and master-controlled cave keys", () => {
  const master = new Set([
    ...rendererInventory("DST_SHARED_OVERRIDE_ENTRIES"),
    ...rendererInventory("DST_MASTER_OVERRIDE_ENTRIES")
  ]);
  const caves = new Set(rendererInventory("DST_CAVES_OVERRIDE_ENTRIES"));

  assert.equal(master.has("toadstool"), false, "toadstool is not a forest option in the validated package");
  for (const key of ["day", "beefaloheat", "krampus", "specialevent", "healthpenalty"]) {
    assert.equal(caves.has(key), false, `${key} is Master-controlled and must not be written by Caves`);
  }
  for (const key of ["ocean_otterdens", "balatro", "palmconetree", "mutated_bearger", "portal_spawnrate"]) {
    assert.equal(master.has(key), true, `${key} must be present in Master`);
  }
  for (const key of ["cavelight", "daywalker", "acidrain_enabled", "rifts_enabled_cave", "tree_rock_regrowth"]) {
    assert.equal(caves.has(key), true, `${key} must be present in Caves`);
  }
});

test("DST native backup launch integers preserve documented lower boundaries", () => {
  assert.deepEqual(
    {
      type: schema.properties.backup_log_count.type,
      default: schema.properties.backup_log_count.default,
      minimum: schema.properties.backup_log_count.minimum
    },
    { type: "integer", default: 100, minimum: 0 }
  );
  assert.deepEqual(
    {
      type: schema.properties.backup_log_period.type,
      default: schema.properties.backup_log_period.default,
      minimum: schema.properties.backup_log_period.minimum
    },
    { type: "integer", default: 86400, minimum: 1 }
  );
});

test("DST mock defaults use the same canonical preset grammar as the real schema", () => {
  const mockSettings = fs.readFileSync(path.join(
    root,
    "apps",
    "desktop",
    "src",
    "api-mock",
    "module-settings.ts"
  ), "utf8");
  const masterDefault = schema.properties.master_worldgenoverride_lua.default;
  const cavesDefault = schema.properties.caves_worldgenoverride_lua.default;

  assert.match(masterDefault, /settings_preset = "SURVIVAL_TOGETHER"/u);
  assert.match(masterDefault, /worldgen_preset = "SURVIVAL_TOGETHER"/u);
  assert.match(cavesDefault, /settings_preset = "DST_CAVE"/u);
  assert.match(cavesDefault, /worldgen_preset = "DST_CAVE"/u);
  assert.doesNotMatch(mockSettings, /\\n  preset = \\"(?:SURVIVAL_TOGETHER|DST_CAVE)\\"/u);
});

test("DST Mod management and option editing share the external Mods workspace", () => {
  const workbench = ["ModWorkbench.tsx", "mod-workbench-model.ts", "mod-workbench-plans.ts"]
    .map((fileName) => fs.readFileSync(path.join(
      root,
      "apps",
      "desktop",
      "src",
      "views",
      "servers",
      fileName
    ), "utf8"))
    .join("\n");
  const registry = fs.readFileSync(path.join(
    root,
    "apps",
    "desktop",
    "src",
    "views",
    "settings",
    "module-registry.ts"
  ), "utf8");

  assert.doesNotMatch(workbench, /DstModConfigPanel|readDontStarveModConfigurationSpecs|dstSpecs/u);
  assert.match(workbench, /<DstWorkshopConfiguration/u);
  assert.match(registry, /"master_mod_configuration_options"/u);
  assert.match(registry, /"caves_mod_configuration_options"/u);
  assert.match(registry, /"mod-workbench-dst-mods", "mods"/u);
  assert.doesNotMatch(registry, /DstModConfigurationRenderer|dst-master-mod-configuration|dst-caves-mod-configuration/u);
});

test("DST Mod options keep native implementation evidence out of the player editor", () => {
  const renderer = fs.readFileSync(path.join(
    root,
    "apps",
    "desktop",
    "src",
    "views",
    "servers",
    "DstWorkshopConfiguration.tsx"
  ), "utf8");
  const panel = fs.readFileSync(path.join(
    root,
    "apps",
    "desktop",
    "src",
    "views",
    "servers",
    "DstModConfigPanel.tsx"
  ), "utf8");
  const optionField = fs.readFileSync(path.join(
    root,
    "apps",
    "desktop",
    "src",
    "views",
    "servers",
    "DstModOptionField.tsx"
  ), "utf8");

  assert.doesNotMatch(renderer, /Native modoverrides\.lua|modConfiguration\.eyebrow/u);
  assert.doesNotMatch(renderer, /contract\.description/u);
  assert.doesNotMatch(panel, /dst-mod-spec-meta/u);
  assert.doesNotMatch(panel, /options from modinfo\.lua/u);
  assert.doesNotMatch(panel, /\btitle=\{/u);
  assert.match(optionField, /useConfigurationFieldHelp/u);
  assert.match(optionField, /aria-describedby/u);
});
