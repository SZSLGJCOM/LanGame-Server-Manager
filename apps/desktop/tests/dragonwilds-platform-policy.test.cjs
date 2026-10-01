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

// Register styles after source extensions so extensionless imports resolve TypeScript first.
require.extensions[".css"] = (module) => module._compile("", module.filename);

const {
  parseGuidedSettingsSchema, readGuidedFieldValue, writeGuidedFieldValue, serializeSettingsObject
} = require("../src/views/settings/guided-settings.ts");
const { resolveSettingsModuleDefinition } = require("../src/views/settings/module-registry.ts");
const { EN_US_RUNESCAPE_DRAGONWILDS_MESSAGES } = require("../src/i18n/games/runescapedragonwilds.en.ts");
const { ZH_CN_RUNESCAPE_DRAGONWILDS_MESSAGES } = require("../src/i18n/games/runescapedragonwilds.zh-cn.ts");
const schemaJson = fs.readFileSync(path.resolve(__dirname, "../../../modules/runescapedragonwilds/schema.json"), "utf8");
const nativeValues = ["Crossplay", "PC", "PlayStation", "Xbox", "Nintendo"];

function schemaFor(locale, catalog) {
  const t = (key, _params, fallback) => catalog[key] ?? fallback ?? key;
  const schema = parseGuidedSettingsSchema({
    summary: { id: "runescapedragonwilds", name: "Dragonwilds" }, schema_json: schemaJson
  }, locale, t);
  assert.equal(schema.parseError, null);
  return { schema, t };
}

test("Dragonwilds presents the native platform policy once under admission with localized choices", () => {
  for (const [locale, catalog] of [["en-US", EN_US_RUNESCAPE_DRAGONWILDS_MESSAGES], ["zh-CN", ZH_CN_RUNESCAPE_DRAGONWILDS_MESSAGES]]) {
    const { schema, t } = schemaFor(locale, catalog);
    const fields = schema.fields.filter((field) => field.key === "platform_policy");
    assert.equal(fields.length, 1, "the verified platform admission control must exist");
    const field = fields[0];
    assert.equal(field.sectionId, "access");
    assert.equal(field.control, "select");
    assert.deepEqual(field.enumOptions.map((option) => option.value), ["", ...nativeValues]);
    if (locale === "zh-CN") {
      assert.match(field.title, /平台/);
      assert.match(field.description, /保留/);
      for (const option of field.enumOptions) assert.match(option.label, /[\u3400-\u9fff]/);
    }
    const groups = resolveSettingsModuleDefinition("runescapedragonwilds").buildFieldGroups(
      "access", schema.fields.filter((item) => item.sectionId === "access"), locale, t
    );
    assert.deepEqual(groups.find((group) => group.id === "admission").fields.map((item) => item.key), ["platform_policy"]);
  }
});

test("Dragonwilds optional platform override keeps unrelated values through edit save and clear", () => {
  const { schema } = schemaFor("en-US", EN_US_RUNESCAPE_DRAGONWILDS_MESSAGES);
  const field = schema.fields.find((item) => item.key === "platform_policy");
  assert.ok(field, "the optional platform override must be available");
  const initial = { server_name: "Existing room", future_setting: { retain: true } };
  assert.equal(readGuidedFieldValue(field, initial), "");
  assert.equal(Object.hasOwn(initial, "platform_policy"), false);
  for (const policy of nativeValues) {
    const edited = writeGuidedFieldValue(initial, field, policy);
    const persisted = JSON.parse(serializeSettingsObject(edited));
    assert.equal(readGuidedFieldValue(field, persisted), policy);
    const cleared = JSON.parse(serializeSettingsObject(writeGuidedFieldValue(persisted, field, "")));
    assert.equal(readGuidedFieldValue(field, cleared), "");
    assert.deepEqual(cleared.future_setting, initial.future_setting);
    assert.equal(cleared.server_name, initial.server_name);
  }
});
