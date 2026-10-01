const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const root = path.resolve(__dirname, "../../..");
require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const schema = (game) => JSON.parse(fs.readFileSync(path.join(root, "modules", game, "schema.json"), "utf8"));
const catalog = (game, locale) => Object.values(require(path.join(root, "apps/desktop/src/i18n/games", `${game}.${locale}.ts`)))[0];

test("V Rising Rift presets preserve native IDs and expose the installed binary's duration tables", () => {
  const props = schema("vrising").properties;
  const definition = require("../src/views/settings/modules/vrising.ts").vrisingSettingsDefinition;
  const cases = {
    war_event_interval: [0, ["30 minutes", "1 hour", "1.5 hours", "2 hours", "4 hours", "8 hours", "12 hours", "24 hours"], ["30 分钟", "1 小时", "1.5 小时", "2 小时", "4 小时", "8 小时", "12 小时", "24 小时"]],
    war_event_major_duration: [1, ["15 minutes", "20 minutes", "25 minutes", "30 minutes", "35 minutes", "45 minutes", "1 hour", "2 hours"], ["15 分钟", "20 分钟", "25 分钟", "30 分钟", "35 分钟", "45 分钟", "1 小时", "2 小时"]],
    war_event_minor_duration: [1, ["15 minutes", "20 minutes", "25 minutes", "30 minutes", "35 minutes", "45 minutes", "1 hour", "2 hours"], ["15 分钟", "20 分钟", "25 分钟", "30 分钟", "35 分钟", "45 分钟", "1 小时", "2 小时"]]
  };
  for (const [key, [defaultValue, en, zh]] of Object.entries(cases)) {
    assert.deepEqual(props[key].enum, [0, 1, 2, 3, 4, 5, 6, 7]);
    assert.equal(props[key].default, defaultValue);
    assert.equal(props[key].maximum, 7);
    assert.deepEqual(props[key].enum_labels, en);
    for (const [locale, expected] of [["en", en], ["zh-cn", zh]]) {
      const messages = catalog("vrising", locale);
      const translate = (messageKey, _params, fallback = messageKey) => messages[messageKey] ?? fallback;
      assert.deepEqual(props[key].enum.map((value) => definition.getEnumOptionLabel(key, value, locale, translate)), expected);
    }
  }
});

test("Core Keeper distinguishes per-client snapshot budget from send frequency", () => {
  const en = catalog("corekeeper", "en");
  const zh = catalog("corekeeper", "zh-cn");
  assert.match(en["settings.schema.corekeeper.max_packets_per_frame.description"], /1,200[\s\S]*9,440/);
  assert.match(zh["settings.schema.corekeeper.max_packets_per_frame.description"], /1,200[\s\S]*9,440/);
  assert.match(en["settings.schema.corekeeper.network_send_rate.title"], /Hz/);
  assert.match(zh["settings.schema.corekeeper.network_send_rate.title"], /Hz/);
  assert.doesNotMatch(en["settings.schema.corekeeper.max_players.description"], /Core Keeper currently accepts 1 to 100/);
});

test("Terraria NPC stream copy describes update intervals and zero without a radius", () => {
  const en = catalog("terraria", "en");
  const zh = catalog("terraria", "zh-cn");
  assert.match(en["settings.schema.terraria.npcstream.description"], /update[\s\S]*0 disables/);
  assert.match(zh["settings.schema.terraria.npcstream.description"], /更新[\s\S]*0/);
  assert.doesNotMatch(en["terraria.settings.validation.npcstreamNonNegative"], /radius/i);
  assert.doesNotMatch(zh["terraria.settings.validation.npcstreamNonNegative"], /半径/);
});

test("Romestead exposes an effective cheat gate rather than an obsolete reserved claim", () => {
  const en = catalog("romestead", "en");
  const zh = catalog("romestead", "zh-cn");
  assert.doesNotMatch(schema("romestead").properties.enable_cheats.description, /reserved/i);
  assert.match(en["settings.schema.romestead.enable_cheats.description"], /spawn[\s\S]*weather/);
  assert.match(zh["settings.schema.romestead.enable_cheats.description"], /生成[\s\S]*天气/);
});
