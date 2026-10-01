const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) =>
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}
require.extensions[".css"] = (module, filename) => module._compile("", filename);
require.extensions[".png"] = (module, filename) => module._compile(`module.exports = ${JSON.stringify(filename)};`, filename);

const { parseGuidedSettingsSchema } = require("../src/views/settings/guided-settings.ts");
const { summarizeConfigurationFieldHelp } = require("../src/views/settings/ConfigurationFieldHelp.tsx");
const { coreKeeperSettingsDefinition } = require("../src/views/settings/modules/corekeeper.ts");
const { EN_US_MESSAGES } = require("../src/i18n-messages.ts");
const { ZH_CN_MESSAGES } = require("../src/i18n-messages-zh-cn.ts");
const modulesRoot = path.resolve(__dirname, "../../../modules");
const reviewedModules = ["abioticfactor", "astroneer", "corekeeper", "dontstarve", "enshrouded", "humanitz", "minecraft", "necesse", "nightingale", "palworld", "projectzomboid", "returntomoria"];

function readFields(id, locale = "en-US") {
  const catalog = locale === "zh-CN" ? ZH_CN_MESSAGES : EN_US_MESSAGES;
  const t = (key, _params, fallback) => catalog[key] ?? fallback ?? key;
  const schema = parseGuidedSettingsSchema({ summary: { id, name: id }, schema_json: fs.readFileSync(path.join(modulesRoot, id, "schema.json"), "utf8") }, locale, t);
  assert.equal(schema.parseError, null);
  return { fields: schema.presentationFields ?? schema.fields, t };
}

test("reviewed Chinese configuration labels distinguish controls without generated word fragments", () => {
  for (const id of reviewedModules) {
    const { fields } = readFields(id, "zh-CN");
    const labels = new Map();
    for (const field of fields.filter((item) => item.presentation?.owner === "configuration" && ["editable", "specialized"].includes(item.presentation.state))) {
      assert.doesNotMatch(field.title, /^原生配置：/u, `${id}.${field.key}`);
      assert.doesNotMatch(field.description ?? "", /^(?:将|根据)“/u, `${id}.${field.key}`);
      const key = `${field.sectionId}:${field.title}`;
      assert.ok(!labels.has(key), `${id}: ${labels.get(key)} and ${field.key} share an indistinguishable label`);
      labels.set(key, field.key);
    }
  }
});

test("Minecraft spam help describes decaying accumulation and preserves the disable value", () => {
  for (const locale of ["en-US", "zh-CN"]) {
    const { fields, t } = readFields("minecraft", locale);
    for (const key of ["chat_spam_threshold_seconds", "command_spam_threshold_seconds"]) {
      const field = fields.find((item) => item.key === key);
      const help = summarizeConfigurationFieldHelp(field.description, field.title, t);
      assert.match(help, /0\.05/);
      assert.match(help, locale === "zh-CN" ? /0 关闭/ : /0 disables/);
      assert.doesNotMatch(help, /Minimum seconds between|间隔低于/i);
    }
  }
});

test("Core Keeper warns that text seeds take precedence over numeric hashes", () => {
  for (const locale of ["en-US", "zh-CN"]) {
    const { fields, t } = readFields("corekeeper", locale);
    const field = fields.find((item) => item.key === "hashed_world_seed");
    const warning = coreKeeperSettingsDefinition.getFieldValidationMessage({ field, value: 123, settings: { world_seed: "world" }, t });
    assert.match(warning, locale === "zh-CN" ? /忽略哈希/ : /ignores the hashed seed/);
    assert.equal(coreKeeperSettingsDefinition.getFieldValidationMessage({ field, value: 123, settings: { world_seed: "" }, t }), undefined);
  }
});

test("native gameplay settings retain their distinct effects", () => {
  const dst = readFields("dontstarve").fields;
  assert.match(dst.find((field) => field.key === "master_frogs").title, /Pond Frog/);
  assert.match(dst.find((field) => field.key === "master_frograin").title, /Frog Rain/);
  assert.match(dst.find((field) => field.key === "caves_atriumgate").description, /40\/30\/20\/10\/5 game days/);
  const pz = readFields("projectzomboid").fields;
  assert.match(pz.find((field) => field.key === "player_respawn_with_other").description, /split-screen or Remote Play/);
  assert.match(pz.find((field) => field.key === "no_fire").description, /except campfires/);
  assert.match(pz.find((field) => field.key === "client_command_filter").description, /does not block command execution/);
});
