const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { parseSource, sourceText, transpileTypeScript, visitSyntax } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}
const model = require("../src/views/servers/mod-workbench-model.ts");
const plans = require("../src/views/servers/mod-workbench-plans.ts");
const enablement = require("../src/views/servers/steam-workshop-enablement-plan.ts");
const dstPolicy = require("../src/views/servers/mod-workbench-dst-policy.ts");
const collections = require("../src/views/servers/mod-workbench-collections.ts");
const { restoreRemovedWorkshopIds } = require("../src/views/servers/mod-workbench-workshop-inventory.ts");
const filename = path.resolve(__dirname, "../src/views/servers/ModWorkbench.tsx");
const source = fs.readFileSync(filename, "utf8");
let handler;
visitSyntax(parseSource(source, filename), (node) => {
  if (node.type === "FunctionDeclaration" && node.identifier?.value === "handleInstallWorkshopItems") handler = sourceText(source, node);
});
assert.ok(handler);
const script = transpileTypeScript(`${handler}\nexports.run = handleInstallWorkshopItems;`, filename);

function harness(moduleId, settings, installedIds = [], options = {}) {
  const appId = { dontstarve: 322330, projectzomboid: 108600, palworld: 1623730, terraria: 1281930 }[moduleId];
  const ids = options.ids ?? ["111111", "222222"];
  const items = options.items ?? Object.fromEntries(ids.map(id => [id, { id, status: "resolved", item_kind: "item", consumer_app_id: appId, children: [] }]));
  const review = { ids, items, contentIds: ids, installedIds, missingIds: ids.filter(id => !installedIds.includes(id)), issues: [], searchedRoots: [] };
  const calls = { downloads: [], saves: [], errors: [], messages: [], inventory: 0 };
  const installed = new Set(installedIds);
  const latestSettingsRef = { current: structuredClone(settings) };
  const exports = {};
  const context = {
    ...model, ...plans, ...enablement, ...dstPolicy, ...collections, restoreRemovedWorkshopIds, exports, Set, Map, Error,
    moduleId, settings, latestSettingsRef, expectedAppId: appId,
    workflow: { steamDownloadMode: "steamcmd-cache" },
    modMutationsDisabled: false, modWorkflowUnsupported: false,
    lookupMap: items, props: { details: { summary: { id: "fixture-instance" } } },
    manualEnablement: moduleId === "palworld" ? { id_strategy: "palworld_package_name", setting_key: "mod_package_names" } : null,
    manualStaging: ["palworld", "terraria"].includes(moduleId) ? {} : null,
    t: (key, params, fallback) => (fallback ?? key).replace(/\{(\w+)\}/g, (_, name) => String(params?.[name] ?? "")),
    describeError: error => error.message,
    startTransition: callback => callback(),
    runModMutation: async (_kind, operation) => operation(),
    canInstallWorkshopItems: () => true,
    readWritableInstanceState: async () => ({ settings: structuredClone(latestSettingsRef.current) }),
    persistSettings: async value => { calls.saves.push(structuredClone(value)); latestSettingsRef.current = value; },
    readSteamWorkshopInstallationStatus: async (_instanceId, requested) => {
      calls.inventory++;
      return { consumer_app_id: appId, items: requested.map(item_id => ({ item_id, installed: installed.has(item_id) })) };
    },
    downloadSteamWorkshopItems: async (_instanceId, requested, missingOnly) => {
      calls.downloads.push({ ids: [...requested], missingOnly });
      options.onDownloadStarted?.();
      if (options.downloadBarrier) await options.downloadBarrier;
      if (options.failDownload) throw new Error("download failed");
      requested.forEach(id => installed.add(id));
      return { items: requested.map(item_id => ({ item_id, expected_path_exists: options.incompleteId !== item_id })) };
    },
    readManualModInventory: async () => ({ items: ids.map(id => ({ name: id, path: `fixture/Mods/Workshop/${id}`, inferred_id: `Package${id}`, item_type: "folder" })) }),
    readProjectZomboidWorkshopModsSnapshot: async () => ({ workshop_root: "fixture", workshop_root_exists: true, items: ids.map(id => ({ workshop_item_id: id, mods: options.missingMetadata ? [] : [{ mod_id: `Mod${id}`, map_ids: [`Map${id}`] }] })) }),
    setDownloadError: value => { if (value) calls.errors.push(value); },
    setInstallingWorkshopIds() {}, setDownloadState() {}, setDownloadResult() {},
    setApplyMessage: value => { if (value) calls.messages.push(value); },
    setWorkshopInstallationResult() {}, setResolvedLookupMap() {}, setManualInventory() {}, setManualInventoryError() {},
    setRetainedWorkshopIds() {}, setPzSnapshot() {}, setSelectedWorkshopIds() {}, setScanNonce() {}
  };
  vm.runInNewContext(script, context, { filename });
  return {
    calls,
    run: enable => exports.run(ids, { review, enable }),
    runDirect: () => exports.run(ids)
  };
}

test("manifest applies a fresh inventory check and delegates cache reuse for the complete list", async () => {
  const state = harness("dontstarve", { master_enabled_workshop_mod_ids: "999999" }, ["111111"]);
  assert.equal(await state.run(false), true);
  assert.equal(state.calls.inventory, 1);
  assert.deepEqual(state.calls.downloads, [{ ids: ["111111", "222222"], missingOnly: true }]);
  assert.deepEqual(state.calls.saves, [{
    master_enabled_workshop_mod_ids: "999999",
    shared_workshop_mod_ids: "111111\n222222"
  }]);
});

test("cached DST items still deploy into this instance before enabling", async () => {
  const state = harness("dontstarve", { master_enabled_workshop_mod_ids: "999999" }, ["111111", "222222"]);
  assert.equal(await state.run(true), true);
  assert.deepEqual(state.calls.downloads, [{ ids: ["111111", "222222"], missingOnly: true }]);
  assert.equal(state.calls.saves[0].master_enabled_workshop_mod_ids, "999999\n111111\n222222");
});

test("raw-owned DST shards reject install-and-enable before downloading but allow download-only", async () => {
  const settings = { master_modoverrides_lua: 'return {["workshop-111111"]={enabled=false}}' };
  const enable = harness("dontstarve", settings);
  assert.equal(await enable.run(true), false);
  assert.deepEqual(enable.calls.downloads, []);
  assert.deepEqual(enable.calls.saves, []);
  assert.match(enable.calls.errors[0], /modoverrides/);
  const download = harness("dontstarve", settings);
  assert.equal(await download.run(false), true);
  assert.equal(download.calls.downloads.length, 1);
  assert.deepEqual(download.calls.saves, [{ ...settings, shared_workshop_mod_ids: "111111\n222222" }]);
});

test("ordinary DST installs ask the backend to reuse and deploy the complete selection", async () => {
  const state = harness("dontstarve", {}, ["111111", "222222"]);
  assert.equal(await state.runDirect(), true);
  assert.deepEqual(state.calls.downloads, [{ ids: ["111111", "222222"], missingOnly: true }]);
  assert.equal(state.calls.saves.length, 1);
  assert.deepEqual(state.calls.messages, ["Deployed 2 Mod item(s) into this instance."]);
});

test("cached DST download-only lists retain ownership without enabling either shard", async () => {
  const state = harness("dontstarve", {}, ["111111", "222222"]);
  assert.equal(await state.run(false), true);
  assert.deepEqual(state.calls.downloads, [{ ids: ["111111", "222222"], missingOnly: true }]);
  assert.deepEqual(state.calls.saves, [{ shared_workshop_mod_ids: "111111\n222222" }]);
});

test("cached DST enablement waits for deployment and does not save after deployment fails", async () => {
  const started = Promise.withResolvers();
  const deployment = Promise.withResolvers();
  const state = harness("dontstarve", {}, ["111111", "222222"], {
    onDownloadStarted: started.resolve,
    downloadBarrier: deployment.promise,
    failDownload: true
  });
  const result = state.run(true);
  // Fail promptly if the workflow finishes without starting deployment.
  await Promise.race([started.promise, result]);
  assert.equal(state.calls.downloads.length, 1);
  assert.deepEqual(state.calls.saves, []);
  deployment.resolve();
  assert.equal(await result, false);
  assert.deepEqual(state.calls.saves, []);
  assert.deepEqual(state.calls.errors, ["download failed"]);
});

test("cached Palworld packages still deploy and enable their PackageNames", async () => {
  const state = harness("palworld", { mod_package_names: "Existing" }, ["111111", "222222"]);
  assert.equal(await state.run(true), true);
  assert.deepEqual(state.calls.downloads, [{ ids: ["111111", "222222"], missingOnly: true }]);
  assert.equal(state.calls.saves[0].mod_package_names, "Existing\nPackage111111\nPackage222222");
});

test("tModLoader download lists preserve internal enabled names and the selected runtime", async () => {
  const state = harness("terraria", { server_runtime: "tmodloader", tmodloader_workshop_item_ids: "999999", tmodloader_enabled_mod_names: "PreservedMod" }, ["111111", "222222"]);
  assert.equal(await state.run(false), true);
  assert.equal(state.calls.downloads.length, 1, "cached files still need instance deployment");
  assert.equal(state.calls.saves[0].tmodloader_workshop_item_ids, "999999\n111111\n222222");
  assert.equal(state.calls.saves[0].tmodloader_enabled_mod_names, "PreservedMod");
  assert.equal(state.calls.saves[0].server_runtime, "tmodloader");
});

test("PZ enables resolved internal Mod and map IDs together with WorkshopItems", async () => {
  const state = harness("projectzomboid", { workshop_items: "999999", mods: "Existing", map_name: "Muldraugh, KY" }, ["111111", "222222"]);
  assert.equal(await state.run(true), true);
  assert.equal(state.calls.saves[0].workshop_items, "999999\n111111\n222222");
  assert.equal(state.calls.saves[0].mods, "Existing\nMod111111\nMod222222");
  assert.equal(state.calls.saves[0].map_name, "Map111111\nMap222222\nMuldraugh, KY");
  assert.deepEqual(state.calls.messages, ["Cached 2 Mod item(s) locally and configured this instance."]);
});

test("download errors, incomplete results and missing PZ metadata never save enablement", async () => {
  for (const [moduleId, options] of [["dontstarve", { failDownload: true }], ["dontstarve", { incompleteId: "222222" }], ["projectzomboid", { missingMetadata: true }]]) {
    const state = harness(moduleId, {}, [], options);
    assert.equal(await state.run(true), false);
    assert.deepEqual(state.calls.saves, []);
    assert.equal(state.calls.errors.length, 1);
  }
});

test("a mixed DST collection deploys and records only server Mods, without native re-expansion", async () => {
  const root = { id: "900000", title: "Mixed", status: "resolved", item_kind: "collection", consumer_app_id: 322330,
    child_count: 2, children: [
      { id: "111111", status: "resolved", item_kind: "item", consumer_app_id: 322330, tags: [] },
      { id: "1365141672", status: "resolved", item_kind: "item", consumer_app_id: 322330, tags: ["client_only_mod"] }
    ] };
  const options = { ids: [root.id], items: { [root.id]: root } };
  const state = harness("dontstarve", { shared_workshop_collection_ids: "888888", master_mod_configuration_options: { "111111": { setting: true } } }, [], options);
  assert.equal(await state.runDirect(), true, state.calls.errors.join("\n"));
  assert.deepEqual(state.calls.downloads, [{ ids: ["111111"], missingOnly: true }]);
  const saved = state.calls.saves[0];
  assert.equal(saved.shared_workshop_mod_ids, "111111");
  assert.equal(saved.master_enabled_workshop_mod_ids, "111111");
  assert.equal(saved.caves_enabled_workshop_mod_ids, "111111");
  assert.equal(saved.shared_workshop_collection_ids, "888888");
  assert.deepEqual(saved.steam_workshop_collections, [{ id: "900000", title: "Mixed", member_ids: ["111111"] }]);
  assert.deepEqual(saved.master_mod_configuration_options, { "111111": { setting: true } });
  for (const [change, reason] of [[{ consumer_app_id: 108600 }, "wrong-game"], [{ status: "not_found" }, "missing"]]) {
    const invalid = structuredClone(root);
    Object.assign(invalid.children[1], change);
    const blocked = harness("dontstarve", {}, [], { ids: [root.id], items: { [root.id]: invalid } });
    assert.equal(await blocked.runDirect(), false);
    assert.deepEqual(blocked.calls.downloads, []);
    assert.deepEqual(blocked.calls.saves, []);
    if (blocked.calls.errors.length) assert.equal(JSON.parse(blocked.calls.errors[0]).reason, reason);
    else assert.deepEqual(blocked.calls.messages, ["This collection contains nested or unresolved entries. Add the individual Mods instead."]);
  }
});
