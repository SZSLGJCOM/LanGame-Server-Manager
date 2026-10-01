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
require.extensions[".css"] = (module) => module._compile("module.exports = {};", module.filename);

const { validateUnturnedLobbyLinks, applyUnturnedSettingsPatch } = require("../src/views/settings/modules/unturned-native-settings.ts");
const { parseGuidedSettingsSchema, isOptionalBooleanOverride, readGuidedFieldValue } = require("../src/views/settings/guided-settings.ts");
const { buildConfigurationWorkspaceModel } = require("../src/views/settings/configuration-workspace-model.ts");
const { unturnedSettingsDefinition } = require("../src/views/settings/modules/unturned.ts");
const moduleRoot = path.resolve(__dirname, "../../../modules/unturned");
const schemaJson = fs.readFileSync(path.join(moduleRoot, "schema.json"), "utf8");
const schema = JSON.parse(schemaJson);
const t = (_key, _parameters, fallback) => fallback;

test("all 263 official native additions have typed UI fields without guessed defaults or specialized-page duplication", () => {
  const parsed = parseGuidedSettingsSchema({ summary: { id: "unturned", name: "Unturned" }, schema_json: schemaJson });
  assert.equal(parsed.parseError, null);
  const model = buildConfigurationWorkspaceModel(parsed);
  const added = Object.entries(schema.properties).filter(([, property]) => property["x-lsgm-source"] === "native_config_3_26_3_12");
  assert.equal(added.length, 263);
  for (const [key, property] of added) {
    const field = parsed.fields.find((field) => field.key === key);
    assert.ok(field, key);
    assert.equal(property.default, undefined, key);
    assert.equal(model.items.find((item) => item.fieldKey === key)?.owner, "configuration", key);
    assert.equal(field.sectionId, property["x-lsgm-section"], key);
    if (field.type === "boolean") assert.equal(isOptionalBooleanOverride(field), true, key);
  }
  for (const [section, fields] of Object.entries(Object.groupBy(parsed.fields, (field) => field.sectionId))) {
    if (section === "room") continue;
    const groups = unturnedSettingsDefinition.buildFieldGroups(section, fields, "en", t);
    const keys = groups.flatMap((group) => group.fields.map((field) => field.key));
    assert.equal(new Set(keys).size, keys.length, section);
    assert.deepEqual([...keys].sort(), fields.map((field) => field.key).sort(), section);
  }
});

test("clearing optional overrides deletes the manager setting while false and zero remain explicit", () => {
  const before = { native_browser_thumbnail: "https://example.test/x", browser_links_json: "[]", native_browser_monetization: "Any", native_items_spawn_chance: 2, native_items_has_durability: true, password: "" };
  const next = applyUnturnedSettingsPatch(before, { native_browser_thumbnail: "", browser_links_json: undefined, native_browser_monetization: "", native_items_spawn_chance: undefined, native_items_has_durability: false });
  assert.deepEqual(next, { native_items_has_durability: false, password: "" });
  assert.equal(before.native_items_spawn_chance, 2);
  assert.equal(applyUnturnedSettingsPatch(next, { native_items_spawn_chance: 0 }).native_items_spawn_chance, 0);
  assert.equal(schema.properties.native_browser_monetization.enum[0], "");
});

test("lobby links validate exact shape, UTF-8 bounds, protocol and control characters", () => {
  const validate = (value) => validateUnturnedLobbyLinks(value, t);
  assert.equal(validate(undefined), undefined);
  assert.equal(validate(""), undefined);
  assert.equal(validate(JSON.stringify([{ Message: "规则\n引号\"", URL: "https://example.test/rules" }])), undefined);
  assert.equal(validate("[]"), undefined);
  for (const value of ["{}", "[null]", " ", JSON.stringify([{ Message: "x", URL: "file:///x" }]), JSON.stringify([{ Message: "x", URL: "https://x\ny" }]), JSON.stringify([{ Message: "界".repeat(342), URL: "https://example.test" }]), JSON.stringify([{ Message: "x", URL: "https://example.test", Extra: true }]), JSON.stringify(Array.from({ length: 33 }, () => ({ Message: "x", URL: "https://example.test" })))]) {
    assert.equal(typeof validate(value), "string", value.slice(0, 50));
  }
});

test("an untouched optional links field passes the real empty-form read and module validation chain", () => {
  const parsed = parseGuidedSettingsSchema({ summary: { id: "unturned", name: "Unturned" }, schema_json: schemaJson });
  const field = parsed.fields.find((field) => field.key === "browser_links_json");
  const settings = { internet_server: false };
  const value = readGuidedFieldValue(field, settings);
  assert.equal(value, "");
  assert.equal(unturnedSettingsDefinition.getFieldValidationMessage({ field, value, settings, locale: "en", t }), undefined);
  assert.equal(applyUnturnedSettingsPatch(settings, { browser_links_json: value }).browser_links_json, undefined);
});
