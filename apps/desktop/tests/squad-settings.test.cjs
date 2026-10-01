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

const {
  parseGuidedSettingsSchema,
  readGuidedFieldValue,
  validateGuidedSettingsObject
} = require("../src/views/settings/guided-settings.ts");
const { translate } = require("../src/i18n.tsx");
const { EN_US_MESSAGES } = require("../src/i18n-messages.ts");
const { ZH_CN_MESSAGES } = require("../src/i18n-messages-zh-cn.ts");
const moduleDetails = {
  summary: { id: "squad", name: "Squad" },
  schema_json: fs.readFileSync(path.resolve(__dirname, "../../../modules/squad/schema.json"), "utf8")
};
const schema = parseGuidedSettingsSchema(moduleDetails, "en-US");

for (const locale of ["en-US", "zh-CN"]) {
  test(`Squad ${locale} distinguishes maps, layers, factions and rotation order`, () => {
    const catalogs = { "en-US": EN_US_MESSAGES, "zh-CN": ZH_CN_MESSAGES };
    const t = (key, params, fallback) => translate(locale, key, params, fallback, catalogs);
    const localized = parseGuidedSettingsSchema(moduleDetails, locale, t);
    const field = (key) => localized.fields.find((entry) => entry.key === key);
    const map = locale === "zh-CN" ? /地图/ : /map/i;
    const layer = locale === "zh-CN" ? /图层/ : /layer/i;
    const faction = locale === "zh-CN" ? /阵营/ : /faction/i;
    assert.match(field("use_vote_level").title, map);
    assert.match(field("use_vote_layer").title, layer);
    assert.match(field("use_vote_factions").title, faction);
    assert.match(field("num_players_diff_for_team_changes").title,
      locale === "zh-CN" ? /人数差/ : /player difference/i);
    assert.match(field("vehicle_claiming_disabled").title,
      locale === "zh-CN" ? /载具认领/ : /vehicle claiming/i);
    assert.match(field("vehicle_kit_requirement_disabled").title,
      locale === "zh-CN" ? /载具兵种/ : /vehicle role/i);
    assert.match(field("allow_fireteam_layers_in_rotation").title, /Fireteam/);
    assert.equal(readGuidedFieldValue(field("vehicle_claiming_disabled"), {}), false);
    assert.equal(readGuidedFieldValue(field("vehicle_kit_requirement_disabled"), {}), false);
    const rotation = field("map_rotation_mode");
    assert.equal(new Set(rotation.enumOptions.map((option) => option.label)).size, 5);
    for (const option of rotation.enumOptions) {
      assert.match(option.label, option.value.startsWith("Level") ? map : layer);
      const mode = option.value.endsWith("_Randomized")
        ? (locale === "zh-CN" ? /随机/ : /random/i)
        : option.value.endsWith("_Vote")
          ? (locale === "zh-CN" ? /投票/ : /vot/i)
          : (locale === "zh-CN" ? /顺序/ : /order/i);
      assert.match(option.label, mode);
      assert.deepEqual(validateGuidedSettingsObject(localized, { map_rotation_mode: option.value })
        .filter((issue) => issue.fieldKey === rotation.key), []);
    }
    assert.equal(readGuidedFieldValue(rotation, {}), "LayerList_Vote");
    const poolKeys = ["layer_voting", "layer_voting_low_players", "layer_voting_night"];
    for (const key of poolKeys) {
      const pool = field(key);
      assert.match(pool.title, locale === "zh-CN" ? /图层池/ : /layer pool/i);
      assert.equal(pool.type, "string");
      assert.match(pool.description, /\.cfg/);
      assert.doesNotMatch(pool.description, /whether|是否/i);
    }
    assert.equal(new Set(poolKeys.map((key) => field(key).title)).size, 3);
  });
}

for (const key of ["time_between_matches_seconds", "time_before_vote_seconds"]) {
  test(`Squad ${key} rejects delays below the official 30-second minimum`, () => {
    const field = schema.fields.find((entry) => entry.key === key);
    assert.ok(field);
    assert.equal(readGuidedFieldValue(field, {}), 60);
    for (const value of [0, 29]) {
      const issues = validateGuidedSettingsObject(schema, { [key]: value });
      assert.ok(issues.some((issue) => issue.fieldKey === key && issue.reason === "minimum"));
    }
    for (const value of [30, 60, 120]) {
      const issues = validateGuidedSettingsObject(schema, { [key]: value });
      assert.deepEqual(issues.filter((issue) => issue.fieldKey === key), []);
    }
  });
}

for (const [locale, expectedTitles] of [
  ["en-US", [
    "Time Between Matches (Seconds)",
    "Delay Before Voting (Seconds)",
    "Standard Match Preparation (Seconds)",
    "Small-Scale Match Preparation (Seconds)"
  ]],
  ["zh-CN", [
    "对局结束等待时间（秒）",
    "投票开始等待时间（秒）",
    "标准对局准备时间（秒）",
    "小规模对局准备时间（秒）"
  ]]
]) {
  test(`Squad timers have distinct ${locale} labels and actionable validation messages`, () => {
    const catalogs = { "en-US": EN_US_MESSAGES, "zh-CN": ZH_CN_MESSAGES };
    const t = (key, params, fallback) => translate(locale, key, params, fallback, catalogs);
    const localized = parseGuidedSettingsSchema(moduleDetails, locale, t);
    const keys = [
      "time_between_matches_seconds", "time_before_vote_seconds",
      "prep_time_standard_seconds", "prep_time_small_scale_seconds"
    ];
    const fields = keys.map((key) => localized.fields.find((entry) => entry.key === key));
    assert.deepEqual(fields.map((field) => field.title), expectedTitles);
    assert.equal(new Set(fields.map((field) => field.title)).size, 4);
    assert.deepEqual(fields.map((field) => readGuidedFieldValue(field, {})), [60, 60, 240, 180]);
    for (const field of fields) {
      const issues = validateGuidedSettingsObject(localized, { [field.key]: field.minimum - 1 }, undefined, t);
      const issue = issues.find((entry) => entry.fieldKey === field.key && entry.reason === "minimum");
      assert.ok(issue);
      assert.ok(issue.message.includes(field.title));
      assert.ok(issue.message.includes(String(field.minimum)));
      assert.ok(field.description.includes(String(field.defaultValue)));
    }
  });
}
