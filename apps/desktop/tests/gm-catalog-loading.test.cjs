const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const sourceRoot = path.join(__dirname, "..", "src");

function evaluateSource(relativePath, dependencies, RegExpConstructor = RegExp) {
  const filename = path.join(sourceRoot, relativePath);
  const source = fs.readFileSync(filename, "utf8");
  const module = { exports: {} };
  new Function("module", "exports", "require", "RegExp", transpileTypeScript(source, filename))(
    module,
    module.exports,
    (request) => {
      assert.ok(Object.hasOwn(dependencies, request), `unexpected dependency ${request}`);
      return dependencies[request];
    },
    RegExpConstructor
  );
  return module.exports;
}

function catalogFixture(filename) {
  const localized = new Set();
  let calls = 0;
  const aliases = (option) => {
    calls++;
    assert.ok(!localized.has(option), "a catalog option was localized more than once");
    localized.add(option);
    return "本地化目录匹配 分类";
  };
  const catalog = evaluateSource(`views/servers/${filename}.ts`, {
    "./gm-catalog-i18n": {
      arkCatalogSearchAliases: aliases,
      dstPrefabSearchAliases: aliases
    }
  });
  return { catalog, calls: () => calls };
}

for (const [file, optionsName, searchName] of [
  ["ark-gm-creature-catalog", "ARK_GM_CREATURE_OPTIONS", "searchArkGmCreatureOptions"],
  ["dst-gm-item-catalog", "DST_GM_ITEM_OPTIONS", "searchDstGmItemOptions"]
]) {
  test(`${file} leaves the default picker ready without translating its search index`, () => {
    const fixture = catalogFixture(file);
    const options = fixture.catalog[optionsName];
    const search = fixture.catalog[searchName];
    assert.equal(fixture.calls(), 0, "importing the catalog must not translate every option");
    assert.deepEqual(search("", 8), options.slice(0, 8));
    assert.deepEqual(search("   ", 3), options.slice(0, 3));
    assert.equal(fixture.calls(), 0, "opening an unfiltered picker must not build the index");
    assert.deepEqual(search("本地化目录匹配", 5), options.slice(0, 5));
    assert.equal(fixture.calls(), options.length);
    search("another search");
    search("本地化目录匹配");
    assert.equal(fixture.calls(), options.length, "later queries must reuse the fixed catalog index");
  });
}

test("ARK item modes build only the requested search index and reuse each localized option", () => {
  const fixture = catalogFixture("ark-gm-item-catalog");
  const {
    ARK_GM_ITEM_OPTIONS: items,
    ARK_GM_NUMERIC_ITEM_OPTIONS: numericItems,
    searchArkGmItemOptions: searchItems,
    searchArkGmNumericItemOptions: searchNumericItems
  } = fixture.catalog;
  assert.equal(fixture.calls(), 0);
  assert.deepEqual(searchItems("", 8), items.slice(0, 8));
  assert.deepEqual(searchNumericItems("   ", 3), numericItems.slice(0, 3));
  assert.equal(fixture.calls(), 0);
  assert.deepEqual(searchNumericItems("本地化目录匹配", 5), numericItems.slice(0, 5));
  assert.equal(fixture.calls(), numericItems.length, "numeric search must not initialize the blueprint index");
  searchNumericItems("Stone");
  assert.equal(fixture.calls(), numericItems.length);
  assert.deepEqual(searchItems("本地化目录匹配", 5), items.slice(0, 5));
  assert.equal(fixture.calls(), numericItems.length + items.length);
  searchItems("Stone");
  searchNumericItems("石头");
  assert.equal(fixture.calls(), numericItems.length + items.length);
});

test("catalog translations reuse compiled rules without changing repeated localized output", () => {
  let regexConstructions = 0;
  const CountingRegExp = new Proxy(RegExp, {
    construct(target, argumentsList) {
      regexConstructions++;
      return Reflect.construct(target, argumentsList);
    }
  });
  const config = evaluateSource("i18n-config.ts", {});
  const translations = evaluateSource("views/servers/gm-catalog-i18n.ts", {
    "../../i18n-config": config
  }, CountingRegExp);
  const initializedRules = regexConstructions;
  for (let repeat = 0; repeat < 3; repeat++) {
    assert.equal(translations.localizeArkCatalogName({ name: "Absorbent Substrate", category: "Resource" }, "zh-CN"), "吸附基质");
    assert.equal(translations.localizeArkCatalogName({ name: "Tusoteuthis Oil", category: "Resource" }, "en-US"), "Tusoteuthis Oil");
    assert.equal(translations.localizeArkCatalogName({ name: "Oil (Tusoteuthis)", category: "Resource" }, "zh-CN"), "油（托斯特巨鱿）");
    assert.equal(translations.localizeArkCatalogName({ name: "Aberrant Dung Beetle", category: "Official" }, "zh-CN"), "畸变粪甲虫");
    assert.equal(translations.localizeDstPrefabName({ value: "asparagus_oversized_waxed", label: "农业 / asparagus_oversized_waxed / asparagus_oversized_waxed", category: "农业" }, "zh-CN"), "芦笋巨型上蜡");
  }
  assert.ok(initializedRules > 0);
  assert.equal(regexConstructions, initializedRules, "translating names must not recompile the term dictionary");
});
