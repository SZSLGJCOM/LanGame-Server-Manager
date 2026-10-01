const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const { loadStorageManagementRequests } = require("./helpers/storage-management-requests.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { composeWorkshopLookups, rememberWorkshopPresentation } = require("../src/views/servers/workshop-presentation.ts");
const item = (id = "111111") => ({ id, title: "Author title", description: "Author description", item_kind: "item", status: "resolved",
  consumer_app_id: 322330, detail_url: `https://steamcommunity.com/sharedfiles/filedetails/?id=${id}`, tags: [], child_count: 0, children: [] });

test("batch metadata cannot replace localized browse text, but remains authoritative for verification", () => {
  const summary = { ...item(), title: "中文标题", description: undefined, description_excerpt: "中文摘要" };
  const metadata = { ...item(), status: "unverified", tags: ["server_only_mod"] };
  const result = composeWorkshopLookups([summary], { [metadata.id]: metadata }, {});
  assert.equal(result[metadata.id].title, "中文标题");
  assert.equal(result[metadata.id].description, undefined);
  assert.equal(result[metadata.id].description_excerpt, "中文摘要");
  assert.equal(result[metadata.id].status, "unverified");
  assert.deepEqual(result[metadata.id].tags, metadata.tags);
});

test("localized details survive later metadata refreshes without changing members or verification", () => {
  const localized = { ...item(), title: "中文标题", description: "中文完整说明", children: [{ ...item("222222"), title: "成员名称" }] };
  const cache = rememberWorkshopPresentation({}, localized);
  const metadata = { ...item(), status: "not_found", children: [{ ...item("222222"), tags: ["client_only_mod"] }, item("333333")] };
  const result = composeWorkshopLookups([], { [metadata.id]: metadata }, cache)[metadata.id];
  assert.equal(result.title, "中文标题");
  assert.equal(result.description, "中文完整说明");
  assert.equal(result.status, "not_found");
  assert.equal(result.children[0].title, "成员名称");
  assert.deepEqual(result.children[0].tags, ["client_only_mod"]);
  assert.deepEqual(result.children.map((child) => child.id), ["222222", "333333"]);
  assert.deepEqual(composeWorkshopLookups([], {}, cache), {});
});

test("author fallback text stays unchanged and the display cache stays bounded for numeric IDs", () => {
  let cache = {};
  for (let index = 0; index < 100; index++) cache = rememberWorkshopPresentation(cache, item(String(1000000 + index)));
  cache = rememberWorkshopPresentation(cache, item("111111"));
  assert.equal(Object.keys(cache).length, 64);
  assert.equal(cache["111111"].title, "Author title");
  assert.equal(cache["111111"].description, "Author description");
});

test("original-text warnings survive metadata refreshes and disappear after localized retry", () => {
  const warning = JSON.stringify({ code: "steam_workshop_network_failed", stage: "details", reason: "timeout" });
  const fallback = { ...item(), localization_warning: warning };
  const cache = rememberWorkshopPresentation({}, fallback);
  const metadata = { ...item(), title: "Refreshed canonical title", status: "unverified" };
  const result = composeWorkshopLookups([], { [metadata.id]: metadata }, cache)[metadata.id];
  assert.equal(result.title, fallback.title);
  assert.equal(result.localization_warning, warning);
  assert.equal(result.status, "unverified");
  const localized = { ...item(), title: "中文标题", description: "中文详情", localization_warning: null };
  const retried = composeWorkshopLookups([], { [metadata.id]: metadata }, rememberWorkshopPresentation(cache, localized))[metadata.id];
  assert.equal(retried.title, localized.title);
  assert.equal(retried.localization_warning, null);
  assert.equal(retried.status, "unverified");
});

test("single details and batched lookups dispatch the caller's locale even if global storage changes", async () => {
  const filename = path.join(__dirname, "../src/api.ts"), exports = {}, calls = [];
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports, require(id) {
      if (id === "./locale-preference") return { readPreferredLocale: () => "en-US" };
      if (id === "./storage-management-requests") return loadStorageManagementRequests({ readPreferredLocale: () => "en-US" });
      if (id === "@tauri-apps/api/core") return { isTauri: () => true };
      assert.equal(id, "./api-transport");
      return { invokeOrMock: async (command, args) => {
        calls.push({ command, locale: args.locale });
        return command === "lookup_steam_workshop_items" ? args.ids.map((id) => item(id)) : item(args.id);
      } };
    }
  }, { filename });
  await exports.lookupSteamWorkshopItems(Array.from({ length: 65 }, (_, index) => String(1000000 + index)), "zh-CN");
  await exports.readSteamWorkshopItemDetails("111111", "zh-CN");
  await exports.readSteamWorkshopItemDetails("111111");
  assert.deepEqual(calls.map((call) => call.locale), ["zh-CN", "zh-CN", "zh-CN", "en-US"]);
  assert.equal(calls[2].command, "read_steam_workshop_item_details");
});
