const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const { loadStorageManagementRequests } = require("./helpers/storage-management-requests.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { WorkshopBrowseController } = require("../src/views/servers/workshop-browse-controller.ts");
const request = (query = "", page = 1) => ({ appId: 322330, query, page, sort: query ? "relevance" : "trend" });
const result = (query = "", page = 1, browseKind = "item") => ({ app_id: 322330, browse_kind: browseKind, query, page, items: [], has_more: page < 2, total_count: 31, page_size: 30 });
const flush = async () => { await Promise.resolve(); await Promise.resolve(); };

test("typing is debounced and only the latest pending search follows an active request", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const calls = [], states = [], completions = [];
  const controller = new WorkshopBrowseController((input) => {
    calls.push(input);
    return new Promise((resolve) => completions.push(resolve));
  }, (state) => states.push(state));
  t.after(() => controller.dispose());
  controller.request(request("g"), 350);
  t.mock.timers.tick(200);
  controller.request(request("global"), 350);
  t.mock.timers.tick(349);
  assert.equal(calls.length, 0);
  t.mock.timers.tick(1);
  assert.equal(calls[0].query, "global");
  controller.request(request("global positions"), 350);
  t.mock.timers.tick(350);
  controller.request(request("地图"), 350);
  t.mock.timers.tick(350);
  assert.equal(calls.length, 1, "no overlapping network requests");
  completions.shift()(result("global"));
  await flush();
  assert.equal(calls.length, 2);
  assert.equal(calls[1].query, "地图");
  assert.equal(states.at(-1).loading, true, "old results cannot replace the current loading state");
  completions.shift()(result("地图"));
  await flush();
  assert.equal(states.at(-1).result.query, "地图");
  assert.equal(states.at(-1).loading, false);
});

test("pagination preserves the last successful page on failure and retry fetches the requested page", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let rejectNext = false, calls = 0;
  const states = [];
  const controller = new WorkshopBrowseController(async (input) => {
    calls++;
    if (rejectNext) throw new Error("Steam unavailable");
    return result(input.query, input.page);
  }, (state) => states.push(state));
  t.after(() => controller.dispose());
  controller.request(request()); t.mock.timers.tick(0); await flush();
  rejectNext = true;
  controller.request(request("", 2));
  assert.equal(states.at(-1).loading, true);
  t.mock.timers.tick(0); await flush();
  assert.equal(states.at(-1).result.page, 1);
  assert.equal(states.at(-1).error, "Steam unavailable");
  rejectNext = false;
  controller.request(request("", 2), 0, true); t.mock.timers.tick(0); await flush();
  assert.equal(states.at(-1).result.page, 2);
  assert.equal(states.at(-1).result.has_more, false);
  controller.request(request());
  assert.equal(states.at(-1).result.page, 1);
  assert.equal(calls, 3, "returning to a recent page uses its cache");
});

test("disposed work cannot publish late results and a different game does not retain the previous catalog", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const states = [];
  let complete;
  const controller = new WorkshopBrowseController(() => new Promise((resolve) => { complete = resolve; }), (state) => states.push(state));
  controller.request(request()); t.mock.timers.tick(0);
  controller.dispose();
  complete(result()); await flush();
  assert.equal(states.length, 1);
  assert.equal(states[0].result, null);
});

test("locale changes clear the prior language immediately and failed requests cannot restore it", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const states = [], calls = [];
  let failChinese = true;
  const controller = new WorkshopBrowseController(async (input) => {
    calls.push(input);
    if (input.locale === "zh-CN" && failChinese) throw new Error("Chinese request failed");
    return { ...result(), items: [{ id: "1000001", title: input.locale }] };
  }, state => states.push(state));
  t.after(() => controller.dispose());
  controller.request({ ...request(), locale: "en-US" }); t.mock.timers.tick(0); await flush();
  assert.equal(states.at(-1).result.items[0].title, "en-US");
  controller.request({ ...request(), locale: "zh-CN" });
  assert.equal(states.at(-1).locale, "zh-CN");
  assert.equal(states.at(-1).result, null);
  t.mock.timers.tick(0); await flush();
  assert.equal(states.at(-1).result, null);
  assert.equal(states.at(-1).error, "Chinese request failed");
  failChinese = false;
  controller.request({ ...request(), locale: "zh-CN" }, 0, true); t.mock.timers.tick(0); await flush();
  assert.equal(states.at(-1).result.items[0].title, "zh-CN");
  controller.request({ ...request(), locale: "en-US" });
  assert.equal(states.at(-1).result.items[0].title, "en-US");
  assert.equal(states.at(-1).locale, "en-US");
  assert.equal(calls.length, 3, "a matching-language cached page remains reusable");
});

test("a late previous-language page cannot replace the active language", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const states = [], calls = [], completions = [];
  const controller = new WorkshopBrowseController(input => {
    calls.push(input);
    return new Promise(resolve => completions.push(resolve));
  }, state => states.push(state));
  t.after(() => controller.dispose());
  controller.request({ ...request(), locale: "en-US" }); t.mock.timers.tick(0);
  controller.request({ ...request(), locale: "zh-CN" }); t.mock.timers.tick(0);
  completions.shift()({ ...result(), items: [{ title: "English" }] }); await flush();
  assert.equal(states.at(-1).result, null);
  assert.equal(states.at(-1).locale, "zh-CN");
  assert.equal(calls.at(-1).locale, "zh-CN");
  completions.shift()({ ...result(), items: [{ title: "中文" }] }); await flush();
  assert.equal(states.at(-1).result.items[0].title, "中文");
});

test("switching between Mods and collections clears the old kind and keeps separate page caches", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const states = [], calls = [];
  const controller = new WorkshopBrowseController(async (input) => {
    calls.push(input);
    return result(input.query, input.page, input.browseKind ?? "item");
  }, (state) => states.push(state));
  t.after(() => controller.dispose());
  controller.request(request()); t.mock.timers.tick(0); await flush();
  assert.equal(states.at(-1).result.browse_kind, "item");
  controller.request({ ...request(), browseKind: "collection" });
  assert.equal(states.at(-1).result, null, "Mods cannot appear under a collection filter while loading");
  t.mock.timers.tick(0); await flush();
  assert.equal(states.at(-1).result.browse_kind, "collection");
  controller.request({ ...request("", 2), browseKind: "collection" });
  t.mock.timers.tick(0); await flush();
  assert.equal(states.at(-1).result.page, 2);
  controller.request(request());
  assert.equal(states.at(-1).result.browse_kind, "item");
  assert.equal(states.at(-1).result.page, 1);
  controller.request({ ...request(), browseKind: "collection" });
  assert.equal(states.at(-1).result.browse_kind, "collection");
  assert.equal(calls.length, 3, "each kind/page has its own cache entry");
});

test("the catalog mock filters collection results before counting and paging", () => {
  const { searchMockWorkshopItems } = require("../src/api-mock/catalogs.ts");
  const mods = searchMockWorkshopItems(322330, "", "trend", 1);
  const collections = searchMockWorkshopItems(322330, "", "trend", 1, "collection");
  assert.equal(mods.browse_kind, "item");
  assert.ok(mods.items.length > 0 && mods.items.every((item) => item.item_kind === "item"));
  assert.equal(collections.browse_kind, "collection");
  assert.ok(collections.items.length > 0 && collections.items.every((item) => item.item_kind === "collection"));
  assert.equal(collections.total_count, collections.items.length);
  assert.match(collections.source_url, /section=collections/);
  assert.equal(searchMockWorkshopItems(322330, "", "trend", 2, "collection").items.length, 0);
});

for (const [name, invalidResult] of [
  ["missing content kind", () => { const value = result(); delete value.browse_kind; return value; }],
  ["wrong content kind", () => result("", 1, "collection")],
  ["wrong game", () => ({ ...result(), app_id: 108600 })],
  ["missing item list", () => ({ ...result(), items: null })],
  ["missing response", () => null]
]) {
  test(`an invalid browse response (${name}) fails explicitly and cannot poison the page cache`, async (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const states = [];
    let valid = false, calls = 0;
    const controller = new WorkshopBrowseController(async () => {
      calls++;
      return valid ? result() : invalidResult();
    }, (state) => states.push(state));
    t.after(() => controller.dispose());
    controller.request(request()); t.mock.timers.tick(0); await flush();
    assert.equal(states.at(-1).loading, false);
    assert.equal(states.at(-1).result, null);
    assert.equal(JSON.parse(states.at(-1).error).code, "steam_workshop_browse_invalid_response");
    valid = true;
    controller.request(request()); t.mock.timers.tick(0); await flush();
    assert.equal(calls, 2, "an invalid result is never cached as a successful empty page");
    assert.equal(states.at(-1).error, null);
    assert.equal(states.at(-1).result.browse_kind, "item");
  });
}


test("the public API batches large Mod libraries without exceeding the backend ID limit", async () => {
  const vm = require("node:vm");
  const filename = path.join(__dirname, "../src/api.ts");
  const calls = [];
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports,
    require(id) {
      if (id === "./locale-preference") return { readPreferredLocale: () => "zh-CN" };
      if (id === "./storage-management-requests") return loadStorageManagementRequests({ readPreferredLocale: () => "zh-CN" });
      if (id === "@tauri-apps/api/core") return { isTauri: () => true };
      assert.equal(id, "./api-transport");
      return {
        invokeOrMock: async (command, args) => {
          calls.push({ command, ids: Array.from(args.ids) });
          return args.ids.map((id) => ({ id }));
        }
      };
    }
  }, { filename });
  const ids = Array.from({ length: 130 }, (_, index) => String(1000000 + index));
  const items = await exports.lookupSteamWorkshopItems([...ids, " 1000000 "]);
  assert.deepEqual(calls.map((call) => call.ids.length), [64, 64, 2]);
  assert.deepEqual(Array.from(items, (item) => item.id), ids);
});
