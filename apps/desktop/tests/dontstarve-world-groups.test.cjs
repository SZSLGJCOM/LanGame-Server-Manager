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
const { dontStarveSettingsDefinition: definition } = require("../src/views/settings/modules/dontstarve.ts");
const schemaJson = fs.readFileSync(path.resolve(__dirname, "../../../modules/dontstarve/schema.json"), "utf8");
const t = (key, _params, fallback) => fallback ?? key;
const schema = parseGuidedSettingsSchema({ summary: { id: "dontstarve", name: "DST" }, schema_json: schemaJson }, "en-US", t);
assert.equal(schema.parseError, null);
const pageFields = (section) => schema.fields.filter((field) => field.sectionId === section);
const groupsFor = (section, fields = pageFields(section)) => definition.buildFieldGroups(section, fields, "en-US", t);

test("DST environmental hazards and giants use their official groups", () => {
  for (const [section, key, groupId] of [
    ["mastersettings", "master_lightning", "misc"],
    ["mastersettings", "master_meteorshowers", "misc"],
    ["mastersettings", "master_wildfires", "misc"],
    ["mastersettings", "master_petrification", "misc"],
    ["mastersettings", "master_antliontribute", "giants"],
    ["mastergen", "master_meteorspawner", "resources"]
  ]) {
    const group = groupsFor(section).find((entry) => entry.fields.some((field) => field.key === key));
    assert.equal(group?.id, groupId, key);
  }
});

test("DST grouping preserves every field, enum and source binding without mutating input", () => {
  for (const section of ["mastergen", "mastersettings", "cavesgen", "cavessettings"]) {
    const fields = pageFields(section);
    const snapshot = structuredClone(fields);
    const grouped = groupsFor(section, fields).flatMap((group) => group.fields);
    assert.deepEqual(grouped.map((field) => field.key).sort(), fields.map((field) => field.key).sort());
    assert.equal(new Set(grouped.map((field) => field.key)).size, fields.length);
    for (const field of grouped) assert.ok(Object.is(field, fields.find((input) => input.key === field.key)));
    assert.deepEqual(fields, snapshot);
  }
});

test("DST uses the native category order regardless of schema field order", () => {
  const expected = {
    mastergen: ["presets", "global", "misc", "resources", "animals", "monsters"],
    mastersettings: ["presets", "global", "events", "survivors", "misc", "resources", "portal_resources", "animals", "monsters", "giants", "lunar_mutations"],
    cavesgen: ["presets", "misc", "resources", "animals", "monsters"],
    cavessettings: ["presets", "misc", "resources", "animals", "monsters", "giants", "lunar_mutations"]
  };
  for (const [section, ids] of Object.entries(expected)) {
    const groups = groupsFor(section);
    assert.deepEqual(groups.map((group) => group.id), ids, section);
    assert.deepEqual(groupsFor(section, [...pageFields(section)].reverse()), groups);
  }
});

test("DST classification follows native source bindings and keeps unknown options visible", () => {
  const lightning = pageFields("mastersettings").find((field) => field.key === "master_lightning");
  const renamed = { ...lightning, key: "renamed_weather_control" };
  const unknown = { ...lightning, key: "master_new_spider_weather", sourceKey: "overrides.new_weather" };
  const groups = groupsFor("mastersettings", [unknown, renamed]);
  assert.deepEqual(groups.map((group) => group.id), ["misc", "other"]);
  assert.equal(groups[0].fields[0].key, renamed.key);
  assert.equal(groups[1].fields[0].key, unknown.key);
});

test("every native world binding is rendered in its verified category", () => {
  const inventory = require("../../../modules/dontstarve/world-options.json");
  for (const [section, location, category, master] of [
    ["mastergen", "forest", "worldgen", true], ["mastersettings", "forest", "settings", true],
    ["cavesgen", "cave", "worldgen", false], ["cavessettings", "cave", "settings", false]
  ]) {
    const expected = inventory.options.filter((option) => option.category === category && option.locations.includes(location) && (master || !option.masterControlled));
    const groups = groupsFor(section);
    const rendered = groups.flatMap((group) => group.fields.filter((field) => field.sourceKey?.startsWith("overrides.")));
    assert.equal(rendered.length, expected.length);
    for (const option of expected) {
      const group = groups.find((entry) => entry.fields.some((field) => field.sourceKey === `overrides.${option.key}`));
      assert.equal(group?.id, option.group, `${section}:${option.key}`);
    }
  }
});

test("every native category has Chinese and English copy", () => {
  const inventory = require("../../../modules/dontstarve/world-options.json");
  const { EN_US_DONT_STARVE_MESSAGES: en } = require("../src/i18n/games/dontstarve.en.ts");
  const { ZH_CN_DONT_STARVE_MESSAGES: zh } = require("../src/i18n/games/dontstarve.zh-cn.ts");
  for (const [category, groups] of Object.entries(inventory.groups)) {
    for (const group of groups) {
      const key = `dst.settings.worldGroups.${category}.${group.id}`;
      assert.ok(typeof en[key] === "string" && en[key].length > 0, key);
      assert.match(zh[key], /[\u3400-\u9fff]/u, key);
    }
  }
  const translator = (key, _params, fallback) => zh[key] ?? fallback ?? key;
  const titles = definition.buildFieldGroups("mastersettings", pageFields("mastersettings"), "zh-CN", translator).map((group) => group.title);
  assert.deepEqual(titles, ["预设", "全局", "活动", "冒险家", "世界", "资源再生", "非自然传送门资源", "生物", "敌对生物", "巨兽", "月亮变异"]);
});
