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
const { parseGuidedSettingsSchema, validateGuidedSettingsObject } = require("../src/views/settings/guided-settings.ts");
const { EN_US_MESSAGES } = require("../src/i18n-messages.ts");
const { ZH_CN_MESSAGES } = require("../src/i18n-messages-zh-cn.ts");
const root = path.resolve(__dirname, "../../..");

function parsed(moduleId, locale) {
  const messages = locale === "zh-CN" ? ZH_CN_MESSAGES : EN_US_MESSAGES;
  return parseGuidedSettingsSchema({
    summary: { id: moduleId, name: moduleId },
    schema_json: fs.readFileSync(path.join(root, "modules", moduleId, "schema.json"), "utf8")
  }, locale, (key, _parameters, fallback) => messages[key] ?? fallback ?? key);
}

function field(moduleId, locale, key) {
  const settings = parsed(moduleId, locale);
  assert.equal(settings.parseError, null);
  const value = settings.fields.find((entry) => entry.key === key);
  assert.ok(value, `${moduleId}.${key} is absent from the configuration form`);
  return value;
}

test("ARK describes text proximity, global platform limits and respawn delay correctly", () => {
  for (const id of ["arksurvivalascended", "arksurvivalevolved"]) {
    assert.match(field(id, "zh-CN", "proximity_chat").title, /文字/);
    assert.doesNotMatch(field(id, "zh-CN", "proximity_chat").description ?? "", /语音/);
    assert.match(field(id, "zh-CN", "resources_respawn_period_multiplier").description, /越大.*越慢/);
  }
  assert.match(field("arksurvivalevolved", "zh-CN", "max_platform_saddle_structure_limit").title, /服务器.*数量上限/);
  assert.match(field("arksurvivalevolved", "en-US", "max_platform_saddle_structure_limit").description, /entire|whole|server/i);
});

test("Conan accepts all twelve documented floating-point settings and keeps schedules distinct", () => {
  const settings = parsed("conanexiles", "zh-CN");
  const fixture = JSON.parse(fs.readFileSync(path.join(root, "modules/conanexiles/config-fixtures/2026-07-13-steamcmd_anonymous_validate_443030.json"), "utf8"));
  const keys = new Set(["avatar_lifetime", "chat_local_radius", "land_claim_radius_multiplier", "item_convertion_multiplier",
    "thrall_corruption_removal_multiplier", "player_corruption_gain_multiplier", "player_corruption_gain_from_sorcery_multiplier",
    "animal_pen_crafting_time_multiplier", "feed_box_range_multiplier", "unconscious_time_seconds", "stability_loss_multiplier", "healthbar_visibility_distance"]);
  for (const key of keys) assert.ok(!Number.isInteger(fixture.settings[key]), `${key} must exercise a fraction`);
  assert.deepEqual(validateGuidedSettingsObject(settings, fixture.settings).filter((issue) => keys.has(issue.fieldKey)), []);
  const schedule = settings.fields.filter((entry) => /^pvp_(time|enabled|building_damage_time|building_damage_enabled)_/.test(entry.key));
  assert.equal(schedule.length, 42);
  assert.equal(new Set(schedule.map((entry) => entry.title)).size, 42);
  assert.equal(field("conanexiles", "zh-CN", "region_allow_asia").sectionId, "access");
  assert.equal(field("conanexiles", "zh-CN", "use_minion_population_limit").sectionId, "followers");
  assert.equal(field("conanexiles", "zh-CN", "offline_players_unconscious_bodies_hours").sectionId, "survival");
  assert.match(field("conanexiles", "en-US", "building_damage_multiplier").description, /received/i);
});

test("RimWorld exposes actual pricing, travel and save behavior instead of file provenance", () => {
  assert.match(field("rimworld", "en-US", "action_market_price_multiplier").description, /base × \(1 \+ x\).*base ÷ \(1 \+ 2x\)/);
  assert.match(field("rimworld", "zh-CN", "action_road_dirt_path_multiplier").description, /值越小.*越快/);
  assert.match(field("rimworld", "zh-CN", "action_site_building_cost").title, /白银/);
  assert.match(field("rimworld", "zh-CN", "action_site_time_interval").title, /毫秒/);
  assert.match(field("rimworld", "zh-CN", "action_zoom_is_enabled").title, /地图预览/);
  assert.match(field("rimworld", "zh-CN", "action_scenario_cooldown").description, /没有使用/);
  assert.match(field("rimworld", "en-US", "sync_local_save").description, /local save.*server requires/i);
  assert.match(field("rimworld", "zh-CN", "enable_server_telemetry").description, /玩家名称/);
  assert.equal(field("rimworld", "zh-CN", "chat_login_notifications").sectionId, "room");
});

test("reviewed configuration fields have unique names and no source-only or generated help", () => {
  for (const id of ["arksurvivalascended", "arksurvivalevolved", "conanexiles", "rimworld"]) {
    for (const locale of ["en-US", "zh-CN"]) {
      const settings = parsed(id, locale);
      const names = new Set();
      for (const entry of settings.fields) {
        const name = `${entry.sectionId}:${entry.title}`;
        assert.ok(!names.has(name), `${id}/${locale}: duplicate ${name}`);
        names.add(name);
        assert.doesNotMatch(entry.description ?? "", /Conan Exiles Enhanced build|Optional override of|控制 .*此项会写入|pinned exact-build/i);
      }
    }
  }
});
