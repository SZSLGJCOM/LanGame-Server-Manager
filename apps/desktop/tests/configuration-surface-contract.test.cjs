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
const { buildConfigurationWorkspaceModel, configurationNavigationRoots } = require("../src/views/settings/configuration-workspace-model.ts");
const { buildSavedConfigurationSchema } = require("../src/views/settings/saved-configuration-schema.ts");
const { listSettingsModuleIds, resolveSettingsModuleDefinition } = require("../src/views/settings/module-registry.ts");
const { resolveConfigurationRendererContract } = require("../src/views/settings/configuration-presentation.ts");
const { EN_US_MESSAGES } = require("../src/i18n-messages.ts");
const { ZH_CN_MESSAGES } = require("../src/i18n-messages-zh-cn.ts");

const modulesRoot = path.resolve(__dirname, "../../../modules");
const surfaces = ["general", "mods", "player_access", "maintenance"];
const surfaceByOwner = { configuration: "general", mods: "mods", player_access: "player_access", maintenance: "maintenance" };
const catalogs = [["en-US", EN_US_MESSAGES], ["zh-CN", ZH_CN_MESSAGES]];
const moduleIds = listSettingsModuleIds();
const sortedKeys = (fields) => fields.map((field) => field.key).sort();
const flatten = (nodes) => nodes.flatMap((node) => [node, ...flatten(node.children)]);

function scalarType(property) {
  const types = (Array.isArray(property.type) ? property.type : [property.type]).filter((type) => type !== "null");
  return types.length === 1 && ["string", "integer", "number", "boolean"].includes(types[0]) ? types[0] : null;
}

function assertGroupedControls(schema, definition, locale, t, readOnly, context) {
  for (const section of schema.sections) {
    const controls = schema.fields.filter((field) => field.sectionId === section.id &&
      (readOnly || !(field.presentation.state === "specialized" &&
        resolveConfigurationRendererContract(field.presentation.rendererId, definition)?.kind === "module-addon")));
    const groups = definition.buildFieldGroups(section.id, controls, locale, t);
    const rendered = groups.length ? groups.flatMap((group) => group.fields) : controls;
    assert.deepEqual(sortedKeys(rendered), sortedKeys(controls), `${context}.${section.id} must render each owned control once`);
    for (const field of rendered) assert.equal(field.sectionId, section.id, `${context}.${field.key} must keep its section`);
  }
}

function savedValue(property) {
  if (Object.hasOwn(property, "default")) return structuredClone(property.default);
  const type = scalarType(property);
  if (type === "boolean") return false;
  if (type === "number" || type === "integer") return 0;
  if (type === "string") return "";
  return property.type === "array" ? [] : {};
}

test("the surface matrix covers every bundled settings module", () => {
  assert.equal(moduleIds.length, 32);
  assert.deepEqual(moduleIds, fs.readdirSync(modulesRoot)
    .filter((id) => fs.existsSync(path.join(modulesRoot, id, "schema.json"))).sort());
});

for (const moduleId of moduleIds) for (const [locale, catalog] of catalogs) {
  test(`${moduleId} ${locale}: all settings surfaces and saved configuration preserve ownership`, () => {
    const t = (key, params, fallback) => String(catalog[key] ?? fallback ?? key)
      .replace(/\{\s*([\w.]+)\s*\}/g, (match, name) => params?.[name] ?? match);
    const details = { summary: { id: moduleId, name: moduleId },
      schema_json: fs.readFileSync(path.join(modulesRoot, moduleId, "schema.json"), "utf8") };
    const properties = JSON.parse(details.schema_json).properties;
    const definition = resolveSettingsModuleDefinition(moduleId);
    const schemas = Object.fromEntries(surfaces.map((surface) => [surface,
      parseGuidedSettingsSchema(details, locale, t, { surface })]));
    const models = {};
    for (const [surface, schema] of Object.entries(schemas)) {
      const context = `${moduleId}.${locale}.${surface}`;
      assert.equal(schema.parseError, null, context);
      models[surface] = buildConfigurationWorkspaceModel(schema);
      assert.deepEqual(models[surface].items.map((item) => item.fieldKey).sort(),
        sortedKeys(schema.presentationFields), `${context} must keep all presentation fields in a valid section`);
      for (const field of schema.fields) {
        assert.equal(surfaceByOwner[field.presentation.owner], surface, `${context}.${field.key}`);
        assert.equal(field.sectionId, properties[field.key]["x-lsgm-section"], `${context}.${field.key}`);
      }
    }

    const general = schemas.general;
    const metadata = new Map(general.presentationFields.map((field) => [field.key, field]));
    for (const [key, property] of Object.entries(properties)) {
      const field = metadata.get(key);
      assert.ok(field, `${moduleId}.${key} must retain schema presentation metadata`);
      const editable = scalarType(property) && ["editable", "specialized"].includes(field.presentation.state);
      const expectedSurface = editable ? surfaceByOwner[field.presentation.owner] : undefined;
      const owners = surfaces.filter((surface) => schemas[surface].fields.some((candidate) => candidate.key === key));
      assert.deepEqual(owners, expectedSurface ? [expectedSurface] : [], `${moduleId}.${key} must have one scalar editing owner`);
    }
    const navigation = flatten(configurationNavigationRoots(models.general.roots));
    for (const item of models.general.items) {
      if (item.owner === "configuration" && ["editable", "specialized"].includes(item.state)) {
        assert.ok(navigation.some((node) => node.id === item.sectionId), `${moduleId}.${item.fieldKey} must be reachable`);
      }
    }
    for (const node of navigation) {
      assert.ok(node.builtInEditor || node.children.length || node.items.some((item) => item.owner === "configuration"),
        `${moduleId}.${node.id} must not expose an external-only category`);
    }
    assertGroupedControls(general, definition, locale, t, false, `${moduleId}.general`);

    const settings = {};
    for (const [index, [key, property]] of Object.entries(properties).entries()) {
      const external = !["configuration", "instance_network"].includes(metadata.get(key).presentation.owner);
      if (external || index % 2 === 0) settings[key] = savedValue(property);
    }
    for (const field of metadata.values()) {
      if (!Object.hasOwn(properties, field.key) && !["configuration", "instance_network"].includes(field.presentation.owner)) {
        settings[field.key] = false;
      }
    }
    const unknownKey = "unrecognized_saved_configuration_fixture";
    assert.equal(metadata.has(unknownKey), false);
    settings[unknownKey] = { empty: "", disabled: false, zero: 0, nested: [null, "retained"] };
    const settingsBefore = structuredClone(settings);
    const schemaBefore = structuredClone(general);
    const saved = buildSavedConfigurationSchema(general, settings, t);
    const expectedSavedKeys = Object.keys(settings).filter((key) => !metadata.has(key) ||
      ["configuration", "instance_network"].includes(metadata.get(key).presentation.owner)).sort();
    assert.deepEqual(sortedKeys(saved.fields), expectedSavedKeys, `${moduleId} archives must exclude external owners and keep unknown keys`);
    const savedModel = buildConfigurationWorkspaceModel(saved);
    assert.deepEqual(savedModel.items.map((item) => item.fieldKey).sort(), expectedSavedKeys);
    for (const field of saved.fields) {
      assert.equal(Object.hasOwn(field, "defaultValue"), false, `${moduleId}.${field.key} must not initialize archive defaults`);
      assert.equal(Object.hasOwn(field, "defaultSource"), false, `${moduleId}.${field.key} must not derive archive defaults`);
      assert.equal(field.presentation.owner, "configuration");
      const source = metadata.get(field.key);
      if (source) {
        assert.equal(field.sectionId, source.sectionId);
        assert.equal(field.title, source.title);
        assert.equal(field.sourceKey, source.sourceKey);
        assert.equal(field.presentation.behavior, source.presentation.behavior);
      }
    }
    assertGroupedControls(saved, definition, locale, t, true, `${moduleId}.archive`);
    const emptySaved = buildSavedConfigurationSchema(general, {}, t);
    assert.deepEqual(emptySaved.fields, [], `${moduleId} missing saved settings must stay absent`);
    assert.doesNotThrow(() => buildConfigurationWorkspaceModel(emptySaved));
    assert.deepEqual(settings, settingsBefore, `${moduleId} archive projection must preserve saved values`);
    assert.deepEqual(general, schemaBefore, `${moduleId} archive projection must retain source ownership metadata`);
  });
}
