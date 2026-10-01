const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) require.extensions[extension] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
require.extensions[".css"] = (module) => module._compile("", module.filename);
const repo = path.resolve(__dirname, "../../..");
const schema = require(path.join(repo, "modules/projectzomboid/schema.json"));
const inventory = require(path.join(repo, "modules/projectzomboid/server-options-build-42.json"));
const retired = require(path.join(repo, "modules/projectzomboid/retired-server-options.json"));
const { parseGuidedSettingsSchema } = require("../src/views/settings/guided-settings.ts");
const { resolveSettingsModuleDefinition } = require("../src/views/settings/module-registry.ts");
const { buildConfigurationWorkspaceModel } = require("../src/views/settings/configuration-workspace-model.ts");
const { EN_US_PROJECT_ZOMBOID_MESSAGES: en } = require("../src/i18n/games/projectzomboid.en.ts");
const { ZH_CN_PROJECT_ZOMBOID_MESSAGES: zh } = require("../src/i18n/games/projectzomboid.zh-cn.ts");
const filename = path.resolve(__dirname, "../src/views/settings/ProjectZomboidPolicyReview.tsx");
function component(catalog = en) {
  const loaded = new Module(filename, module); loaded.filename = filename;
  const fromFile = Module.createRequire(filename);
  loaded.require = (id) => id === "../../i18n" ? { useI18n: () => ({ t: (key, _params, fallback) => catalog[key] ?? fallback }) } : fromFile(id);
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  return loaded.exports;
}
function descendants(node) {
  return React.isValidElement(node) ? [node, ...React.Children.toArray(node.props.children).flatMap(descendants)] : [];
}

test("all 144 native Build 42 options are modeled or have a managed identity/port owner", () => {
  const managed = new Set(["DefaultPort", "UDPPort", "RCONPort", "ResetID", "ServerPlayerID"]);
  const modeled = new Set(Object.values(schema.properties).map((field) => field["x-lsgm-source-key"]));
  assert.equal(inventory.options.length, 144);
  for (const option of inventory.options) assert.ok(modeled.has(option.key) || managed.has(option.key), option.key);
  const template = fs.readFileSync(path.join(repo, "modules/projectzomboid/templates/server.ini.hbs"), "utf8");
  for (const [key, option] of Object.entries(retired)) {
    assert.equal(schema.properties[key], undefined, key);
    assert.ok(!template.includes(`${option.native_key}=`), option.native_key);
  }
});

test("native anti-cheat enums and constraints match the shipped constructors", () => {
  assert.deepEqual(schema.properties.anti_cheat_speed.enum, [1, 2, 3, 4]);
  assert.equal(schema.properties.anti_cheat_speed.default, 2);
  assert.equal(schema.properties.anti_cheat_no_clip.default, 4);
  assert.equal(schema.properties.bad_word_policy.enum.length, 3);
  assert.equal(schema.properties.max_players.maximum, 254);
  assert.equal(schema.properties.chat_message_character_limit.minimum, 64);
});

test("new fields have dedicated groups and keep player lists, Mods and maintenance outside Config", () => {
  const parsed = parseGuidedSettingsSchema({ summary: { id: "projectzomboid" }, schema_json: JSON.stringify(schema) });
  const model = buildConfigurationWorkspaceModel(parsed);
  const definition = resolveSettingsModuleDefinition("projectzomboid");
  for (const [key, field] of Object.entries(schema.properties)) {
    if (field["x-lsgm-order"] < 900) continue;
    const item = model.items.find((item) => item.fieldKey === key);
    assert.equal(item.owner, "configuration", key);
    const fields = parsed.fields.filter((entry) => entry.sectionId === field["x-lsgm-section"]);
    const groups = definition.buildFieldGroups(field["x-lsgm-section"], fields, "en-US", (_key, _params, fallback) => fallback);
    assert.ok(groups.some((group) => group.id !== "additional" && group.fields.some((entry) => entry.key === key)), key);
  }
});

test("review is absent for fresh/default settings and never leaks stored values", () => {
  const { ProjectZomboidPolicyReview: Review } = component();
  const defaults = Object.fromEntries(Object.entries(retired).map(([key, entry]) => [key, entry.default]));
  for (const settings of [{}, defaults]) assert.equal(Review({ settings, disabled: false, onPatch() {} }), null);
  const node = Review({ settings: { discord_channel_id: "private-channel-identifier" }, disabled: false, onPatch() {} });
  const html = renderToStaticMarkup(node);
  assert.ok(html.includes("DiscordChannelID"));
  assert.ok(!html.includes("private-channel-identifier"));
});

test("checking and cancelling review change only the acknowledgement; disabled UI stays disabled", () => {
  for (const catalog of [en, zh]) {
    const { ProjectZomboidPolicyReview: Review, PROJECT_ZOMBOID_REVIEW_KEY: key } = component(catalog);
    const patches = [], settings = { anti_cheat_protection_type_1: false, mods: "A" };
    const props = { settings, disabled: false, onPatch(patch) { patches.push(patch); Object.assign(settings, patch); } };
    let input = descendants(Review(props)).find((node) => node.type === "input");
    assert.equal(input.props.checked, false);
    input.props.onChange({ currentTarget: { checked: true } });
    assert.deepEqual(patches[0], { [key]: true });
    input = descendants(Review(props)).find((node) => node.type === "input");
    assert.equal(input.props.checked, true);
    input.props.onChange({ currentTarget: { checked: false } });
    assert.deepEqual(patches[1], { [key]: false });
    assert.equal(settings.anti_cheat_protection_type_1, false);
    assert.equal(settings.mods, "A");
    props.disabled = true;
    assert.equal(descendants(Review(props)).find((node) => node.type === "input").props.disabled, true);
  }
});
