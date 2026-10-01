const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("", module.filename);

const { buildConfiguredEntries, buildConfigurableEntries, buildEnabledRows } = require("../src/views/servers/mod-workbench-model.ts");
const { buildDstModEnablementPlan, buildModSettingsApplyPlan, buildModSettingsRemovePlan } = require("../src/views/servers/mod-workbench-plans.ts");
const { readManagedWorkshopCollections } = require("../src/views/servers/mod-workbench-collections.ts");
const { buildWorkshopCollectionRemovalPlan } = require("../src/views/servers/mod-workbench-collection-removal.ts");
const A = "1000001", B = "1000002", X = "2000001", Y = "2000002", Z = "2000003";
const record = (id, members) => ({ id, title: `Collection ${id}`, member_ids: members });
const members = (settings) => readManagedWorkshopCollections(settings, "dontstarve").flatMap((entry) => entry.member_ids);
const rows = (settings) => buildEnabledRows(buildConfigurableEntries("dontstarve",
  buildConfiguredEntries("dontstarve", settings, undefined, members(settings))));
const remove = (settings, ids) => buildModSettingsRemovePlan("dontstarve", settings, ids, {}, 322330, null, members(settings));

test("persisted DST collection members are owned disabled rows without remote collection expansion", () => {
  const settings = { steam_workshop_collections: [record(A, [X, Y]), record(B, [Y, Z])],
    master_enabled_workshop_mod_ids: Y, dst_removed_workshop_mod_ids: [Z] };
  assert.deepEqual(rows(settings).map((row) => [row.id, row.entry.key]), [[Y, "dst-enabled"], [X, "dst-disabled"]]);
  assert.deepEqual(buildConfiguredEntries("dontstarve", { shared_workshop_collection_ids: A }), [
    { key: "dst-shared-collections", label: "Instance collections", fieldLabel: "shared_workshop_collection_ids",
      kind: "steam-collection", values: [A], ids: [A] }
  ]);
  assert.deepEqual(buildConfiguredEntries("unturned", {}, undefined, [X]), []);
  assert.deepEqual(buildConfiguredEntries("dontstarve", {}, undefined, ["bad", "0000001", "18446744073709551616"]), []);
});

test("DST single and batch enablement share one immutable plan and preserve options and collection snapshots", () => {
  const settings = { steam_workshop_collections: [record(A, [X, Y])], master_enabled_workshop_mod_ids: Z,
    master_mod_configuration_options: { [X]: { difficulty: 2 } },
    caves_mod_configuration_options: { [`workshop-${Y}`]: { difficulty: 5 } }, server_name: "Keep" };
  const before = structuredClone(settings);
  const enabled = buildDstModEnablementPlan(settings, [X, Y, X], true, members(settings));
  assert.equal(enabled.shared_workshop_mod_ids, `${X}\n${Y}`);
  assert.equal(enabled.master_enabled_workshop_mod_ids, `${Z}\n${X}\n${Y}`);
  assert.equal(enabled.caves_enabled_workshop_mod_ids, `${X}\n${Y}`);
  const disabled = buildDstModEnablementPlan(enabled, [X, Y], false, members(enabled));
  assert.equal(disabled.shared_workshop_mod_ids, `${X}\n${Y}`);
  assert.equal(disabled.master_enabled_workshop_mod_ids, Z);
  assert.equal(disabled.caves_enabled_workshop_mod_ids, "");
  assert.deepEqual(rows(disabled).map((row) => [row.id, row.entry.key]), [[Z, "dst-enabled"], [X, "dst-disabled"], [Y, "dst-disabled"]]);
  for (const key of ["master_mod_configuration_options", "caves_mod_configuration_options", "steam_workshop_collections", "server_name"]) {
    assert.deepEqual(disabled[key], settings[key]);
  }
  const single = buildDstModEnablementPlan(disabled, [X], true, members(disabled));
  assert.equal(single.master_enabled_workshop_mod_ids, `${Z}\n${X}`);
  assert.equal(single.caves_enabled_workshop_mod_ids, X);
  assert.deepEqual(settings, before);
});

test("DST batch enablement rejects raw Lua, unknown members, removed snapshots and invalid IDs atomically", () => {
  const settings = { steam_workshop_collections: [record(A, [X, Y])], dst_removed_workshop_mod_ids: [Y] };
  for (const enabled of [true, false]) {
    assert.equal(buildDstModEnablementPlan(settings, [X, Z], enabled, members(settings)), null);
    assert.equal(buildDstModEnablementPlan(settings, [X, Y], enabled, members(settings)), null);
    assert.equal(buildDstModEnablementPlan(settings, [], enabled, members(settings)), null);
    assert.equal(buildDstModEnablementPlan(settings, ["0000001"], enabled, ["0000001"]), null);
    for (const shard of ["master", "caves"]) {
      const raw = { ...settings, [`${shard}_modoverrides_lua`]: "return {custom=true}" };
      assert.equal(buildDstModEnablementPlan(raw, [X], enabled, members(raw)), null);
    }
  }
  assert.deepEqual(settings, { steam_workshop_collections: [record(A, [X, Y])], dst_removed_workshop_mod_ids: [Y] });
});

test("single DST removal preserves its collection snapshot and options, and repair restores the same Mod", () => {
  const settings = { steam_workshop_collections: [record(A, [X, Y]), record(B, [Y])],
    master_mod_configuration_options: { [X]: { difficulty: 2 } },
    caves_mod_configuration_options: { [`workshop-${X}`]: { difficulty: 3 } } };
  const plan = remove(settings, [X]);
  assert.equal(plan.canRemove, true);
  assert.deepEqual(plan.removedIds, [X]);
  assert.deepEqual(plan.nextSettings.dst_removed_workshop_mod_ids, [X]);
  assert.deepEqual(plan.nextSettings.steam_workshop_collections, settings.steam_workshop_collections);
  assert.deepEqual(plan.nextSettings.master_mod_configuration_options, settings.master_mod_configuration_options);
  assert.deepEqual(plan.nextSettings.caves_mod_configuration_options, settings.caves_mod_configuration_options);
  assert.deepEqual(rows(plan.nextSettings).map((row) => row.id), [Y]);
  assert.equal(remove(plan.nextSettings, [X]).canRemove, false);
  assert.equal(remove(settings, [Z]).canRemove, false);
  const item = { id: X, status: "resolved", item_kind: "item", consumer_app_id: 322330, children: [] };
  const repaired = buildModSettingsApplyPlan("dontstarve", plan.nextSettings, [X], { [X]: item }, 322330).nextSettings;
  assert.equal(Object.hasOwn(repaired, "dst_removed_workshop_mod_ids"), false);
  assert.deepEqual(rows(repaired).map((row) => [row.id, row.entry.key]), [[X, "dst-enabled"], [Y, "dst-disabled"]]);
  assert.deepEqual(repaired.master_mod_configuration_options, settings.master_mod_configuration_options);
  assert.deepEqual(repaired.steam_workshop_collections, settings.steam_workshop_collections);
});

test("snapshot-only DST members can be removed and known overlaps stay protected in whole-collection removal", () => {
  const settings = { steam_workshop_collections: [record(A, [X, Y]), record(B, [Y])] };
  const single = remove(settings, [X]);
  assert.equal(single.canRemove, true);
  assert.deepEqual(single.removedIds, [X]);
  assert.deepEqual(rows(single.nextSettings).map((row) => row.id), [Y]);
  const whole = buildWorkshopCollectionRemovalPlan({ moduleId: "dontstarve", settings, collectionId: A, selectedMemberIds: [X] });
  assert.deepEqual(whole.nextSettings.steam_workshop_collections, [record(B, [Y])]);
  assert.deepEqual(whole.nextSettings.dst_removed_workshop_mod_ids, [X]);
  assert.deepEqual(rows(whole.nextSettings).map((row) => row.id), [Y]);
  assert.throws(() => buildWorkshopCollectionRemovalPlan({ moduleId: "dontstarve", settings, collectionId: A, selectedMemberIds: [Y] }),
    (error) => error.code === "shared-member");
});

test("DST removal refuses marker overflow without removing ownership or collection records", () => {
  const settings = { steam_workshop_collections: [record(A, [X])], shared_workshop_mod_ids: X,
    dst_removed_workshop_mod_ids: Array.from({ length: 8192 }, (_, index) => String(3000000 + index)) };
  const before = structuredClone(settings);
  const plan = remove(settings, [X]);
  assert.equal(plan.canRemove, false);
  assert.equal(plan.nextSettings, null);
  assert.equal(plan.summaryKey, "servers.mods.removeSummary.ownershipLimit");
  assert.throws(() => buildWorkshopCollectionRemovalPlan({ moduleId: "dontstarve", settings, collectionId: A, selectedMemberIds: [X] }),
    (error) => error.code === "invalid-records");
  assert.deepEqual(settings, before);
});
