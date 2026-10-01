const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("", module.filename);

const { parseGuidedSettingsSchema } = require("../src/views/settings/guided-settings.ts");
const { resolveSettingsModuleDefinition } = require("../src/views/settings/module-registry.ts");
const { validateGuidedSettingsObject } = require("../src/views/settings/guided-settings.ts");

const repositoryRoot = path.resolve(__dirname, "..", "..", "..");
const moduleRoot = path.join(repositoryRoot, "modules", "barotrauma");

function attributeNames(fragment) {
  return [...fragment.matchAll(/\s([A-Za-z][A-Za-z0-9]*)="[^"]*"/g)]
    .map((match) => match[1]);
}

function referenceInventory() {
  const xml = fs.readFileSync(
    path.join(moduleRoot, "reference-configs", "serversettings-v1.13.4.0.xml"),
    "utf8"
  );
  const rootTag = xml.match(/<serversettings\b[\s\S]*?>/)?.[0];
  const campaignTag = xml.match(/<campaignsettings\b[\s\S]*?\/>/)?.[0];
  assert.ok(rootTag, "missing exact-build serversettings root");
  assert.ok(campaignTag, "missing exact-build campaignsettings child");
  return {
    root: attributeNames(rootTag),
    campaign: attributeNames(campaignTag)
  };
}

function ledgerEntries(toml, tableName) {
  const blocks = toml.split(`[[${tableName}]]`).slice(1);
  return blocks.map((block) => ({
    source: block.match(/^source\s*=\s*"([^"]+)"/m)?.[1],
    key: block.match(/^key\s*=\s*"([^"]+)"/m)?.[1],
    schemaKey: block.match(/^schema_key\s*=\s*"([^"]+)"/m)?.[1]
  }));
}

test("Barotrauma exact-build XML keys are each schema-backed or precisely excluded", () => {
  const inventory = referenceInventory();
  assert.equal(inventory.root.length, 106);
  assert.equal(inventory.campaign.length, 18);

  const schema = JSON.parse(fs.readFileSync(path.join(moduleRoot, "schema.json"), "utf8"));
  const mappings = new Map();
  for (const [schemaKey, property] of Object.entries(schema.properties ?? {})) {
    if (property["x-lsgm-source"] === "server_settings") {
      mappings.set(property["x-lsgm-source-key"], schemaKey);
    }
    if (property["x-lsgm-source"] === "barotrauma_campaign_settings_xml") {
      mappings.set(property["x-lsgm-source-key"], schemaKey);
    }
  }

  const ledger = fs.readFileSync(path.join(moduleRoot, "config-sources.toml"), "utf8");
  const exclusions = new Set(
    ledgerEntries(ledger, "exclusions")
      .filter((entry) => entry.source === "server_settings")
      .map((entry) => entry.key)
  );
  const expected = new Set([
    ...inventory.root.map((key) => `serversettings.${key}`),
    ...inventory.campaign.map((key) => `campaignsettings.${key}`)
  ]);
  assert.deepEqual(
    new Set([...mappings.keys(), ...exclusions].filter((key) => expected.has(key))),
    expected
  );
  assert.equal(new Set(mappings.values()).size, mappings.size);
});

test("Barotrauma production template renders every exact-build XML attribute once", () => {
  const inventory = referenceInventory();
  const template = fs.readFileSync(
    path.join(moduleRoot, "templates", "serversettings.xml.hbs"),
    "utf8"
  );
  const rootTag = template.match(/<serversettings\b[\s\S]*?>/)?.[0];
  const campaignTag = template.match(/<campaignsettings\b[\s\S]*?\/>/)?.[0];
  assert.ok(rootTag);
  assert.ok(campaignTag);
  assert.deepEqual(attributeNames(rootTag), inventory.root);
  assert.deepEqual(attributeNames(campaignTag), inventory.campaign);
  assert.match(rootTag, /\sAutoRestart="/);
  assert.doesNotMatch(rootTag, /\sautorestart="/);
});

test("Barotrauma fractional settings retain native float types and closed enums", () => {
  const { properties } = JSON.parse(fs.readFileSync(path.join(moduleRoot, "schema.json"), "utf8"));
  for (const key of ["respawn_interval", "traitor_probability", "pvp_stun_resist",
    "selected_level_difficulty", "new_campaign_default_salary", "max_auto_ban_time",
    "campaign_crew_vitality_multiplier", "campaign_oxygen_multiplier", "campaign_repair_fail_multiplier"]) {
    assert.equal(properties[key].type, "number", `${key} must accept fractional native values`);
  }
  assert.deepEqual(properties.respawn_mode.enum, ["None", "MidRound", "BetweenRounds", "Permadeath"]);
  assert.deepEqual(properties.campaign_world_hostility.enum, ["Low", "Medium", "High", "Hellish"]);
  assert.deepEqual(properties.los_mode.enum, ["None", "Transparent", "Opaque"]);
  assert.equal(properties.traitor_probability.maximum, 1);
  assert.equal(properties.max_lag_compensation.maximum, 500);
});

function guidedSchema(locale) {
  return parseGuidedSettingsSchema({
    summary: { id: "barotrauma", name: "Barotrauma" },
    schema_json: fs.readFileSync(path.join(moduleRoot, "schema.json"), "utf8")
  }, locale, (key, _params, fallback) => fallback ?? key);
}

test("Barotrauma fields have semantic copy and a single explicit configuration owner", () => {
  const properties = JSON.parse(fs.readFileSync(path.join(moduleRoot, "schema.json"), "utf8")).properties;
  const definition = resolveSettingsModuleDefinition("barotrauma");
  for (const locale of ["en-US", "zh-CN"]) {
    const parsed = guidedSchema(locale);
    assert.equal(parsed.parseError, null);
    const byKey = new Map(parsed.fields.map((field) => [field.key, field]));
    for (const key of Object.keys(properties)) {
      const copy = definition.getFieldCopy(key, () => "stale generated text", locale);
      assert.ok(copy?.title && copy?.description, `${locale}: ${key} lacks reviewed copy`);
      assert.doesNotMatch(copy.description, /pinned exact-build|Native Barotrauma|Written to|作为 .*写入|工作流分组/);
      if (locale === "zh-CN") assert.match(copy.title, /[\u3400-\u9fff]/, key);
    }
    assert.ok(!byKey.has("admin_entries"), "administrator roster belongs to player management");
    assert.ok(!byKey.has("mod_workshop_ids"), "content roster belongs to Mods");
    assert.equal(byKey.get("auto_restart").sectionId, "round");
    assert.equal(byKey.get("save_server_logs").sectionId, "runtime");
    assert.equal(byKey.get("allow_remote_campaign_interactions").sectionId, "campaign");
    assert.equal(byKey.get("event_removal_time").sectionId, "network");
    assert.equal(byKey.get("min_respawn_ratio").sectionId, "round");
    const grouped = [];
    for (const section of parsed.sections) {
      const fields = parsed.fields.filter((field) => field.sectionId === section.id);
      const groups = definition.buildFieldGroups(section.id, fields, locale, (_key, _params, fallback) => fallback ?? "");
      assert.ok(groups.every((group) => group.id !== "additional"), `${locale}: ${section.id} has unclassified fields`);
      grouped.push(...groups.flatMap((group) => group.fields.map((field) => field.key)));
    }
    assert.equal(grouped.length, byKey.size);
    assert.equal(new Set(grouped).size, byKey.size, "each editable field is grouped once");
    const respawn = byKey.get("respawn_mode");
    assert.equal(respawn.control, "select");
    if (locale === "zh-CN") assert.ok(respawn.enumOptions.every((option) => /[\u3400-\u9fff]/.test(option.label)));
  }
});

test("Barotrauma form accepts fractional native values and rejects invalid ranges", () => {
  const parsed = guidedSchema("zh-CN");
  const fractionalValues = {
    respawn_interval: 30.5,
    traitor_probability: 0.25,
    pvp_stun_resist: 0.35,
    selected_level_difficulty: 35.5,
    new_campaign_default_salary: 10.5,
    campaign_oxygen_multiplier: 1.2,
    campaign_crew_vitality_multiplier: 3.25,
    campaign_repair_fail_multiplier: 1.5,
    max_transport_time: 0
  };
  const reviewedKeys = new Set(Object.keys(fractionalValues));
  assert.deepEqual(validateGuidedSettingsObject(parsed, fractionalValues)
    .filter((issue) => reviewedKeys.has(issue.fieldKey)), []);
  const invalid = validateGuidedSettingsObject(parsed, {
    traitor_probability: 2,
    max_lag_compensation: 501,
    campaign_max_mission_count: 11
  });
  for (const key of ["traitor_probability", "max_lag_compensation", "campaign_max_mission_count"]) {
    assert.ok(invalid.some((issue) => issue.fieldKey === key), key);
  }
});
