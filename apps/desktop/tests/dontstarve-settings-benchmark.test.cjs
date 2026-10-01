const assert = require("node:assert/strict");
const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const root = path.resolve(__dirname, "..", "..", "..");
const schema = JSON.parse(fs.readFileSync(path.join(root, "modules", "dontstarve", "schema.json"), "utf8"));
const inventorySource = fs.readFileSync(path.join(
  root,
  "crates",
  "app-storage",
  "src",
  "templates_render_dst_inventory.rs"
), "utf8");

function schemaInventory(sectionIds) {
  return Object.values(schema.properties)
    .filter((property) => sectionIds.has(property["x-lsgm-section"]) &&
      property["x-lsgm-source-key"].startsWith("overrides."))
    .map((property) => property["x-lsgm-source-key"].replace(/^overrides\./u, ""))
    .sort();
}

function rendererInventory(constantName) {
  const block = inventorySource.match(new RegExp(
    `const ${constantName}:[\\s\\S]*?= &\\[([\\s\\S]*?)\\n\\];`
  ));
  assert.ok(block, `${constantName} must exist`);
  return Array.from(
    block[1].matchAll(/\(\s*"[^"]+"\s*,\s*"([^"]+)"\s*,\s*"[^"]+"\s*,?\s*\)/gu),
    (match) => match[1]
  );
}

function digest(values) {
  return crypto.createHash("sha256").update([...values].sort().join("\n")).digest("hex");
}

function schemaDefinitionDigest(sectionIds) {
  const definitions = Object.entries(schema.properties)
    .filter(([, property]) => sectionIds.has(property["x-lsgm-section"]) &&
      property["x-lsgm-source-key"].startsWith("overrides."))
    .map(([key, property]) => ({
      key,
      sourceKey: property["x-lsgm-source-key"],
      type: property.type,
      default: property.default,
      enum: property.enum
    }))
    .sort((left, right) => left.key.localeCompare(right.key, "en"));
  return crypto.createHash("sha256").update(JSON.stringify(definitions)).digest("hex");
}

test("DST first-party world inventories remain exact at Master 190 and Caves 86", () => {
  const master = schemaInventory(new Set(["mastergen", "mastersettings"]));
  const caves = schemaInventory(new Set(["cavesgen", "cavessettings"]));

  assert.equal(master.length, 190);
  assert.equal(caves.length, 86);
  assert.equal(digest(master), "3bde26ac863fdade168e43ae46e6d71959ed1af64205ba09c0d3aa20db8f4c44");
  assert.equal(digest(caves), "2692c43317cb32a15dabd50bcf30046aaf420173a96ec5608994bcf0fd0531a0");
  assert.equal(
    schemaDefinitionDigest(new Set(["mastergen", "mastersettings"])),
    "b980e63db29914f8e0029d16a03d733b2c530b7fb80372e080e6eb94ad94e0d4"
  );
  assert.equal(
    schemaDefinitionDigest(new Set(["cavesgen", "cavessettings"])),
    "bf6fc99253f213acf6a5a1d471c1ad7427e07c2451e69f56019070896345059e"
  );

  const renderedMaster = [
    ...rendererInventory("DST_SHARED_OVERRIDE_ENTRIES"),
    ...rendererInventory("DST_MASTER_OVERRIDE_ENTRIES")
  ];
  const renderedCaves = rendererInventory("DST_CAVES_OVERRIDE_ENTRIES");
  assert.deepEqual([...renderedMaster].sort(), master);
  assert.deepEqual([...renderedCaves].sort(), caves);
  assert.equal(new Set(renderedMaster).size, 190);
  assert.equal(new Set(renderedCaves).size, 86);
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
