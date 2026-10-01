const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function fixture(transport) {
  const storage = new Map([["langame.locale", "zh-CN"]]);
  const calls = [];
  let onCall = () => {};
  const window = {
    location: { hostname: "127.0.0.1", protocol: "http:", hash: "" },
    navigator: { language: "en-US" },
    localStorage: { getItem: (key) => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value) },
    sessionStorage: { getItem: () => "controlled-fixture" }
  };
  async function dispatch(command, args) {
    calls.push({ command, args });
    onCall(command);
    return command === "lookup_steam_workshop_items" ? args.ids.map((id) => ({ id })) : [];
  }
  const loaded = new Map();
  function load(name) {
    if (loaded.has(name)) return loaded.get(name);
    const filename = path.join(__dirname, "../src", `${name}.ts`);
    const exports = {};
    const source = fs.readFileSync(filename, "utf8").replaceAll("import.meta.env.DEV", "false");
    vm.runInNewContext(transpileTypeScript(source, filename), {
      exports, window, URLSearchParams,
      fetch: async (_url, options) => {
        const { command, args } = JSON.parse(options.body);
        const value = await dispatch(command, args);
        return { ok: true, json: async () => ({ ok: true, value }) };
      },
      require(id) {
        if (id === "@tauri-apps/api/core") return { isTauri: () => transport === "tauri", invoke: dispatch };
        if (id === "./desktop-exit-lifecycle") return require("./helpers/desktop-exit-fixture.cjs")
          .loadDesktopExitModule({ isTauri: () => transport === "tauri", invoke: dispatch });
        if (["./api-transport", "./locale-preference", "./i18n-config", "./storage-management-requests"].includes(id)) return load(id.slice(2));
        throw new Error(`Unexpected dependency: ${id}`);
      }
    }, { filename });
    loaded.set(name, exports);
    return exports;
  }
  return { api: load("api"), locale: load("locale-preference"), calls, setOnCall: (callback) => { onCall = callback; } };
}

for (const transport of ["tauri", "lan"]) {
  test(`${transport}: displayed Workshop details carry the selected item and explicit interface language`, async () => {
    const { api, calls } = fixture(transport);
    await api.readSteamWorkshopItemDetails("666155465", "zh-CN");
    assert.equal(calls.at(-1).command, "read_steam_workshop_item_details");
    assert.deepEqual(JSON.parse(JSON.stringify(calls.at(-1).args)), { id: "666155465", locale: "zh-CN" });
  });
  test(`${transport}: collection file removal carries the same settings revision through its dedicated transaction`, async () => {
    const { api, calls } = fixture(transport);
    const input = { id: "fixture-instance", settings_json: '{"steam_workshop_collections":[]}' };
    const expected = '{"steam_workshop_collections":[{"id":"345678","title":"Saved","member_ids":["123456"]}]}';
    await api.updateInstance(input, expected);
    assert.equal(calls.at(-1).command, "update_instance_record_if_current");
    assert.equal(calls.at(-1).args.expectedSettingsJson, expected);
    await api.updateInstance(input, expected, { collectionId: "345678", memberIds: ["123456"] });
    assert.equal(calls.at(-1).command, "remove_instance_workshop_collection");
    assert.deepEqual(calls.at(-1).args.input, input);
    assert.equal(calls.at(-1).args.expectedSettingsJson, expected);
    assert.equal(calls.at(-1).args.collectionId, "345678");
    assert.deepEqual([...calls.at(-1).args.memberIds], ["123456"]);
  });

  test(`${transport}: Workshop collection browsing carries an explicit kind without changing the default`, async () => {
    const { api, calls } = fixture(transport);
    await api.searchSteamWorkshopItems(322330, "地图");
    assert.equal(calls.at(-1).args.browseKind, "item");
    await api.searchSteamWorkshopItems(322330, "地图", "recent", 2, "zh-CN", "collection");
    const args = calls.at(-1).args;
    assert.equal(args.browseKind, "collection");
    assert.equal(args.browse_kind, "collection");
    assert.equal(args.query, "地图");
    assert.equal(args.sort, "recent");
    assert.equal(args.page, 2);
  });

  test(`${transport}: a single member removal explicitly retains its saved collection snapshot`, async () => {
    const { api, calls } = fixture(transport);
    const settings = '{"steam_workshop_collections":[{"id":"345678","title":"Saved","member_ids":["123456"]}]}';
    const input = { id: "fixture-instance", settings_json: settings };
    await api.updateInstance(input, settings, { collectionId: "345678", memberIds: ["123456"], retainCollection: true });
    assert.equal(calls.at(-1).command, "remove_instance_workshop_collection");
    assert.deepEqual(JSON.parse(JSON.stringify(calls.at(-1).args)), {
      input, expectedSettingsJson: settings, collectionId: "345678", memberIds: ["123456"], retainCollection: true
    });
  });

  test(`${transport}: new public requests immediately follow the selected language`, async () => {
    const { api, locale, calls } = fixture(transport);
    for (const language of ["zh-CN", "en-US", "zh-CN"]) {
      locale.writePreferredLocale(language);
      await api.lookupSteamWorkshopItems(["3766499200"]);
      await api.fetchSteamNewsForApp(322330);
      await api.downloadSteamWorkshopItems("fixture-instance", ["3766499200"], true);
      await api.searchSteamWorkshopItems(322330, "地图");
      assert.deepEqual(calls.slice(-4).map((call) => call.args.locale), Array(4).fill(language));
      assert.equal(calls.at(-2).args.missingOnly, true);
    }
    await api.searchSteamWorkshopItems(322330, "map", "relevance", 1, "en-US");
    assert.equal(calls.at(-1).args.locale, "en-US", "an explicitly supplied request locale remains authoritative");
  });

  test(`${transport}: an in-flight Workshop batch retains its locale while the next request uses the new language`, async () => {
    const { api, locale, calls, setOnCall } = fixture(transport);
    setOnCall(() => locale.writePreferredLocale("en-US"));
    const ids = Array.from({ length: 130 }, (_, index) => String(1000000 + index));
    const items = await api.lookupSteamWorkshopItems(ids);
    assert.equal(items.length, ids.length);
    assert.deepEqual(calls.map((call) => call.args.ids.length), [64, 64, 2]);
    assert.deepEqual(calls.map((call) => call.args.locale), ["zh-CN", "zh-CN", "zh-CN"]);
    await api.lookupSteamWorkshopItems(["3766499200"]);
    assert.equal(calls.at(-1).args.locale, "en-US");
  });

  test(`${transport}: news, world imports and backup restores retain the request language`, async () => {
    const { api, locale, calls } = fixture(transport);
    for (const language of ["zh-CN", "en-US"]) {
      locale.writePreferredLocale(language);
      await api.importDontStarveWorldData("fixture-instance", "D:/fixture/world");
      await api.restoreInstanceBackup("fixture-instance", "fixture-backup");
      assert.deepEqual(calls.slice(-2).map((call) => call.args.locale), [language, language]);
    }
    await api.fetchSteamNewsForApp(322330, 3, "zh-CN");
    assert.equal(calls.at(-1).args.locale, "zh-CN", "news uses the component's captured language");
    await api.importDontStarveWorldData("fixture-instance", "D:/fixture/world", "zh-CN");
    await api.restoreInstanceBackup("fixture-instance", "fixture-backup", "zh-CN");
    assert.deepEqual(calls.slice(-2).map((call) => call.args.locale), ["zh-CN", "zh-CN"]);
  });

  test(`${transport}: media registration carries only the scoped source, media kind and language`, async () => {
    const { api, calls } = fixture(transport);
    const source = "https://video.akamai.steamstatic.com/store_trailers/fixture/main.m3u8?t=3";
    await api.registerMediaCacheSource(source, "hls", "en-US");
    assert.equal(calls.at(-1).command, "register_media_cache_source");
    assert.deepEqual(JSON.parse(JSON.stringify(calls.at(-1).args)), { url: source, kind: "hls", locale: "en-US" });
  });
}
