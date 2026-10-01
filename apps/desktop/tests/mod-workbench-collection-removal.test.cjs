const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) =>
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}

const {
  CollectionRemovalError, buildWorkshopCollectionRemovalPreview, buildWorkshopCollectionRemovalPlan
} = require("../src/views/servers/mod-workbench-collection-removal.ts");
const { buildConfiguredEntries, buildConfigurableEntries, buildEnabledRows } = require("../src/views/servers/mod-workbench-model.ts");
const { buildModSettingsApplyPlan } = require("../src/views/servers/mod-workbench-plans.ts");

const A = "1000001", B = "1000002", X = "2000001", Y = "2000002", Z = "2000003";
const record = (id, member_ids) => ({ id, title: `Collection ${id}`, member_ids });
const settings = (overrides = {}) => ({ steam_workshop_collections: [record(A, [X, Y])], ...overrides });
const input = (moduleId, overrides = {}) => ({ moduleId, collectionId: A, settings: settings(), selectedMemberIds: [X, Y], ...overrides });
const dstRows = (source) => buildEnabledRows(buildConfigurableEntries("dontstarve", buildConfiguredEntries("dontstarve", source)));
const dstItem = (id) => ({ id, status: "resolved", item_kind: "item", consumer_app_id: 322330, children: [] });
const expectCode = (code, operation) => assert.throws(operation, (error) => error instanceof CollectionRemovalError && error.code === code);
const pzItem = (id, modNames = [], mapNames = [], overrides = {}) => ({
  workshop_item_id: id, item_path: `C:/Workshop/${id}`, status: "installed",
  mods: modNames.map((mod_id, index) => ({
    mod_id, map_ids: index === 0 ? mapNames : [], status: "loaded", directory_name: mod_id, mod_path: `C:/Workshop/${id}/${mod_id}`
  })), ...overrides
});
const pzSnapshot = (items) => ({ workshop_root: "C:/Workshop", workshop_root_exists: true, items });
const inventoryItem = (id, inferred_id, overrides = {}) => ({
  name: id, path: `C:/Fixture/Mods/Workshop/${id}`, item_type: "directory", file_count: 2,
  total_bytes: 256, inferred_id, ...overrides
});
const inventory = (items, overrides = {}) => ({
  instance_id: "instance-palworld", module_id: "palworld", source_label: "Workshop", target_label: "Workshop",
  target_path: "C:/Fixture/Mods/Workshop", target_exists: true, items, ...overrides
});

test("preview uses saved members and protects every known overlapping collection without inferring standalone ownership", () => {
  const source = settings({ steam_workshop_collections: [record(A, [X, Y]), record(B, [Y, Z])] });
  const preview = buildWorkshopCollectionRemovalPreview(input("dontstarve", { settings: source }));
  assert.deepEqual(preview.memberIds, [X, Y]);
  assert.deepEqual(preview.removableMemberIds, [X]);
  assert.deepEqual(preview.protectedMembers, [{ id: Y, collectionIds: [B] }]);
  assert.deepEqual(preview.unknownCollectionIds, []);
  assert.equal(preview.memberRemovalBlock, null);
  assert.deepEqual(source.steam_workshop_collections, [record(A, [X, Y]), record(B, [Y, Z])]);
});

test("unknown legacy membership blocks member deletion while leaving source-only removal available", () => {
  const source = settings({ shared_workshop_collection_ids: `${A}\n${B}`, master_enabled_workshop_mod_ids: `${X}\n${Y}` });
  const preview = buildWorkshopCollectionRemovalPreview(input("dontstarve", { settings: source }));
  assert.equal(preview.memberRemovalBlock, "unknown-collection-members");
  assert.deepEqual(preview.unknownCollectionIds, [B]);
  assert.deepEqual(preview.removableMemberIds, []);
  expectCode("unknown-collection-members", () => buildWorkshopCollectionRemovalPlan(input("dontstarve", { settings: source })));
  const removed = buildWorkshopCollectionRemovalPlan(input("dontstarve", { settings: source, selectedMemberIds: [] }));
  assert.deepEqual(removed.nextSettings, { ...source, steam_workshop_collections: [], shared_workshop_collection_ids: B });
  assert.deepEqual(removed.fileRemovalIds, []);
});

test("an imported collection with no snapshot cannot acquire guessed members", () => {
  const source = { shared_workshop_collection_ids: A, master_enabled_workshop_mod_ids: X };
  const preview = buildWorkshopCollectionRemovalPreview(input("dontstarve", { settings: source }));
  assert.equal(preview.memberRemovalBlock, "missing-member-snapshot");
  expectCode("invalid-selection", () => buildWorkshopCollectionRemovalPlan(input("dontstarve", { settings: source, selectedMemberIds: [X] })));
  assert.equal(buildWorkshopCollectionRemovalPlan(input("dontstarve", { settings: source, selectedMemberIds: [] })).nextSettings.shared_workshop_collection_ids, "");
});

test("selection cannot exceed the saved snapshot or remove known shared members", () => {
  expectCode("invalid-selection", () => buildWorkshopCollectionRemovalPlan(input("unturned", { selectedMemberIds: [Z] })));
  const source = settings({ steam_workshop_collections: [record(A, [X, Y]), record(B, [Y])] });
  expectCode("shared-member", () => buildWorkshopCollectionRemovalPlan(input("unturned", { settings: source, selectedMemberIds: [Y] })));
  expectCode("collection-not-found", () => buildWorkshopCollectionRemovalPreview(input("unturned", { collectionId: Z })));
  expectCode("invalid-records", () => buildWorkshopCollectionRemovalPreview(input("unturned", { settings: { steam_workshop_collections: "broken" } })));
});

test("DST removes chosen enablement and download IDs while preserving all Mod options and unselected members", () => {
  const source = settings({
    steam_workshop_collections: [record(A, [X, Y]), record(B, [Z])], shared_workshop_collection_ids: `${A}\n${B}`,
    shared_workshop_mod_ids: `${X}\n${Y}\n${Z}`, master_enabled_workshop_mod_ids: `${X}\n${Y}`,
    caves_enabled_workshop_mod_ids: `${X}\n${Z}`, islands_enabled_workshop_mod_ids: `${X}\n${Y}`,
    volcano_enabled_workshop_mod_ids: `${X}\n${Z}`, master_mod_configuration_options: { [X]: { strength: 5 } },
    caves_mod_configuration_options: { [`workshop-${X}`]: { difficulty: 3 } },
    islands_mod_configuration_options: { [X]: { season: 2 } },
    volcano_mod_configuration_options: { [X]: { eruption: 1 } }, unrelated: { value: 42 }
  });
  const before = structuredClone(source);
  const plan = buildWorkshopCollectionRemovalPlan(input("dontstarve", { settings: source, selectedMemberIds: [X, X] }));
  assert.deepEqual(plan.nextSettings, {
    ...source, steam_workshop_collections: [record(B, [Z])], shared_workshop_collection_ids: B,
    shared_workshop_mod_ids: `${Y}\n${Z}`, master_enabled_workshop_mod_ids: Y, caves_enabled_workshop_mod_ids: Z,
    islands_enabled_workshop_mod_ids: Y, volcano_enabled_workshop_mod_ids: Z,
    dst_removed_workshop_mod_ids: [X]
  });
  assert.deepEqual(plan.removedMemberIds, [X]);
  assert.deepEqual(plan.fileRemovalIds, []);
  assert.deepEqual(dstRows(plan.nextSettings).map((row) => row.id), [Y, Z]);
  assert.deepEqual(source, before);
});

test("DST preserves configuration-only ownership until a member was explicitly removed", () => {
  const source = settings({ master_mod_configuration_options: { [X]: { strength: 5 } },
    caves_mod_configuration_options: { [`workshop-${Y}`]: { difficulty: 3 } } });
  assert.deepEqual(dstRows(source).map((row) => [row.id, row.entry.key]), [[X, "dst-disabled"], [Y, "dst-disabled"]]);
  const removed = buildWorkshopCollectionRemovalPlan(input("dontstarve", { settings: source, selectedMemberIds: [X] }));
  assert.deepEqual(dstRows(removed.nextSettings).map((row) => row.id), [Y]);
  assert.deepEqual(removed.nextSettings.master_mod_configuration_options, source.master_mod_configuration_options);
  assert.deepEqual(removed.nextSettings.caves_mod_configuration_options, source.caves_mod_configuration_options);
});

test("DST explicit downloads and shard enablement retain ownership despite a removal marker", () => {
  for (const field of ["shared_workshop_mod_ids", "master_enabled_workshop_mod_ids", "caves_enabled_workshop_mod_ids",
    "islands_enabled_workshop_mod_ids", "volcano_enabled_workshop_mod_ids"]) {
    const source = { dst_removed_workshop_mod_ids: [X], [field]: X, master_mod_configuration_options: { [X]: { keep: true } } };
    assert.deepEqual(dstRows(source).map((row) => row.id), [X], field);
  }
});

test("readding a removed DST Mod reuses retained options and clears only its removal marker", () => {
  const source = settings({ master_enabled_workshop_mod_ids: `${X}\n${Y}`,
    master_mod_configuration_options: { [X]: { strength: 5 }, [Y]: { strength: 7 } },
    caves_mod_configuration_options: { [`workshop-${X}`]: { difficulty: 3 } } });
  const removed = buildWorkshopCollectionRemovalPlan(input("dontstarve", { settings: source })).nextSettings;
  assert.deepEqual(dstRows(removed), []);
  const addX = buildModSettingsApplyPlan("dontstarve", removed, [X], { [X]: dstItem(X) }, 322330);
  assert.equal(addX.canApply, true);
  assert.deepEqual(addX.nextSettings.dst_removed_workshop_mod_ids, [Y]);
  assert.deepEqual(dstRows(addX.nextSettings).map((row) => [row.id, row.entry.key]), [[X, "dst-enabled"]]);
  assert.deepEqual(addX.nextSettings.master_mod_configuration_options, source.master_mod_configuration_options);
  assert.deepEqual(addX.nextSettings.caves_mod_configuration_options, source.caves_mod_configuration_options);
  const addY = buildModSettingsApplyPlan("dontstarve", addX.nextSettings, [Y], { [Y]: dstItem(Y) }, 322330);
  assert.equal(addY.canApply, true);
  assert.equal(Object.hasOwn(addY.nextSettings, "dst_removed_workshop_mod_ids"), false);
  assert.deepEqual(dstRows(addY.nextSettings).map((row) => row.id), [X, Y]);
  assert.deepEqual(removed.dst_removed_workshop_mod_ids, [X, Y]);
});

test("DST readd persists marker cleanup even when every native ownership field already contains the Mod", () => {
  const source = { dst_removed_workshop_mod_ids: [X], shared_workshop_mod_ids: X,
    master_enabled_workshop_mod_ids: X, caves_enabled_workshop_mod_ids: X };
  const plan = buildModSettingsApplyPlan("dontstarve", source, [X], { [X]: dstItem(X) }, 322330);
  assert.equal(plan.canApply, true);
  assert.equal(Object.hasOwn(plan.nextSettings, "dst_removed_workshop_mod_ids"), false);
  assert.deepEqual(plan.addedIds, [X]);
  assert.deepEqual(source.dst_removed_workshop_mod_ids, [X]);
});

test("DST removal deduplicates markers and never marks members retained by another collection", () => {
  const source = settings({ steam_workshop_collections: [record(A, [X, Y]), record(B, [Y])],
    dst_removed_workshop_mod_ids: [Z], shared_workshop_mod_ids: `${X}\n${Y}`,
    master_mod_configuration_options: { [X]: { keep: true }, [Y]: { keep: true }, [Z]: { keep: true } } });
  expectCode("shared-member", () => buildWorkshopCollectionRemovalPlan(input("dontstarve", { settings: source })));
  const plan = buildWorkshopCollectionRemovalPlan(input("dontstarve", { settings: source, selectedMemberIds: [X, X] }));
  assert.deepEqual(plan.nextSettings.dst_removed_workshop_mod_ids, [Z, X]);
  assert.deepEqual(dstRows(plan.nextSettings).map((row) => row.id), [Y]);
  assert.deepEqual(source.dst_removed_workshop_mod_ids, [Z]);
  const sourceOnly = buildWorkshopCollectionRemovalPlan(input("dontstarve", { settings: source, selectedMemberIds: [] }));
  assert.deepEqual(sourceOnly.nextSettings.dst_removed_workshop_mod_ids, [Z]);
});

test("DST custom Lua blocks member changes but permits removing only the collection record", () => {
  const source = settings({ shared_workshop_collection_ids: A, master_modoverrides_lua: `return {["workshop-${X}"]={enabled=true}}` });
  expectCode("raw-overrides", () => buildWorkshopCollectionRemovalPlan(input("dontstarve", { settings: source })));
  const plan = buildWorkshopCollectionRemovalPlan(input("dontstarve", { settings: source, selectedMemberIds: [] }));
  assert.equal(plan.nextSettings.master_modoverrides_lua, source.master_modoverrides_lua);
  assert.equal(plan.nextSettings.shared_workshop_collection_ids, "");
});

test("PZ removes selected Workshop IDs but retains internal Mod and map IDs needed by remaining items", () => {
  const source = settings({
    workshop_items: `${X}\n${Y}`, mods: "TargetMod;SharedMod;RemainingMod", map_name: "TargetMap;SharedMap;Muldraugh, KY",
    custom_options: { TargetMod: { keep: true } }
  });
  const before = structuredClone(source);
  const snapshot = pzSnapshot([
    pzItem(X, ["TargetMod", "SharedMod"], ["TargetMap", "SharedMap", "Muldraugh, KY"]),
    pzItem(Y, ["sharedmod", "RemainingMod"], ["sharedmap"])
  ]);
  const plan = buildWorkshopCollectionRemovalPlan(input("projectzomboid", { settings: source, selectedMemberIds: [X], pzSnapshot: snapshot }));
  assert.equal(plan.nextSettings.workshop_items, Y);
  assert.equal(plan.nextSettings.mods, "SharedMod\nRemainingMod");
  assert.equal(plan.nextSettings.map_name, "SharedMap\nMuldraugh, KY");
  assert.deepEqual(plan.nextSettings.custom_options, source.custom_options);
  assert.deepEqual(plan.retainedLocalIds, ["SharedMod", "SharedMap", "Muldraugh, KY"]);
  assert.deepEqual(plan.fileRemovalIds, []);
  assert.deepEqual(source, before);
});

test("PZ needs complete fresh metadata for selected and remaining configured Workshop items", () => {
  const source = settings({ workshop_items: `${X}\n${Y}`, mods: "TargetMod;RemainingMod", map_name: "Muldraugh, KY" });
  const good = [pzItem(X, ["TargetMod"]), pzItem(Y, ["RemainingMod"])];
  for (const snapshot of [
    null, pzSnapshot([good[0]]), pzSnapshot([good[1]]), { ...pzSnapshot(good), workshop_root_exists: false },
    pzSnapshot([good[0], pzItem(Y, ["RemainingMod"], [], { status: "installed_with_warnings" })]),
    pzSnapshot([good[0], pzItem(Y, [])]),
    pzSnapshot([good[0], { ...good[1], mods: [{ ...good[1].mods[0], status: "missing_mod_id", mod_id: null }] }]),
    pzSnapshot([good[0], good[1], good[1]])
  ]) {
    expectCode("local-metadata-missing", () => buildWorkshopCollectionRemovalPlan(input("projectzomboid", { settings: source, selectedMemberIds: [X], pzSnapshot: snapshot })));
  }
  assert.equal(source.workshop_items, `${X}\n${Y}`);
  assert.equal(source.mods, "TargetMod;RemainingMod");
});

test("PZ map-only metadata removes exclusive maps and preserves shared and vanilla maps", () => {
  const mapOnly = (id, maps) => pzItem(id, [], [], {
    mods: [{ mod_id: null, map_ids: maps, status: "loaded", directory_name: "Map", mod_path: `C:/Workshop/${id}/Map` }]
  });
  const source = settings({ workshop_items: `${X}\n${Y}`, mods: "UnrelatedMod", map_name: "ExclusiveMap;SharedMap;Muldraugh, KY" });
  const plan = buildWorkshopCollectionRemovalPlan(input("projectzomboid", {
    settings: source, selectedMemberIds: [X], pzSnapshot: pzSnapshot([
      mapOnly(X, ["ExclusiveMap", "SharedMap", "Muldraugh, KY"]), mapOnly(Y, ["SharedMap"])
    ])
  }));
  assert.equal(plan.nextSettings.workshop_items, Y);
  assert.equal(plan.nextSettings.mods, "UnrelatedMod");
  assert.equal(plan.nextSettings.map_name, "SharedMap\nMuldraugh, KY");
  assert.deepEqual(plan.retainedLocalIds, ["SharedMap", "Muldraugh, KY"]);
});

test("Palworld removes the exact package names case-insensitively and retains unrelated enabled packages", () => {
  const source = settings({ mod_package_names: "targetpackage\nRemainingPackage", package_options: { targetpackage: { keep: true } } });
  const plan = buildWorkshopCollectionRemovalPlan(input("palworld", {
    settings: source, selectedMemberIds: [X], manualInventory: inventory([
      inventoryItem(X, "TargetPackage"), inventoryItem(Y, "RemainingPackage")
    ])
  }));
  assert.equal(plan.nextSettings.mod_package_names, "RemainingPackage");
  assert.deepEqual(plan.nextSettings.package_options, source.package_options);
  assert.deepEqual(plan.fileRemovalIds, []);
  assert.equal(source.mod_package_names, "targetpackage\nRemainingPackage");
});

test("Palworld retains a PackageName shared by remaining inventory or an unselected member", () => {
  const source = settings({ mod_package_names: "SharedPackage" });
  for (const items of [
    [inventoryItem(X, "SharedPackage"), inventoryItem(Y, "sharedpackage")],
    [inventoryItem(`${X}-${Y}`, "SharedPackage")]
  ]) {
    const plan = buildWorkshopCollectionRemovalPlan(input("palworld", { settings: source, selectedMemberIds: [X], manualInventory: inventory(items) }));
    assert.equal(plan.nextSettings.mod_package_names, "SharedPackage");
    assert.deepEqual(plan.retainedLocalIds, ["SharedPackage"]);
  }
});

test("Palworld rejects missing, outside-target and ambiguous inventory metadata instead of guessing ownership", () => {
  for (const scanned of [
    null, inventory([]), inventory([inventoryItem(X, null)]),
    inventory([inventoryItem(X, "Target")], { target_exists: false }),
    inventory([inventoryItem(X, "Target")], { module_id: "valheim" }),
    inventory([inventoryItem(X, "Target", { file_count: 0 })]),
    inventory([inventoryItem(X, "Target", { path: `C:/SharedCache/${X}` })]),
    inventory([inventoryItem(X, "Target", { path: `C:/Fixture/Mods/Workshop/../${X}` })]),
    inventory([inventoryItem(X, "Target"), inventoryItem(Y, null)])
  ]) {
    expectCode("local-metadata-missing", () => buildWorkshopCollectionRemovalPlan(input("palworld", { selectedMemberIds: [X], manualInventory: scanned })));
  }
});

test("Palworld does not match Workshop digits in instance ancestors or longer numeric IDs", () => {
  const scanned = inventory([
    inventoryItem("OtherPackage", "OtherPackage", { path: `C:/Fixture/${X}/Mods/Workshop/OtherPackage` }),
    inventoryItem(`${X}9`, "LongerId", { path: `C:/Fixture/${X}/Mods/Workshop/${X}9` })
  ], { target_path: `C:/Fixture/${X}/Mods/Workshop` });
  expectCode("local-metadata-missing", () => buildWorkshopCollectionRemovalPlan(input("palworld", { selectedMemberIds: [X], manualInventory: scanned })));
});

test("whole PZ removal skips previously removed snapshot members while safely removing remaining owned members", () => {
  const source = settings({ workshop_items: Y, mods: "Remaining", map_name: "Muldraugh, KY" });
  const plan = buildWorkshopCollectionRemovalPlan(input("projectzomboid", { settings: source,
    pzSnapshot: pzSnapshot([pzItem(Y, ["Remaining"])]) }));
  assert.equal(plan.nextSettings.workshop_items, "");
  assert.equal(plan.nextSettings.mods, "");
  assert.deepEqual(plan.nextSettings.steam_workshop_collections, []);
  const absent = settings({ workshop_items: "", mods: "Manual", map_name: "Muldraugh, KY" });
  assert.deepEqual(buildWorkshopCollectionRemovalPlan(input("projectzomboid", { settings: absent })).nextSettings,
    { ...absent, steam_workshop_collections: [] });
});

test("whole Palworld removal keeps absence and removed markers distinct from active instance payloads", () => {
  const source = settings({ mod_package_names: "RemainingPackage", steam_workshop_removed_mod_ids: [X] });
  const plan = buildWorkshopCollectionRemovalPlan(input("palworld", { settings: source, manualInventory: inventory([
    inventoryItem(X, "OldPackage"), inventoryItem(Y, "RemainingPackage")
  ]) }));
  assert.equal(plan.nextSettings.mod_package_names, "");
  assert.deepEqual(plan.nextSettings.steam_workshop_removed_mod_ids, [X, Y]);
  assert.deepEqual(plan.nextSettings.steam_workshop_collections, []);
  const explicitlyRemoved = settings({ mod_package_names: "ManualPackage", steam_workshop_removed_mod_ids: [X, Y] });
  assert.deepEqual(buildWorkshopCollectionRemovalPlan(input("palworld", { settings: explicitlyRemoved })).nextSettings,
    { ...explicitlyRemoved, steam_workshop_collections: [] });
  expectCode("local-metadata-missing", () => buildWorkshopCollectionRemovalPlan(input("palworld", {
    settings: source, manualInventory: inventory([inventoryItem(X, "OldPackage")])
  })));
});

test("Terraria cannot clear unrelated internal Mod names when no trustworthy Workshop-to-name mapping exists", () => {
  const source = settings({ tmodloader_workshop_item_ids: `${X}\n${Y}`, tmodloader_enabled_mod_names: "CalamityMod\nMagicStorage" });
  expectCode("local-metadata-missing", () => buildWorkshopCollectionRemovalPlan(input("terraria", { settings: source, selectedMemberIds: [X] })));
  const sourceOnly = buildWorkshopCollectionRemovalPlan(input("terraria", { settings: source, selectedMemberIds: [] }));
  assert.equal(sourceOnly.nextSettings.tmodloader_enabled_mod_names, "CalamityMod\nMagicStorage");
  assert.equal(sourceOnly.nextSettings.tmodloader_workshop_item_ids, `${X}\n${Y}`);
});

test("Terraria can remove download IDs when no internal names are enabled, preserving the exact empty value", () => {
  const source = settings({ tmodloader_workshop_item_ids: `${X}\n${Y}`, tmodloader_enabled_mod_names: "# no enabled Mods\n-- retained comment" });
  const plan = buildWorkshopCollectionRemovalPlan(input("terraria", { settings: source, selectedMemberIds: [X] }));
  assert.equal(plan.nextSettings.tmodloader_workshop_item_ids, Y);
  assert.equal(plan.nextSettings.tmodloader_enabled_mod_names, source.tmodloader_enabled_mod_names);
});

test("Squad leaves settings untouched except provenance and returns only reviewed members for the file transaction", () => {
  const source = settings({ server_name: "Fixture", local_options: { [X]: { keep: true } } });
  const plan = buildWorkshopCollectionRemovalPlan(input("squad", { settings: source, selectedMemberIds: [Y, Y] }));
  assert.deepEqual(plan.nextSettings, { ...source, steam_workshop_collections: [] });
  assert.deepEqual(plan.removedMemberIds, [Y]);
  assert.deepEqual(plan.fileRemovalIds, [Y]);
  assert.deepEqual(plan.retainedLocalIds, []);
});

test("other supported config-based games remove only the requested IDs and preserve their files and options", () => {
  for (const [moduleId, fields] of [
    ["unturned", ["workshop_file_ids"]], ["arksurvivalevolved", ["active_mod_ids", "auto_managed_mod_ids"]],
    ["barotrauma", ["mod_workshop_ids"]], ["conanexiles", ["mod_workshop_ids"]], ["soulmask", ["mod_workshop_ids"]]
  ]) {
    const source = settings({ ...Object.fromEntries(fields.map((field) => [field, `${X}\n${Y}`])), custom_options: { [X]: { keep: true } } });
    const plan = buildWorkshopCollectionRemovalPlan(input(moduleId, { settings: source, selectedMemberIds: [X] }));
    for (const field of fields) assert.equal(plan.nextSettings[field], Y, `${moduleId}: ${field}`);
    assert.deepEqual(plan.nextSettings.custom_options, source.custom_options);
    assert.deepEqual(plan.fileRemovalIds, []);
  }
});

test("unsupported module operations fail with a typed code while source-only removal stays possible", () => {
  expectCode("unsupported-module", () => buildWorkshopCollectionRemovalPlan(input("unknown")));
  assert.deepEqual(buildWorkshopCollectionRemovalPlan(input("unknown", { selectedMemberIds: [] })).nextSettings.steam_workshop_collections, []);
});
