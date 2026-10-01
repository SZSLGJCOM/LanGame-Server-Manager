const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    const outputText = transpileTypeScript(source, filename);
    module._compile(outputText, filename);
  };
}

const {
  buildConfigurableEntries,
  buildConfiguredEntries,
  buildEnabledRows,
  canToggleModEnabledRow,
  reorderEnabledEntryValues
} = require(path.join(desktopRoot, "src", "views", "servers", "mod-workbench-model.ts"));
const {
  buildModSettingsApplyPlan,
  buildModSettingsRemovePlan
} = require(path.join(desktopRoot, "src", "views", "servers", "mod-workbench-plans.ts"));
const { parseWorkshopIdList } = require(path.join(desktopRoot, "src", "views", "settings", "guided-setting-values.ts"));

test("DST collection download, enablement and ownership use server members without re-expanding native collections", () => {
  const { buildDownloadableSteamIds } = require("../src/views/servers/mod-workbench-model.ts");
  const { collectInstalledWorkshopCollections, mergeManagedWorkshopCollections } = require("../src/views/servers/mod-workbench-collections.ts");
  const { buildWorkshopCollectionRemovalPlan } = require("../src/views/servers/mod-workbench-collection-removal.ts");
  const server = { id: "2000001", status: "resolved", item_kind: "item", consumer_app_id: 322330, children: [] };
  const client = { ...server, id: "1365141672", tags: ["Client_Only_Mod"] };
  const root = { ...server, id: "1000001", item_kind: "collection", children: [server, client], child_count: 2 };
  const lookup = { [root.id]: root, [client.id]: client };
  const options = { [server.id]: { difficulty: 2 } };
  const settings = { master_mod_configuration_options: options };
  assert.deepEqual(buildDownloadableSteamIds([root.id], lookup, 322330), [server.id]);
  const plan = buildModSettingsApplyPlan("dontstarve", settings, [root.id], lookup, 322330);
  assert.equal(plan.canApply, true);
  for (const field of ["shared_workshop_mod_ids", "master_enabled_workshop_mod_ids", "caves_enabled_workshop_mod_ids"]) {
    assert.equal(plan.nextSettings[field], server.id);
  }
  assert.equal(Object.hasOwn(plan.nextSettings, "shared_workshop_collection_ids"), false);
  const merged = mergeManagedWorkshopCollections(plan.nextSettings, collectInstalledWorkshopCollections([root.id], lookup, 322330));
  assert.deepEqual(merged.steam_workshop_collections[0].member_ids, [server.id]);
  const removed = buildWorkshopCollectionRemovalPlan({ moduleId: "dontstarve", settings: merged,
    collectionId: root.id, selectedMemberIds: [server.id] });
  assert.deepEqual(removed.nextSettings.steam_workshop_collections, []);
  assert.equal(removed.nextSettings.master_enabled_workshop_mod_ids, "");
  assert.deepEqual(removed.nextSettings.master_mod_configuration_options, options);
  assert.deepEqual(removed.nextSettings.dst_removed_workshop_mod_ids, [server.id]);
  const existingNative = { ...settings, shared_workshop_collection_ids: "9999999\n8888888" };
  assert.equal(buildModSettingsApplyPlan("dontstarve", existingNative, [root.id], lookup, 322330)
    .nextSettings.shared_workshop_collection_ids, existingNative.shared_workshop_collection_ids);
  assert.deepEqual(buildDownloadableSteamIds([client.id], lookup, 322330), []);
  assert.equal(buildModSettingsApplyPlan("dontstarve", {}, [client.id], lookup, 322330).canApply, false);
  const clientsOnly = { ...root, children: [client], child_count: 1 };
  assert.equal(buildModSettingsApplyPlan("dontstarve", {}, [root.id], { [root.id]: clientsOnly }, 322330).canApply, false);
  for (const overrides of [{ consumer_app_id: 108600 }, { consumer_app_id: null }, { status: "not_found" }, { item_kind: "guide" }]) {
    const stale = { ...lookup, [client.id]: { ...client, ...overrides } };
    assert.deepEqual(buildDownloadableSteamIds([root.id], stale, 322330), []);
    assert.equal(buildModSettingsApplyPlan("dontstarve", {}, [root.id], stale, 322330).canApply, false);
  }
});

test("unverified metadata preserves local Mod controls without authorizing a download", () => {
  const { isUnsupportedWorkshopItem, buildDownloadableSteamIds, isIncompleteWorkshopCollection } =
    require("../src/views/servers/mod-workbench-model.ts");
  const unknown = { id: "123456", status: "unverified", item_kind: "unknown", consumer_app_id: 322330,
    title: "Saved Mod", message: "HTTP 429", children: [], child_count: 0 };
  assert.equal(isUnsupportedWorkshopItem(unknown, 322330), false);
  assert.deepEqual(buildDownloadableSteamIds([unknown.id], { [unknown.id]: unknown }, 322330), []);
  const collection = { ...unknown, id: "234567", status: "resolved", item_kind: "collection", children: [unknown], child_count: 1 };
  assert.equal(isIncompleteWorkshopCollection(collection), true);
  assert.deepEqual(buildDownloadableSteamIds([collection.id], { [collection.id]: collection }, 322330), []);
  assert.equal(isUnsupportedWorkshopItem({ ...unknown, tags: ["client_only_mod"] }, 322330), true);
});

test("PZ map order has a dedicated editor and is excluded from generic editable Mod rows", () => {
  const entries = buildConfiguredEntries("projectzomboid", { map_name: "CustomMap;Muldraugh, KY", mods: "CustomMod" });
  assert.equal(entries.find((entry) => entry.key === "pz-maps")?.fieldLabel, "map_name");
  const rows = buildEnabledRows(buildConfigurableEntries("projectzomboid", entries));
  assert.equal(rows.some((row) => row.entry.key === "pz-maps"), false);
  assert.deepEqual(rows.map((row) => row.value), ["CustomMod"]);
});

test("DST raw-only settings cannot be silently changed through structured enablement", () => {
  const id = "2039181790";
  const item = { id, status: "resolved", item_kind: "item", consumer_app_id: 322330, children: [] };
  for (const shard of ["master", "caves"]) {
    const settings = { [`${shard}_modoverrides_lua`]: `return {["workshop-${id}"]={enabled=false}}` };
    const plan = buildModSettingsApplyPlan("dontstarve", settings, [id], { [id]: item }, 322330);
    assert.equal(plan.canApply, false);
    assert.equal(plan.nextSettings, null);
    assert.match(plan.summaryFallback, /modoverrides/i);
    const mixed = { ...settings, master_enabled_workshop_mod_ids: id };
    const removal = buildModSettingsRemovePlan("dontstarve", mixed, [id], { [id]: item }, 322330, null);
    assert.equal(removal.canRemove, false);
    assert.equal(removal.nextSettings, null);
  }
});

test("preparing a disabled DST Mod never writes shard enablement or configuration", () => {
  const id = "2039181790";
  const item = { id, status: "resolved", item_kind: "item", consumer_app_id: 322330, children: [] };
  for (const settings of [
    { shared_workshop_mod_ids: id, master_enabled_workshop_mod_ids: "", caves_enabled_workshop_mod_ids: "",
      master_mod_configuration_options: { [id]: { difficulty: 2 } } },
    { shared_workshop_mod_ids: id, master_enabled_workshop_mod_ids: id, caves_enabled_workshop_mod_ids: "" },
    { master_modoverrides_lua: `return {["workshop-${id}"]={enabled=false}}` }
  ]) {
    const before = structuredClone(settings);
    const plan = buildModSettingsApplyPlan("dontstarve", settings, [id], { [id]: item }, 322330, "prepare");
    assert.equal(plan.nextSettings, null, "Reading Mod options must not enable either shard");
    assert.equal(plan.canApply, false);
    assert.deepEqual(settings, before);
  }
});

test("enablement toggles require a reversible persisted disabled entry", () => {
  const row = (key) => ({ key: `${key}:123456`, id: "123456", value: "123456",
    entry: { key, label: "Mod", fieldLabel: "mods", kind: "game-mod", ids: ["123456"], values: ["123456"] } });
  assert.equal(canToggleModEnabledRow(row("dst-enabled"), "dontstarve", false), true);
  assert.equal(canToggleModEnabledRow(row("dst-disabled"), "dontstarve", false), true);
  assert.equal(canToggleModEnabledRow(row("dst-shared-mods"), "dontstarve", false), false);
  assert.equal(canToggleModEnabledRow(row("palworld-enabled_mods"), "palworld", true), true);
  assert.equal(canToggleModEnabledRow(row("unturned-workshop"), "unturned", false), false);
  assert.equal(canToggleModEnabledRow(row("pz-workshop"), "projectzomboid", false), true);
  assert.equal(canToggleModEnabledRow(row("pz-mods"), "projectzomboid", false), false);
});

test("nested or unresolved collections cannot be partly enabled", () => {
  const child = { id: "100000", status: "resolved", item_kind: "item", consumer_app_id: 322330 };
  for (const unsupported of [
    { ...child, id: "900001", item_kind: "collection" },
    { ...child, id: "900002", status: "not_found" },
    { ...child, id: "900003", status: "unsupported", item_kind: "unsupported" }
  ]) {
    const outer = { id: "900000", status: "resolved", item_kind: "collection", consumer_app_id: 322330,
      children: [child, unsupported], child_count: 2 };
    const plan = buildModSettingsApplyPlan("dontstarve", {}, [outer.id], { [outer.id]: outer }, 322330);
    assert.equal(plan.canApply, false);
    assert.equal(plan.nextSettings, null);
  }
});

test("Workshop lists retain comma-separated IDs alongside URLs, newlines and comments", () => {
  for (const raw of [
    "# ignored 9999999999,8888888888\r\n-- ignored 7777777777,6666666666\n"
      + "https://steamcommunity.com/sharedfiles/filedetails/?id=2039181790&searchtext=\r\n"
      + "workshop-1392778117\n2039181790\n",
    "# ignored 9999999999,8888888888\r\n-- ignored 7777777777,6666666666\n"
      + "https://steamcommunity.com/sharedfiles/filedetails/?id=2039181790&searchtext=, "
      + "workshop-1392778117,,2039181790\n"
  ]) {
    assert.deepEqual(parseWorkshopIdList(raw), ["2039181790", "1392778117"]);
  }
});

test("adding a DST Mod retains every existing comma-separated subscription and shard ID", () => {
  const settings = {
    shared_workshop_mod_ids: "2039181790,1392778117",
    master_enabled_workshop_mod_ids: "2039181790,1392778117",
    caves_enabled_workshop_mod_ids: "2039181790,1392778117"
  };
  const addedId = "1111111111";
  const lookup = {
    [addedId]: { id: addedId, status: "resolved", item_kind: "item", consumer_app_id: 322330, children: [] }
  };
  const entries = buildConfiguredEntries("dontstarve", settings);
  const enabledRows = buildEnabledRows(buildConfigurableEntries("dontstarve", entries));
  assert.deepEqual(enabledRows.map((row) => row.id), ["2039181790", "1392778117"]);
  const plan = buildModSettingsApplyPlan("dontstarve", settings, [addedId], lookup, 322330);
  assert.equal(plan.canApply, true);
  for (const key of Object.keys(settings)) {
    assert.equal(plan.nextSettings[key], "2039181790\n1392778117\n1111111111");
    assert.equal(settings[key], "2039181790,1392778117");
  }
});

test("ARK enablement keeps its native download and active lists in sync", () => {
  const settings = { active_mod_ids: "111111", auto_managed_mod_ids: "111111", auto_managed_mods: false, server_name: "Keep me" };
  const lookup = { "222222": { id: "222222", status: "resolved", item_kind: "item", consumer_app_id: 346110, children: [] } };
  const plan = buildModSettingsApplyPlan("arksurvivalevolved", settings, ["222222"], lookup, 346110);
  assert.equal(plan.nextSettings.active_mod_ids, "111111\n222222");
  assert.equal(plan.nextSettings.auto_managed_mod_ids, "111111\n222222");
  assert.equal(plan.nextSettings.auto_managed_mods, true);
  assert.equal(plan.nextSettings.server_name, "Keep me");
  assert.equal(settings.auto_managed_mods, false);
  const removed = buildModSettingsRemovePlan("arksurvivalevolved", plan.nextSettings, ["222222"], lookup, 346110, null);
  assert.equal(removed.nextSettings.active_mod_ids, "111111");
  assert.equal(removed.nextSettings.auto_managed_mod_ids, "111111");
  assert.equal(removed.nextSettings.server_name, "Keep me");
});

test("ARK repairs missing native installer membership without accepting an invalid Mod", () => {
  const settings = { active_mod_ids: "222222", auto_managed_mod_ids: "", auto_managed_mods: true };
  const item = { id: "222222", status: "resolved", item_kind: "item", consumer_app_id: 346110, children: [] };
  const plan = buildModSettingsApplyPlan("arksurvivalevolved", settings, [item.id], { [item.id]: item }, 346110);
  assert.equal(plan.canApply, true);
  assert.equal(plan.nextSettings.auto_managed_mod_ids, "222222");
  const blocked = buildModSettingsApplyPlan("arksurvivalevolved", settings, [item.id], { [item.id]: { ...item, status: "unsupported", item_kind: "guide" } }, 346110);
  assert.equal(blocked.canApply, false);
  assert.equal(blocked.nextSettings, null);
});

test("DST configuration merges shard enablement into one stable row", () => {
  const entries = buildConfiguredEntries("dontstarve", {
    shared_workshop_mod_ids: "101000\n102000",
    shared_workshop_collection_ids: "900000",
    master_enabled_workshop_mod_ids: "101000\n102000",
    caves_enabled_workshop_mod_ids: "102000\n103000"
  });

  const configurableEntries = buildConfigurableEntries("dontstarve", entries);
  assert.equal(configurableEntries.length, 1);
  assert.equal(configurableEntries[0].key, "dst-enabled");
  assert.deepEqual(configurableEntries[0].values, ["101000", "102000", "103000"]);
  assert.deepEqual(buildEnabledRows(configurableEntries).map((row) => row.id), ["101000", "102000", "103000"]);
});

test("settings planning enables DST mods on both generated shards", () => {
  const plan = buildModSettingsApplyPlan(
    "dontstarve",
    {
      shared_workshop_mod_ids: "101000",
      master_enabled_workshop_mod_ids: "101000",
      caves_enabled_workshop_mod_ids: ""
    },
    ["102000"],
    {
      "102000": {
        id: "102000",
        title: "Example",
        item_kind: "item",
        consumer_app_id: 322330,
        children: []
      }
    },
    322330
  );

  assert.equal(plan.canApply, true);
  assert.deepEqual(plan.addedIds, ["102000"]);
  assert.equal(plan.nextSettings.shared_workshop_mod_ids, "101000\n102000");
  assert.equal(plan.nextSettings.master_enabled_workshop_mod_ids, "101000\n102000");
  assert.equal(plan.nextSettings.caves_enabled_workshop_mod_ids, "102000");
});

test("Project Zomboid removal clears Workshop and scanned local identifiers together", () => {
  const plan = buildModSettingsRemovePlan(
    "projectzomboid",
    {
      workshop_items: "100010\n100020",
      mods: "Alpha;Beta",
      map_name: "MapA;MapB"
    },
    ["100010"],
    {},
    108600,
    {
      workshop_root: "workshop",
      workshop_root_exists: true,
      items: [
        {
          workshop_item_id: "100010",
          status: "installed",
          workshop_item_path: "workshop/100010",
          workshop_item_exists: true,
          mods: [{ mod_id: "Alpha", status: "loaded", mod_path: "workshop/100010/mods/Alpha", map_ids: ["MapA"] }]
        },
        {
          workshop_item_id: "100020", status: "installed",
          mods: [{ mod_id: "Beta", status: "loaded", map_ids: ["MapB"] }]
        }
      ]
    }
  );

  assert.equal(plan.canRemove, true);
  assert.equal(plan.nextSettings.workshop_items, "100020");
  assert.equal(plan.nextSettings.mods, "Beta");
  assert.equal(plan.nextSettings.map_name, "MapB");
});

test("enabled-row reordering preserves the remaining order", () => {
  const entry = {
    key: "pz-mods",
    label: "Enabled Mod IDs",
    fieldLabel: "mods",
    kind: "game-mod",
    ids: [],
    values: ["Alpha", "Beta", "Gamma"]
  };

  assert.deepEqual(reorderEnabledEntryValues(entry, "Gamma", "Alpha"), ["Gamma", "Alpha", "Beta"]);
  assert.equal(reorderEnabledEntryValues(entry, "Alpha", "Alpha"), null);
});

test("removal accepts configured Workshop IDs even when their online metadata is unusable", () => {
  const id = "123456";
  for (const override of [
    { status: "unsupported", item_kind: "guide" },
    { tags: ["client_only_mod"] },
    { consumer_app_id: 108600 },
    { status: "not_found", item_kind: "unknown" }
  ]) {
    const settings = { shared_workshop_mod_ids: id, master_enabled_workshop_mod_ids: id,
      caves_enabled_workshop_mod_ids: id, master_mod_configuration_options: { [id]: { difficulty: 10 } } };
    const item = { id, status: "resolved", item_kind: "item", consumer_app_id: 322330, children: [], ...override };
    const plan = buildModSettingsRemovePlan("dontstarve", settings, [id], { [id]: item }, 322330, null);
    assert.equal(plan.canRemove, true, JSON.stringify(override));
    assert.equal(plan.nextSettings.shared_workshop_mod_ids, "");
    assert.equal(plan.nextSettings.master_enabled_workshop_mod_ids, "");
    assert.equal(plan.nextSettings.caves_enabled_workshop_mod_ids, "");
    assert.deepEqual(plan.nextSettings.master_mod_configuration_options, settings.master_mod_configuration_options);
    assert.deepEqual(plan.nextSettings.dst_removed_workshop_mod_ids, [id]);
  }
});

test("DST My Mods retains only instance-owned disabled entries and configuration-only entries", () => {
  const settings = { shared_workshop_mod_ids: "123456", master_enabled_workshop_mod_ids: "234567",
    master_mod_configuration_options: { "345678": { difficulty: 10 } },
    caves_mod_configuration_options: { "workshop-456789": { difficulty: 20 } } };
  const entries = buildConfiguredEntries("dontstarve", settings);
  const rows = buildEnabledRows(buildConfigurableEntries("dontstarve", entries));
  assert.deepEqual(rows.map((row) => row.id), ["234567", "123456", "345678", "456789"]);
  assert.deepEqual(rows.map((row) => row.entry.key), ["dst-enabled", "dst-disabled", "dst-disabled", "dst-disabled"]);
  const removed = buildModSettingsRemovePlan("dontstarve", settings, ["345678"], {}, 322330, null);
  assert.equal(removed.canRemove, true, "configuration-only ownership must be removable");
  assert.deepEqual(removed.nextSettings.master_mod_configuration_options, settings.master_mod_configuration_options);
  assert.deepEqual(removed.nextSettings.dst_removed_workshop_mod_ids, ["345678"]);
  assert.equal(buildEnabledRows(buildConfigurableEntries("dontstarve", buildConfiguredEntries("dontstarve", removed.nextSettings)))
    .some((row) => row.id === "345678"), false);
});

test("changed collection metadata cannot broaden removal of an individually owned Mod", () => {
  const id = "123456", childId = "234567";
  const lookup = { [id]: { id, status: "resolved", item_kind: "collection", consumer_app_id: 322330,
    children: [{ id: childId, status: "resolved", item_kind: "item", consumer_app_id: 322330 }] } };
  const settings = { shared_workshop_mod_ids: `${id}\n${childId}`, master_enabled_workshop_mod_ids: `${id}\n${childId}`,
    caves_enabled_workshop_mod_ids: childId, master_mod_configuration_options: { [childId]: { difficulty: 10 } } };
  const plan = buildModSettingsRemovePlan("dontstarve", settings, [id], lookup, 322330, null);
  assert.equal(plan.canRemove, true);
  assert.equal(plan.nextSettings.shared_workshop_mod_ids, childId);
  assert.equal(plan.nextSettings.master_enabled_workshop_mod_ids, childId);
  assert.equal(plan.nextSettings.caves_enabled_workshop_mod_ids, childId);
  assert.deepEqual(plan.nextSettings.master_mod_configuration_options, settings.master_mod_configuration_options);

  const ownedCollection = buildModSettingsRemovePlan("dontstarve", { ...settings, shared_workshop_collection_ids: id },
    [id], lookup, 322330, null);
  assert.equal(ownedCollection.nextSettings.shared_workshop_collection_ids, "");
  assert.equal(ownedCollection.nextSettings.shared_workshop_mod_ids, "");
  assert.equal(ownedCollection.nextSettings.master_enabled_workshop_mod_ids, "");
});
