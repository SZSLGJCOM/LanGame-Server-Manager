const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function compileTypeScript(module, filename) {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}
require.extensions[".ts"] = compileTypeScript;
require.extensions[".tsx"] = compileTypeScript;
require.extensions[".css"] = (module) => module._compile("module.exports = {};", module.filename);

const { parseGuidedSettingsSchema, writeGuidedFieldValue } = require("../src/views/settings/guided-settings.ts");
const { resolveSettingsModuleDefinition } = require("../src/views/settings/module-registry.ts");
const { EN_US_MESSAGES } = require("../src/i18n-messages.ts");
const { ZH_CN_MESSAGES } = require("../src/i18n-messages-zh-cn.ts");
const root = path.resolve(__dirname, "../../..");
const moduleId = "arksurvivalascended";
const schema = JSON.parse(fs.readFileSync(path.join(root, "modules", moduleId, "schema.json"), "utf8"));
const descriptions = JSON.parse(fs.readFileSync(path.join(root, "scripts/ark_field_descriptions.json"), "utf8"))[moduleId];
const expected = {
  num_gachas_per_tribe_for_penalty: ["limits", "gacha", -1],
  gachas_over_tribe_limit_penalty: ["limits", "gacha", 3],
  limit_replicators_per_saddle: ["building", "structure-rules", 0],
  limit_cloning_chambers_range: ["limits", "cloning", 10000],
  limit_cloning_chambers_in_range_num: ["limits", "cloning", 0],
  limit_cloning_chambers_in_tribe_num: ["limits", "cloning", -1]
};

test("ASA v97 controls are localized, optional and grouped once by behavior", () => {
  const definition = resolveSettingsModuleDefinition(moduleId);
  for (const [locale, catalog] of [["en-US", EN_US_MESSAGES], ["zh-CN", ZH_CN_MESSAGES]]) {
    const translate = (key, _params, fallback) => catalog[key] ?? fallback ?? key;
    const parsed = parseGuidedSettingsSchema({
      summary: { id: moduleId, name: moduleId },
      schema_json: JSON.stringify(schema)
    }, locale, translate);
    assert.deepEqual(
      parsed.fields.filter((field) => field.sourceId === "asa_server_patch_notes_v97").map((field) => field.key).sort(),
      Object.keys(expected).sort()
    );
    for (const [key, [sectionId, groupId, value]] of Object.entries(expected)) {
      const field = parsed.fields.find((candidate) => candidate.key === key);
      assert.equal(field.required, false, key);
      assert.equal(field.defaultValue, undefined, key);
      assert.equal(field.control, "number", key);
      assert.equal(field.sectionId, sectionId, key);
      assert.equal(field.presentation.state, "editable", key);
      const groups = definition.buildFieldGroups(sectionId,
        parsed.fields.filter((candidate) => candidate.sectionId === sectionId), locale, translate);
      assert.equal(groups.flatMap((group) => group.fields).filter((candidate) => candidate.key === key).length, 1, key);
      assert.ok(groups.find((group) => group.id === groupId).fields.includes(field), key);

      const filled = writeGuidedFieldValue({ server_name: "ASA" }, field, String(value));
      assert.equal(filled[key], value, key);
      assert.deepEqual(writeGuidedFieldValue(filled, field, ""), { server_name: "ASA" }, key);
      const prefix = `settings.schema.${moduleId}.${key}`;
      assert.ok(catalog[`${prefix}.title`], key);
      assert.equal(catalog[`${prefix}.description`], descriptions[key][locale === "en-US" ? "en" : "zh-CN"], key);
      assert.equal(schema.properties[key].description, descriptions[key].en, key);
    }
    for (const groupId of ["gacha", "cloning"]) {
      assert.ok(catalog[`ark.settings.groups.limits.${groupId}.title`], `${locale}: ${groupId}`);
    }
  }
});
