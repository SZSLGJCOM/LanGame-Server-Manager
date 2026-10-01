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
    arksurvivalevolved: { schema: 374, configuration: 368, guided: 367, structured: 18, maintenance: ["auto_save_period_minutes", "max_num_of_save_backups"] },
    arksurvivalascended: { schema: 274, configuration: 269, guided: 268, structured: 16, maintenance: ["auto_save_period_minutes"] }
  };

  for (const [moduleId, expected] of Object.entries(expectations)) {
    const parsed = parseGuidedSettingsSchema({
      summary: { id: moduleId, name: moduleId },
      schema_json: JSON.stringify(schema(moduleId))
    }, "en-US", translate);
    const model = buildConfigurationWorkspaceModel(parsed);
    const configurationItems = model.items.filter((item) => item.owner === "configuration");
    assert.deepEqual(model.items.filter((item) => item.owner === "maintenance").map((item) => item.fieldKey).sort(),
      [...expected.maintenance].sort(), `${moduleId} Maintenance ownership`);

    assert.equal(parsed.presentationFields.length, expected.schema, `${moduleId} presentation`);
    assert.equal(model.items.length, expected.schema, `${moduleId} model`);
    assert.equal(parsed.fields.length, expected.guided, `${moduleId} scalar field contracts`);
    const structured = configurationItems.filter((item) => item.state === "specialized");
    assert.equal(structured.length, expected.structured, `${moduleId} structured controls`);
    for (const item of structured) {
      assert.ok(resolveSettingsModuleDefinition(moduleId).specializedRenderers[item.field.presentation.rendererId],
        `${moduleId} ${item.fieldKey} requires an actual structured editor`);
    }
    assert.equal(configurationItems.length, expected.configuration, `${moduleId} Configuration ownership`);
    assert.equal(
      configurationItems.filter((item) => model.actionableSectionIds.includes(item.sectionId)).length,
      expected.configuration,
      `${moduleId} every Configuration field must have a reachable section`
    );

    const definition = resolveSettingsModuleDefinition(moduleId);
    const renderedKeys = parsed.sections.flatMap((section) => {
      const fields = parsed.fields.filter((field) => field.sectionId === section.id);
      const groups = definition.buildFieldGroups?.(section.id, fields, "en-US", translate) ?? [];
      return (groups.length > 0 ? groups.flatMap((group) => group.fields) : fields)
        .map((field) => field.key);
    });
    assert.equal(new Set(renderedKeys).size, expected.guided, `${moduleId} unique scalar field groups`);
    assert.deepEqual(
      [...renderedKeys].sort(),
      parsed.fields.map((field) => field.key).sort(),
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
