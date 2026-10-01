const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}

const { MOD_WORKFLOW_CATALOG } = require("../src/views/servers/mod-workbench-capability.ts");
const { buildDownloadableSteamIds } = require("../src/views/servers/mod-workbench-model.ts");
const { buildModSettingsApplyPlan } = require("../src/views/servers/mod-workbench-plans.ts");
const { collectInstalledWorkshopCollections, mergeManagedWorkshopCollections } = require("../src/views/servers/mod-workbench-collections.ts");

test("DST installation downloads Workshop content before retaining native shard enablement", () => {
  assert.equal(MOD_WORKFLOW_CATALOG.dontstarve.steamDownloadMode, "steamcmd-cache");
  const manifest = fs.readFileSync(path.resolve(__dirname, "../../../modules/dontstarve/module.toml"), "utf8");
  assert.match(manifest, /\[workshop\][\s\S]*?consumer_app_id\s*=\s*322330/);

  const lookup = {
    "378160973": {
      id: "378160973",
      status: "resolved",
      item_kind: "item",
      consumer_app_id: 322330,
      children: []
    }
  };
  const ids = buildDownloadableSteamIds(["378160973"], lookup, 322330);
  assert.deepEqual(ids, ["378160973"]);

  const settings = {
    shared_workshop_mod_ids: "123456789",
    master_enabled_workshop_mod_ids: "123456789",
    caves_enabled_workshop_mod_ids: "",
    master_mod_configuration_options: { "123456789": { enabled: true } },
    caves_mod_configuration_options: { "123456789": { enabled: false } }
  };
  const plan = buildModSettingsApplyPlan("dontstarve", settings, ids, lookup, 322330);
  assert.equal(plan.canApply, true);
  assert.equal(plan.nextSettings.shared_workshop_mod_ids, "123456789\n378160973");
  assert.equal(plan.nextSettings.master_enabled_workshop_mod_ids, "123456789\n378160973");
  assert.equal(plan.nextSettings.caves_enabled_workshop_mod_ids, "378160973");
  assert.deepEqual(plan.nextSettings.master_mod_configuration_options, settings.master_mod_configuration_options);
  assert.deepEqual(plan.nextSettings.caves_mod_configuration_options, settings.caves_mod_configuration_options);
  assert.equal(settings.shared_workshop_mod_ids, "123456789");
});

test("DST collection downloads and owns only server children while preserving explicit native requests", () => {
  const lookup = {
    "900000000": {
      id: "900000000",
      status: "resolved",
      item_kind: "collection",
      consumer_app_id: 322330,
      child_count: 3,
      children: [
        { id: "378160973", item_kind: "item", status: "resolved", consumer_app_id: 322330 },
        { id: "378160973", item_kind: "item", status: "resolved", consumer_app_id: 322330 },
        { id: "1365141672", item_kind: "item", status: "resolved", consumer_app_id: 322330, tags: ["client_only_mod"] }
      ]
    }
  };
  assert.deepEqual(buildDownloadableSteamIds(["900000000"], lookup, 322330), ["378160973"]);
  const settings = { shared_workshop_collection_ids: "999999999" };
  const plan = buildModSettingsApplyPlan("dontstarve", settings, ["900000000"], lookup, 322330);
  assert.equal(plan.canApply, true);
  assert.equal(plan.nextSettings.shared_workshop_collection_ids, "999999999");
  assert.equal(plan.nextSettings.shared_workshop_mod_ids, "378160973");
  assert.equal(plan.nextSettings.master_enabled_workshop_mod_ids, "378160973");
  assert.equal(plan.nextSettings.caves_enabled_workshop_mod_ids, "378160973");
  const managed = mergeManagedWorkshopCollections(plan.nextSettings,
    collectInstalledWorkshopCollections(["900000000"], lookup, 322330));
  assert.deepEqual(managed.steam_workshop_collections, [{ id: "900000000", title: "900000000", member_ids: ["378160973"] }]);
  const badLookup = { "900000000": { ...lookup["900000000"], children: [
    lookup["900000000"].children[0],
    { id: "111111111", item_kind: "item", status: "resolved", consumer_app_id: 108600, tags: ["client_only_mod"] }
  ], child_count: 2 } };
  assert.deepEqual(buildDownloadableSteamIds(["900000000"], badLookup, 322330), []);
  const blocked = buildModSettingsApplyPlan("dontstarve", settings, ["900000000"], badLookup, 322330);
  assert.equal(blocked.canApply, false);
  assert.equal(blocked.nextSettings, null);
  assert.throws(() => collectInstalledWorkshopCollections(["900000000"], badLookup, 322330),
    { reason: "wrong-game", item_id: "111111111" });
  assert.deepEqual(settings, { shared_workshop_collection_ids: "999999999" });
});

test("DST collections with all children returned still reject guides or unsupported children as a whole", () => {
  for (const child of [
    { id: "441378551", item_kind: "guide", status: "unsupported", consumer_app_id: 322330 },
    { id: "441378551", item_kind: "item", status: "unsupported", consumer_app_id: 322330 }
  ]) {
    const lookup = {
      "900000000": {
        id: "900000000", status: "resolved", item_kind: "collection", consumer_app_id: 322330,
        child_count: 2,
        children: [{ id: "378160973", item_kind: "item", status: "resolved", consumer_app_id: 322330 }, child]
      }
    };
    const settings = {
      shared_workshop_mod_ids: "123456789",
      master_enabled_workshop_mod_ids: "123456789",
      master_mod_configuration_options: { "123456789": { enabled: true } }
    };
    const snapshot = structuredClone(settings);
    assert.deepEqual(buildDownloadableSteamIds(["900000000"], lookup, 322330), [],
      "an unsupported child must not produce a partial collection download");
    const plan = buildModSettingsApplyPlan("dontstarve", settings, ["900000000"], lookup, 322330);
    assert.equal(plan.canApply, false);
    assert.equal(plan.nextSettings, null, "an incomplete collection must not produce settings to save");
    assert.equal(plan.summaryKey, "servers.mods.incompleteCollection");
    assert.deepEqual(plan.addedIds, []);
    assert.deepEqual(settings, snapshot);
  }
});

test("guides and client-only DST Mods cannot be downloaded or enabled as server Mods", () => {
  const lookup = {
    "441378551": { id: "441378551", consumer_app_id: 322330, status: "unsupported", item_kind: "guide", children: [] },
    "3734727477": { id: "3734727477", consumer_app_id: 322330, status: "resolved", item_kind: "item", tags: ["client_only_mod"], children: [] }
  };
  assert.deepEqual(buildDownloadableSteamIds(Object.keys(lookup), lookup, 322330), []);
  const settings = { master_enabled_workshop_mod_ids: "378160973" };
  const plan = buildModSettingsApplyPlan("dontstarve", settings, Object.keys(lookup), lookup, 322330);
  assert.equal(plan.canApply, false);
  assert.equal(plan.nextSettings, null);
  assert.deepEqual(settings, { master_enabled_workshop_mod_ids: "378160973" });
});
