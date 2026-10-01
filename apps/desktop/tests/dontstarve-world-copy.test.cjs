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

const { parseGuidedSettingsSchema } = require("../src/views/settings/guided-settings.ts");
const { EN_US_MESSAGES } = require("../src/i18n-messages.ts");
const { ZH_CN_MESSAGES } = require("../src/i18n-messages-zh-cn.ts");
const { EN_US_DONT_STARVE_WORLD_COPY_MESSAGES } = require("../src/i18n/games/dontstarve.en.part1.ts");
const { dontStarveSettingsDefinition } = require("../src/views/settings/modules/dontstarve.ts");
const schema = JSON.parse(fs.readFileSync(path.resolve(__dirname, "../../../modules/dontstarve/schema.json"), "utf8"));
const details = { summary: { id: "dontstarve", name: "Don't Starve Together" }, schema_json: JSON.stringify(schema) };

function fieldsFor(locale, catalog) {
  const t = (key, _params, fallback) => catalog[key] ?? fallback ?? key;
  const parsed = parseGuidedSettingsSchema(details, locale, t);
  assert.equal(parsed.parseError, null);
  return new Map(parsed.fields.map((field) => [field.key, field]));
}

function labels(fields, key) {
  const field = fields.get(key);
  assert.ok(field, `missing ${key}`);
  return Object.fromEntries(field.enumOptions.map((option) => [option.value, option.label]));
}

const reviewedCopyFields = [
  "backup_log_count", "backup_log_period", "caves_acidrain_enabled",
  "caves_atriumgate", "caves_berrybush", "caves_boons",
  "caves_cavelight", "caves_chest_mimics", "caves_daywalker",
  "caves_evergreen_regrowth", "caves_itemmimics", "caves_moon_spider",
  "caves_mutated_birds", "caves_mutated_merm", "caves_mutated_spiderqueen",
  "caves_prefabswaps_start", "caves_reeds_regrowth", "caves_rifts_enabled",
  "caves_rifts_frequency", "caves_start_location", "caves_task_set",
  "caves_touchstone", "caves_tree_rock", "caves_tree_rock_regrowth",
  "caves_twiggytrees_regrowth", "caves_wormattacks_boss", "master_alternatehunt",
  "master_antliontribute", "master_balatro", "master_bananabush_portalrate",
  "master_boons", "master_cactus_regrowth", "master_daywalker2",
  "master_eyeofterror", "master_frograin", "master_frogs",
  "master_hound_mounds", "master_hunt", "master_junkyard",
  "master_lightcrab_portalrate", "master_lunarhail_frequency", "master_monkeytail_portalrate",
  "master_mutated_bearger", "master_mutated_bird_gestalt", "master_mutated_birds",
  "master_mutated_buzzard_gestalt", "master_mutated_deerclops", "master_mutated_merm",
  "master_mutated_spiderqueen", "master_mutated_warg", "master_ocean_otterdens",
  "master_otters_setting", "master_palmcone_seed_portalrate", "master_palmconetree",
  "master_palmconetree_regrowth", "master_pigs_setting", "master_pirateraids",
  "master_portal_spawnrate", "master_powder_monkey_portalrate", "master_rabbits_setting",
  "master_reeds_regrowth", "master_rifts_enabled", "master_rifts_frequency",
  "master_sharkboi", "master_spiders_setting", "master_stageplays",
  "master_summerhounds", "master_terrariumchest", "master_walrus_setting",
  "master_wanderingtrader_enabled", "master_winterhounds", "world_basicresource_regrowth",
  "world_crow_carnival", "world_ghostenabled", "world_ghostsanitydrain",
  "world_hallowed_nights", "world_lessdamagetaken", "world_portalresurection",
  "world_resettime", "world_spawnmode", "world_winters_feast",
  "world_year_of_the_beefalo", "world_year_of_the_bunnyman", "world_year_of_the_carrat",
  "world_year_of_the_catcoon", "world_year_of_the_dragonfly", "world_year_of_the_gobbler",
  "world_year_of_the_knight", "world_year_of_the_pig", "world_year_of_the_snake",
  "world_year_of_the_varg"
];

test("DST reviewed fields show meaningful bilingual names and descriptions", () => {
  const copyFields = Object.keys(EN_US_DONT_STARVE_WORLD_COPY_MESSAGES)
    .filter((key) => key.startsWith("settings.schema.dontstarve.") && key.endsWith(".title"))
    .map((key) => key.slice("settings.schema.dontstarve.".length, -".title".length));
  assert.deepEqual([...copyFields].sort(), [...reviewedCopyFields].sort());
  for (const [locale, catalog] of [["zh-CN", ZH_CN_MESSAGES], ["en-US", EN_US_MESSAGES]]) {
    const fields = fieldsFor(locale, catalog);
    for (const key of copyFields) {
      const field = fields.get(key);
      assert.ok(field, key);
      for (const value of [field.title, field.description]) {
        assert.ok(value?.trim(), `${locale}: ${key} is missing copy`);
        assert.doesNotMatch(value, /原生配置|Native .+ option from Klei|PALMCONETREE/u);
        assert.equal(/[\u3400-\u9fff]/u.test(value), locale === "zh-CN", `${locale}: ${key}`);
      }
    }
  }
  const zh = fieldsFor("zh-CN", ZH_CN_MESSAGES);
  assert.equal(zh.get("master_palmconetree").title, "棕榈松果树");
  assert.equal(zh.get("master_rifts_enabled").title, "月亮裂隙");
  assert.equal(zh.get("caves_rifts_enabled").title, "暗影裂隙");
  assert.match(zh.get("master_palmcone_seed_portalrate").description, /相对概率/u);
  assert.match(zh.get("caves_tree_rock_regrowth").description, /再生速度/u);
});

test("DST single-character Chinese option labels survive the real settings parser", () => {
  const zh = fieldsFor("zh-CN", ZH_CN_MESSAGES);
  assert.equal(labels(zh, "world_autumn").shortseason, "短");
  assert.equal(labels(zh, "world_autumn").longseason, "长");
  assert.equal(labels(zh, "master_palmconetree_regrowth").slow, "慢");
  assert.equal(labels(zh, "world_specialevent").none, "无");
});

test("DST extra starting resources labels describe world age without changing native values", () => {
  const zh = fieldsFor("zh-CN", ZH_CN_MESSAGES);
  const en = fieldsFor("en-US", EN_US_MESSAGES);
  assert.deepEqual(labels(zh, "world_extrastartingitems"), {
    "0": "始终发放", "5": "第 5 天后", default: "第 10 天后（默认）",
    "15": "第 15 天后", "20": "第 20 天后", none: "不发放"
  });
  assert.deepEqual(labels(en, "world_extrastartingitems"), {
    "0": "Always", "5": "After day 5", default: "After day 10 (default)",
    "15": "After day 15", "20": "After day 20", none: "Never"
  });
  assert.match(zh.get("world_extrastartingitems").description, /不是物品数量/u);
  assert.equal(zh.get("world_extrastartingitems").defaultValue, "default");
});

test("DST reused native values keep their field-specific gameplay meaning", () => {
  const zh = fieldsFor("zh-CN", ZH_CN_MESSAGES);
  assert.equal(labels(zh, "world_ghostenabled").none, "重新选择冒险家");
  assert.equal(labels(zh, "world_ghostenabled").always, "变为鬼魂");
  assert.equal(labels(zh, "world_portalresurection").always, "启用");
  assert.equal(labels(zh, "world_resettime").none, "保留世界");
  assert.equal(labels(zh, "world_resettime").always, "立即重置");
  assert.deepEqual(labels(zh, "world_lessdamagetaken"), {
    always: "较少伤害", none: "默认伤害", more: "较多伤害"
  });
  assert.deepEqual(labels(zh, "world_spawnmode"), { fixed: "绚丽之门", scatter: "随机位置" });
  assert.equal(labels(zh, "master_rifts_enabled").default, "随游戏进度解锁");
  assert.equal(labels(zh, "caves_rifts_enabled").always, "不受进度限制");
  assert.equal(labels(zh, "caves_task_set").cave_default, "洞穴生物群落");
});

test("DST cave controls follow the native world, resource, and creature groups", () => {
  const zh = fieldsFor("zh-CN", ZH_CN_MESSAGES);
  const t = (key, _params, fallback) => ZH_CN_MESSAGES[key] ?? fallback ?? key;
  const fieldGroups = new Map();
  for (const section of ["cavesgen", "cavessettings"]) {
    const fields = [...zh.values()].filter((field) => field.sectionId === section);
    const groups = dontStarveSettingsDefinition.buildFieldGroups(section, fields, "zh-CN", t);
    for (const group of groups) {
      assert.ok(group.title.trim());
      for (const field of group.fields) fieldGroups.set(field.key, group.id);
    }
  }
  assert.equal(fieldGroups.get("caves_atriumgate"), "misc");
  assert.equal(fieldGroups.get("caves_acidrain_enabled"), "misc");
  assert.equal(fieldGroups.get("caves_cavelight"), "misc");
  assert.equal(fieldGroups.get("caves_banana"), "resources");
  assert.equal(fieldGroups.get("caves_rock"), "resources");
  assert.equal(fieldGroups.get("caves_rocky"), "animals");
  assert.equal(fieldGroups.get("caves_rocky_setting"), "animals");
});
