const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function compileTypeScript(module, filename) {
  const source = fs.readFileSync(filename, "utf8");
  module._compile(transpileTypeScript(source, filename), filename);
}

require.extensions[".ts"] = compileTypeScript;
require.extensions[".tsx"] = compileTypeScript;
require.extensions[".css"] = function compileEmptyCss(module) {
  module._compile("module.exports = {};", module.filename);
};

const repositoryRoot = path.resolve(__dirname, "..", "..", "..");
const desktopRoot = path.join(repositoryRoot, "apps", "desktop");
const modulesRoot = path.join(repositoryRoot, "modules");
const { resolveConfigurationFieldPresentation } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "configuration-presentation.ts"
));
const { resolveSettingsModuleDefinition } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "module-registry.ts"
));
const { parseGuidedSettingsSchema } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "guided-settings.ts"
));
const { buildConfigurationWorkspaceModel } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "configuration-workspace-model.ts"
));

const translate = (_key, _params, fallback) => fallback ?? "";
const expectedParentIds = [
  "ark-cluster-transfer",
  "ark-world-gameplay",
  "ark-rates-progression",
  "ark-building-resources",
  "ark-content-rules"
];

function schema(moduleId) {
  return JSON.parse(fs.readFileSync(path.join(modulesRoot, moduleId, "schema.json"), "utf8"));
}

function presentation(moduleId, fieldKey) {
  const definition = resolveSettingsModuleDefinition(moduleId);
  return resolveConfigurationFieldPresentation(
    fieldKey,
    schema(moduleId).properties[fieldKey],
    definition
  );
}

test("ASE and ASA retain gameplay domains while shared administration and runtime own their native subgroups", () => {
  for (const moduleId of ["arksurvivalevolved", "arksurvivalascended"]) {
    const sections = resolveSettingsModuleDefinition(moduleId).getSections(translate, "en-US");
    const parents = sections.filter((section) => section.parentId === undefined);
    const children = sections.filter((section) => section.parentId !== undefined);

    assert.deepEqual(parents.map((section) => section.id), expectedParentIds, moduleId);
    assert.deepEqual(parents.map((section) => section.order), [0, 100, 200, 300, 400], moduleId);
    assert.ok(children.length >= 20, `${moduleId} must retain its detailed native sections`);
    assert.ok(children.every((section) => [...expectedParentIds, "access", "runtime"].includes(section.parentId)), moduleId);
    assert.equal(sections.some((section) => section.id === "session"), false, moduleId);
    assert.equal(new Set(sections.map((section) => section.id)).size, sections.length, moduleId);
  }
});

test("ARK configuration remains reachable while save policies belong to Maintenance", () => {
  const expectations = {
    arksurvivalevolved: {
      maintenance: ["auto_save_period_minutes", "max_num_of_save_backups"],
      mods: ["active_mod_ids", "ark_mod_map_overrides"],
      structured: ["player_base_stat_multipliers_attribute", "override_engram_entries"]
    },
    arksurvivalascended: {
      maintenance: ["auto_save_period_minutes"],
      mods: ["mod_ids_csv", "asa_mod_configurations", "ark_mod_map_overrides"],
      structured: []
    }
  };
  const sharedStructuredKeys = [
    "additional_maps",
    "per_level_stats_multiplier_player_integer",
    "per_level_stats_multiplier_dino_wild_integer",
    "per_level_stats_multiplier_dino_tamed_type_integer",
    "level_experience_ramp_overrides",
    "override_player_level_engram_points",
    "engram_entry_auto_unlocks",
    "override_named_engram_entries",
    "npc_replacements",
    "dino_spawn_weight_multipliers",
    "config_add_npc_spawn_entries_container",
    "config_subtract_npc_spawn_entries_container",
    "config_override_npc_spawn_entries_container",
    "config_override_supply_crate_items",
    "config_override_item_crafting_costs",
    "config_override_item_max_quantity"
  ];

  for (const [moduleId, expected] of Object.entries(expectations)) {
    const currentSchema = schema(moduleId);
    const schemaKeys = Object.keys(currentSchema.properties).sort();
    const parsed = parseGuidedSettingsSchema({
      summary: { id: moduleId, name: moduleId },
      schema_json: JSON.stringify(currentSchema)
    }, "en-US", translate);
    assert.equal(parsed.parseError, null, `${moduleId} schema must parse`);
    const model = buildConfigurationWorkspaceModel(parsed);
    const configurationItems = model.items.filter((item) => item.owner === "configuration");
    const workspaceKeys = {
      maintenance: expected.maintenance,
      mods: expected.mods,
      player_access: ["exclusive_join_list", "priority_join_list", "admin_account_ids"]
    };
    for (const [owner, keys] of Object.entries(workspaceKeys)) {
      assert.deepEqual(model.items.filter((item) => item.owner === owner).map((item) => item.fieldKey).sort(),
        [...keys].sort(), `${moduleId} ${owner} ownership`);
    }
    const separatelyOwnedKeys = new Set(Object.values(workspaceKeys).flat());
    const expectedConfigurationKeys = schemaKeys.filter((key) => !separatelyOwnedKeys.has(key));
    const expectedGuidedKeys = expectedConfigurationKeys.filter((key) =>
      !["object", "array"].includes(currentSchema.properties[key].type));

    assert.deepEqual(parsed.presentationFields.map((field) => field.key).sort(), schemaKeys, `${moduleId} presentation`);
    assert.deepEqual(model.items.map((item) => item.fieldKey).sort(), schemaKeys, `${moduleId} model`);
    assert.deepEqual(parsed.fields.map((field) => field.key).sort(), expectedGuidedKeys, `${moduleId} scalar field contracts`);
    const structured = configurationItems.filter((item) => item.state === "specialized");
    assert.deepEqual(structured.map((item) => item.fieldKey).sort(),
      [...sharedStructuredKeys, ...expected.structured].sort(), `${moduleId} structured controls`);
    const definition = resolveSettingsModuleDefinition(moduleId);
    for (const item of structured) {
      const renderer = definition.specializedRenderers[item.field.presentation.rendererId];
      assert.equal(renderer?.kind, "module-addon", `${moduleId} ${item.fieldKey} requires a structured editor`);
      assert.equal(typeof renderer.Renderer, "function",
        `${moduleId} ${item.fieldKey} requires an actual structured editor`);
    }
    assert.deepEqual(configurationItems.map((item) => item.fieldKey).sort(),
      expectedConfigurationKeys, `${moduleId} Configuration ownership`);
    assert.deepEqual(
      configurationItems.filter((item) => model.actionableSectionIds.includes(item.sectionId))
        .map((item) => item.fieldKey).sort(),
      expectedConfigurationKeys,
      `${moduleId} every Configuration field must have a reachable section`
    );

    const renderedKeys = parsed.sections.flatMap((section) => {
      const fields = parsed.fields.filter((field) => field.sectionId === section.id);
      const groups = definition.buildFieldGroups?.(section.id, fields, "en-US", translate) ?? [];
      return (groups.length > 0 ? groups.flatMap((group) => group.fields) : fields)
        .map((field) => field.key);
    });
    assert.deepEqual(
      [...renderedKeys].sort(),
      expectedGuidedKeys,
      `${moduleId} field grouping must neither drop nor duplicate controls`
    );
    const mapField = configurationItems.find((item) => item.fieldKey === "additional_maps");
    assert.ok(mapField, `${moduleId} map topology must belong to Configuration`);
    assert.equal(mapField.sectionId, "transfer");
    assert.equal(mapField.field.presentation.rendererId, "ark-cluster-maps");
    const mapRenderer = definition.specializedRenderers["ark-cluster-maps"];
    assert.equal(mapRenderer.kind, "module-addon");
    assert.equal(mapRenderer.fieldKey, "additional_maps");
    assert.equal(typeof mapRenderer.Renderer, "function");
    assert.equal(parsed.fields.some((field) => field.key === "additional_maps"), false,
      `${moduleId} map arrays must not fall back to a scalar text field`);
  }
});

test("ARK edition-specific mod fields are isolated and have one operational owner", () => {
  const aseProperties = schema("arksurvivalevolved").properties;
  const asaProperties = schema("arksurvivalascended").properties;

  assert.ok(aseProperties.auto_managed_mod_ids);
  assert.equal(aseProperties.passive_mod_ids_csv, undefined);
  assert.ok(asaProperties.passive_mod_ids_csv);
  assert.equal(asaProperties.auto_managed_mod_ids, undefined);

  assert.deepEqual(
    { owner: presentation("arksurvivalevolved", "active_mod_ids").owner,
      state: presentation("arksurvivalevolved", "active_mod_ids").state },
    { owner: "mods", state: "specialized" }
  );
  assert.deepEqual(
    { owner: presentation("arksurvivalevolved", "auto_managed_mods").owner,
      state: presentation("arksurvivalevolved", "auto_managed_mods").state },
    { owner: "configuration", state: "editable" }
  );
  assert.deepEqual(
    { owner: presentation("arksurvivalevolved", "auto_managed_mod_ids").owner,
      state: presentation("arksurvivalevolved", "auto_managed_mod_ids").state },
    { owner: "configuration", state: "editable" }
  );
  assert.deepEqual(
    { owner: presentation("arksurvivalascended", "mod_ids_csv").owner,
      state: presentation("arksurvivalascended", "mod_ids_csv").state },
    { owner: "mods", state: "specialized" }
  );
  assert.equal(asaProperties.asa_mod_configurations.type, "object");
  assert.deepEqual(
    { owner: presentation("arksurvivalascended", "asa_mod_configurations").owner,
      state: presentation("arksurvivalascended", "asa_mod_configurations").state },
    { owner: "mods", state: "specialized" }
  );
  assert.deepEqual(
    { owner: presentation("arksurvivalascended", "passive_mod_ids_csv").owner,
      state: presentation("arksurvivalascended", "passive_mod_ids_csv").state },
    { owner: "configuration", state: "editable" }
  );
});

test("ARK edition contracts retain native value types and evidence-backed ranges", () => {
  const aseProperties = schema("arksurvivalevolved").properties;
  const asaProperties = schema("arksurvivalascended").properties;

  for (const properties of [aseProperties, asaProperties]) {
    assert.equal(properties.active_event.type, "string");
    assert.equal(properties.active_event.default, "");
    assert.equal(properties.battleye_enabled.type, "boolean");
    assert.equal(properties.battleye_enabled.default, true);
  }

  assert.deepEqual(
    {
      type: aseProperties.auto_managed_mod_ids.type,
      format: aseProperties.auto_managed_mod_ids.format,
      surface: aseProperties.auto_managed_mod_ids["x-lsgm-source-surface"]
    },
    { type: "string", format: "textarea", surface: "materializer" }
  );
  assert.equal(asaProperties.passive_mod_ids_csv.format, "textarea");

  for (const key of ["fishing_loot_quality_multiplier", "supply_crate_loot_quality_multiplier"]) {
    assert.equal(aseProperties[key].minimum, 1, key);
    assert.equal(aseProperties[key].maximum, 5, key);
  }
  assert.equal(asaProperties.fishing_loot_quality_multiplier, undefined,
    "Fishing remains outside the ASA editor until its native INI input contract is independently confirmed");
  assert.equal(asaProperties.supply_crate_loot_quality_multiplier.type, "number");
  assert.equal(asaProperties.supply_crate_loot_quality_multiplier["x-lsgm-source"], "asa_advanced_game_ini");
  assert.equal(asaProperties.supply_crate_loot_quality_multiplier.default, undefined);
  assert.equal(asaProperties.supply_crate_loot_quality_multiplier.minimum, undefined);
  assert.equal(asaProperties.supply_crate_loot_quality_multiplier.maximum, undefined);
});

test("ARK controls render project-authored bilingual help while independent help remains available", () => {
  const React = require("react");
  const { renderToStaticMarkup } = require("react-dom/server");
  const { ConfigurationField } = require(path.join(desktopRoot, "src/views/settings/ConfigurationField.tsx"));
  const { EN_US_MESSAGES } = require(path.join(desktopRoot, "src/i18n-messages.ts"));
  const { ZH_CN_MESSAGES } = require(path.join(desktopRoot, "src/i18n-messages-zh-cn.ts"));
  const authoredHelp = require(path.join(desktopRoot, "../../scripts/ark_field_descriptions.json"));
  const copy = { concealSecret: "Hide", revealSecret: "Show",
    restartScopes: { cluster: "Cluster", none: "None", server: "Server", world: "World" } };
  for (const [locale, catalog] of [["en-US", EN_US_MESSAGES], ["zh-CN", ZH_CN_MESSAGES]]) {
    const t = (key, params, fallback) => String(catalog[key] ?? fallback ?? key).replace(
      /\{\s*([\w.]+)\s*\}/g, (match, name) => params?.[name] == null ? match : String(params[name])
    );
    for (const [moduleId, key] of [["arksurvivalevolved", "crossplay"], ["arksurvivalascended", "always_tick_dedicated_skeletal_meshes"]]) {
      const current = schema(moduleId);
      const expected = authoredHelp[moduleId][key][locale === "en-US" ? "en" : locale];
      assert.equal(current.properties[key].description, authoredHelp[moduleId][key].en);
      const parsed = parseGuidedSettingsSchema({ summary: { id: moduleId, name: moduleId },
        schema_json: JSON.stringify(current) }, locale, t);
      const field = parsed.fields.find((entry) => entry.key === key);
      assert.ok(field?.title.trim(), `${moduleId}/${locale} retains its field label`);
      assert.equal(field.description, expected, `${moduleId}/${locale} retains meaningful authored help`);
      const html = renderToStaticMarkup(React.createElement(ConfigurationField, {
        copy, field, onPatch() {}, settings: { [key]: false }, t, value: false
      }));
      assert.match(html, /^<div class="configuration-field settings-schema-field"/);
      const inputs = [...html.matchAll(/<input\b[^>]*>/g)];
      assert.equal(inputs.length, 1, "the field renders exactly one native checkbox");
      const inputId = inputs[0][0].match(/\bid="([^"]+)"/)?.[1];
      assert.ok(inputId, "the checkbox has a stable accessible ID");
      const labels = [...html.matchAll(/<label\b[^>]*\bfor="([^"]+)"[^>]*>([\s\S]*?)<\/label>/g)];
      assert.equal(labels.length, 1, "the checkbox has exactly one explicit label");
      assert.equal(labels[0][1], inputId, "the label targets this checkbox");
      assert.ok(labels[0][2].includes(inputs[0][0]), "the toggle label contains its checkbox");
      const descriptionId = inputs[0][0].match(/\baria-describedby="([^"]+)"/)?.[1];
      assert.ok(descriptionId, "the checkbox references its authored help");
      assert.ok(html.includes(`id="${descriptionId}"`), "the referenced help node exists");
      assert.match(html, /type="checkbox"/);
      assert.match(html, /role="tooltip"/);
      assert.match(html, /aria-describedby="[^"]+-description"/);
      assert.ok(html.includes(expected.split(/[。.]/u)[0]), "Rendered help must explain the setting");
      assert.doesNotMatch(html, /undefined|<span\b[^>]*>\s*<\/span>/);
      const cluster = parsed.fields.find((entry) => entry.key === "cluster_id");
      assert.ok(cluster?.description?.includes(locale === "zh-CN" ? "共享集群目录" : "shared cluster directory"),
        "Independent cluster help must survive provenance cleanup");
    }
  }
});
