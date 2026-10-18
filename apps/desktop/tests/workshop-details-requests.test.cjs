const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { WorkshopDetailsRequests } = require("../src/views/servers/workshop-details-requests.ts");
const item = (id, title = id) => ({ id, title, status: "resolved", item_kind: "item", localization_warning: null });

test("duplicate details share one read and reopening uses only a recent same-language success", async () => {
  let now = 0, calls = 0, complete;
  const requests = new WorkshopDetailsRequests((id, locale) => {
    calls++;
    return new Promise(resolve => { complete = () => resolve(item(id, locale)); });
  }, () => now);
  const first = requests.read("111111", "zh-CN"), duplicate = requests.read("111111", "zh-CN");
  assert.equal(first, duplicate);
  complete();
  assert.equal((await first).item.title, "zh-CN");
  assert.equal((await first).cached, false);
  const reopened = await requests.read("111111", "zh-CN");
  assert.equal(reopened.item.title, "zh-CN");
  assert.equal(reopened.cached, true, "cached display content is distinguishable from fresh verification");
  assert.equal(calls, 1);
  const english = requests.read("111111", "en-US"); complete(); await english;
  assert.equal(calls, 2);
  now = 60_000;
  const expired = requests.read("111111", "zh-CN"); complete(); await expired;
  assert.equal(calls, 3);
});

test("explicit retry invalidates cached and pending details without stale repopulation", async () => {
  const completions = [];
  const requests = new WorkshopDetailsRequests((id) => new Promise(resolve => completions.push(title => resolve(item(id, title)))));
  const old = requests.read("111111", "zh-CN");
  requests.invalidate("111111", "zh-CN");
  const fresh = requests.read("111111", "zh-CN");
  completions[1]("Fresh"); await fresh;
  completions[0]("Old"); await old;
  assert.equal((await requests.read("111111", "zh-CN")).item.title, "Fresh");
  requests.invalidate("111111", "zh-CN");
  const retried = requests.read("111111", "zh-CN");
  assert.equal(completions.length, 3);
  completions[2]("Retried"); await retried;
});

test("rate limits, collections, unverified items, localization fallbacks and wrong IDs are never successful cache hits", async () => {
  for (const response of [
    { ...item("111111"), status: "unverified" },
    { ...item("111111"), item_kind: "collection" },
    { ...item("111111"), localization_warning: "HTTP 429" },
    item("222222"), new Error("HTTP 429")
  ]) {
    let calls = 0;
    const requests = new WorkshopDetailsRequests(async () => {
      calls++;
      if (response instanceof Error) throw response;
      return response;
    });
    for (let count = 0; count < 2; count++) await requests.read("111111", "zh-CN").catch(() => {});
    assert.equal(calls, 2);
  }
});

test("the detail cache is bounded and changing the workbench clears it", async () => {
  let calls = 0;
  const requests = new WorkshopDetailsRequests(async id => { calls++; return item(id); });
  for (let index = 0; index < 33; index++) await requests.read(String(100000 + index), "zh-CN");
  await requests.read("100000", "zh-CN");
  assert.equal(calls, 34);
  requests.clear();
  await requests.read("100000", "zh-CN");
  assert.equal(calls, 35);
});
