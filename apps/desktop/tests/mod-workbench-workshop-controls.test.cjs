const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) =>
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}
const { buildConfiguredEntries, buildEnabledRows, canReorderEnabledRow, canToggleModEnabledRow } = require("../src/views/servers/mod-workbench-model.ts");
const { buildModSettingsApplyPlan, buildModSettingsRemovePlan } = require("../src/views/servers/mod-workbench-plans.ts");
const { WorkshopControlError, readWorkshopControlState, readWorkshopControlStates, readOwnedWorkshopModIds, supportsWorkshopModEnablement,
  buildWorkshopModEnablementPlan, buildWorkshopDownloadOwnershipPlan, buildProjectZomboidWorkshopRemovalPlan
} = require("../src/views/servers/mod-workbench-workshop-controls.ts");
const X = "2000001", Y = "2000002", Z = "2000003";
const details = (key) => ({ workshop: { provider: "steam" }, mods: { enablement: { setting_key: key, setting_label: "Mods" } } });
const localMod = (mod_id, map_ids = []) => ({ status: "loaded", mod_id, map_ids });
const item = (workshop_item_id, mods) => ({ workshop_item_id, status: "installed", mods });
const snapshot = (...items) => ({ workshop_root_exists: true, workshop_root: "instance/workshop", items });
const state = (moduleId, settings, id, scan) => readWorkshopControlState(moduleId, settings, id, scan);
const failsWith = (code) => (error) => error instanceof WorkshopControlError && error.code === code;

test("config-list games persist disabled ownership and restore through the same real load-list fields", () => {
  for (const moduleId of ["barotrauma", "conanexiles", "soulmask"]) {
    const source = { mod_workshop_ids: `${X}\n${Y}`, unrelated: { keep: true },
      steam_workshop_collections: [{ id: "9000001", title: "Collection", member_ids: [X, Y] }] };
    const before = structuredClone(source);
    const disabled = buildWorkshopModEnablementPlan(moduleId, source, [X, X], false).nextSettings;
    assert.equal(disabled.mod_workshop_ids, Y);
    assert.deepEqual(disabled.steam_workshop_disabled_mod_ids, [X]);
    assert.deepEqual(state(moduleId, disabled, X), { owned: true, enabled: false, partiallyEnabled: false, canToggle: true });
    assert.deepEqual(state(moduleId, disabled, Y), { owned: true, enabled: true, partiallyEnabled: false, canToggle: true });
    const rows = buildEnabledRows(buildConfiguredEntries(moduleId, disabled, details("mod_workshop_ids")));
    assert.deepEqual(rows.map((row) => [row.id, row.entry.key]), [[Y, `${moduleId}-mod_workshop_ids`], [X, `${moduleId}-disabled`]]);
    assert.ok(rows.every((row) => canToggleModEnabledRow(row, moduleId, false)));
    assert.equal(canReorderEnabledRow(rows[1]), false);
    const allOff = buildWorkshopModEnablementPlan(moduleId, disabled, [Y], false).nextSettings;
    assert.deepEqual(allOff.steam_workshop_disabled_mod_ids, [X, Y]);
    const restored = buildWorkshopModEnablementPlan(moduleId, allOff, [X, Y], true).nextSettings;
    assert.equal(restored.mod_workshop_ids, `${X}\n${Y}`);
    assert.equal(Object.hasOwn(restored, "steam_workshop_disabled_mod_ids"), false);
    assert.deepEqual(restored.steam_workshop_collections, source.steam_workshop_collections);
    assert.deepEqual(restored.unrelated, source.unrelated);
    assert.deepEqual(source, before);
  }
});

test("ASE uses its existing auto-managed ID list to retain inactive instance Mods", () => {
  const source = { active_mod_ids: `${X},${Y}`, auto_managed_mod_ids: Y, auto_managed_mods: false, difficulty: 2 };
  const disabled = buildWorkshopModEnablementPlan("arksurvivalevolved", source, [X], false).nextSettings;
  assert.equal(disabled.active_mod_ids, Y);
  assert.equal(disabled.auto_managed_mod_ids, `${Y}\n${X}`);
  assert.equal(disabled.auto_managed_mods, false, "a toggle must not change the user's update policy");
  assert.equal(Object.hasOwn(disabled, "steam_workshop_disabled_mod_ids"), false);
  assert.deepEqual(readOwnedWorkshopModIds("arksurvivalevolved", disabled), [Y, X]);
  assert.equal(state("arksurvivalevolved", disabled, X).enabled, false);
  const rows = buildEnabledRows(buildConfiguredEntries("arksurvivalevolved", disabled, details("active_mod_ids")));
  assert.deepEqual(rows.map((row) => [row.id, row.entry.key]), [[Y, "arksurvivalevolved-active_mod_ids"], [X, "arksurvivalevolved-disabled"]]);
  assert.equal(canReorderEnabledRow(rows[1]), false);
  const enabled = buildWorkshopModEnablementPlan("arksurvivalevolved", disabled, [X], true).nextSettings;
  assert.equal(enabled.active_mod_ids, `${Y}\n${X}`);
  assert.equal(enabled.difficulty, 2);
  assert.deepEqual(source, { active_mod_ids: `${X},${Y}`, auto_managed_mod_ids: Y, auto_managed_mods: false, difficulty: 2 });
});

test("download-only ownership does not activate any supported game's newly downloaded Mod", () => {
  for (const moduleId of ["projectzomboid", "arksurvivalevolved", "barotrauma", "conanexiles", "soulmask"]) {
    const source = { mods: "OldMod", map_name: "Muldraugh, KY", active_mod_ids: Y, mod_workshop_ids: Y,
      steam_workshop_collections: [] };
    const next = buildWorkshopDownloadOwnershipPlan(moduleId, source, [X, X, Y]);
    assert.ok(readOwnedWorkshopModIds(moduleId, next).includes(X));
    assert.equal(next.mods, source.mods);
    assert.equal(next.map_name, source.map_name);
    assert.equal(next.active_mod_ids, source.active_mod_ids);
    assert.equal(next.mod_workshop_ids, source.mod_workshop_ids);
    assert.equal(state(moduleId, next, X).enabled, false);
    if (moduleId === "projectzomboid") assert.equal(next.workshop_items, `${X}\n${Y}`);
    else if (moduleId === "arksurvivalevolved") {
      assert.equal(next.auto_managed_mod_ids, `${X}\n${Y}`);
      assert.equal(next.auto_managed_mods, true);
    } else assert.deepEqual(next.steam_workshop_disabled_mod_ids, [X]);
  }
});

test("PZ single disable preserves Workshop ownership, remaining shared internal IDs and the vanilla map", () => {
  const scan = snapshot(item(X, [localMod("Alpha", ["MapA"]), localMod("Shared", ["SharedMap"])]),
    item(Y, [localMod("shared", ["sharedmap"])]));
  const source = { workshop_items: `${X}\n${Y}`, mods: "Alpha;Shared;Other", map_name: "MapA;SharedMap;Muldraugh, KY", other: "keep" };
  const disabled = buildWorkshopModEnablementPlan("projectzomboid", source, [X], false, scan);
  assert.equal(disabled.nextSettings.workshop_items, source.workshop_items);
  assert.equal(disabled.nextSettings.mods, "Shared\nOther");
  assert.equal(disabled.nextSettings.map_name, "SharedMap\nMuldraugh, KY");
  assert.deepEqual(disabled.retainedLocalIds, ["Shared", "SharedMap"]);
  assert.deepEqual(state("projectzomboid", disabled.nextSettings, X, scan), { owned: true, enabled: false, partiallyEnabled: true, canToggle: true });
  assert.equal(state("projectzomboid", disabled.nextSettings, Y, scan).enabled, true);
  const restored = buildWorkshopModEnablementPlan("projectzomboid", disabled.nextSettings, [X], true, scan).nextSettings;
  assert.equal(restored.mods, "Shared\nOther\nAlpha");
  assert.equal(restored.map_name, "MapA\nSharedMap\nMuldraugh, KY");
  assert.equal(state("projectzomboid", restored, X, scan).enabled, true);
  assert.equal(restored.other, "keep");
  assert.equal(source.mods, "Alpha;Shared;Other");
});

test("PZ collection disable removes shared IDs only when every owner is included and remains reversible", () => {
  const scan = snapshot(item(X, [localMod("Alpha"), localMod("Shared", ["SharedMap"])]), item(Y, [localMod("Shared", ["SharedMap"])]));
  const source = { workshop_items: `${X}\n${Y}`, mods: "Alpha;Shared;Other", map_name: "SharedMap;Muldraugh, KY" };
  const plan = buildWorkshopModEnablementPlan("projectzomboid", source, [X, Y], false, scan);
  assert.equal(plan.nextSettings.mods, "Other");
  assert.equal(plan.nextSettings.map_name, "Muldraugh, KY");
  assert.equal(plan.nextSettings.workshop_items, source.workshop_items);
  assert.deepEqual(plan.retainedLocalIds, []);
  assert.equal(state("projectzomboid", plan.nextSettings, X, scan).enabled, false);
  assert.equal(state("projectzomboid", plan.nextSettings, X, scan).partiallyEnabled, false);
  const restored = buildWorkshopModEnablementPlan("projectzomboid", plan.nextSettings, [X, Y], true, scan).nextSettings;
  assert.equal(state("projectzomboid", restored, X, scan).enabled, true);
  assert.equal(state("projectzomboid", restored, Y, scan).enabled, true);
});

test("PZ map-only payloads can stop and resume without treating the vanilla map as enabled Mod content", () => {
  const scan = snapshot(item(X, [localMod(null, ["MapA", "Muldraugh, KY"])]));
  const source = { workshop_items: X, mods: "Unrelated", map_name: "MapA;Muldraugh, KY" };
  const disabled = buildWorkshopModEnablementPlan("projectzomboid", source, [X], false, scan).nextSettings;
  assert.equal(disabled.mods, "Unrelated");
  assert.equal(disabled.map_name, "Muldraugh, KY");
  assert.deepEqual(state("projectzomboid", disabled, X, scan), { owned: true, enabled: false, partiallyEnabled: false, canToggle: true });
  const enabled = buildWorkshopModEnablementPlan("projectzomboid", disabled, [X], true, scan).nextSettings;
  assert.equal(enabled.map_name, "MapA\nMuldraugh, KY");
});

test("PZ safe removal shares mapping protections but additionally removes Workshop ownership", () => {
  const scan = snapshot(item(X, [localMod("Alpha", ["MapA", "Muldraugh, KY"]), localMod("Shared")]), item(Y, [localMod("Shared")]));
  const source = { workshop_items: `${X}\n${Y}`, mods: "Alpha;Shared", map_name: "MapA;Muldraugh, KY",
    steam_workshop_collections: [{ id: "9000001", title: "Collection", member_ids: [X, Y] }] };
  const plan = buildProjectZomboidWorkshopRemovalPlan(source, [X], scan);
  assert.equal(plan.nextSettings.workshop_items, Y);
  assert.equal(plan.nextSettings.mods, "Shared");
  assert.equal(plan.nextSettings.map_name, "Muldraugh, KY");
  assert.deepEqual(plan.retainedLocalIds, ["Shared", "Muldraugh, KY"]);
  assert.deepEqual(plan.nextSettings.steam_workshop_collections, source.steam_workshop_collections);
  assert.equal(state("projectzomboid", plan.nextSettings, X, scan).owned, false);
});

test("PZ rejects incomplete, failed or ambiguous metadata for any configured owner before changing settings", () => {
  const source = { workshop_items: `${X}\n${Y}`, mods: "Alpha;Shared", map_name: "Muldraugh, KY" };
  const validX = item(X, [localMod("Alpha")]), validY = item(Y, [localMod("Shared")]);
  for (const scan of [null, snapshot(validX), snapshot(validX, { ...validY, status: "missing" }),
    snapshot(validX, item(Y, [{ ...localMod("Shared"), status: "failed" }])), snapshot(validX, validY, validY),
    snapshot(validX, item(Y, [localMod(null)])), { ...snapshot(validX, validY), workshop_root_exists: false }]) {
    for (const enabled of [true, false]) {
      assert.throws(() => buildWorkshopModEnablementPlan("projectzomboid", source, [X], enabled, scan), failsWith("local-metadata-missing"));
    }
    assert.throws(() => buildProjectZomboidWorkshopRemovalPlan(source, [X], scan), failsWith("local-metadata-missing"));
  }
  assert.equal(state("projectzomboid", source, X, null).canToggle, false);
  assert.equal(source.mods, "Alpha;Shared");
});

test("unsupported workflows and unknown ownership cannot gain artificial toggles", () => {
  for (const moduleId of ["unturned", "terraria", "squad", "rimworld", "palworld", "dontstarve"]) {
    assert.equal(supportsWorkshopModEnablement(moduleId), false);
    assert.equal(state(moduleId, {}, X).canToggle, false);
    assert.throws(() => buildWorkshopModEnablementPlan(moduleId, {}, [X], true), failsWith("unsupported-module"));
  }
  for (const moduleId of ["projectzomboid", "arksurvivalevolved", "barotrauma", "conanexiles", "soulmask"]) {
    assert.throws(() => buildWorkshopModEnablementPlan(moduleId, { steam_workshop_collections: [{ id: "9000001", title: "Source only", member_ids: [X] }] }, [X], true), failsWith("not-owned"));
    assert.throws(() => buildWorkshopDownloadOwnershipPlan(moduleId, {}, ["0000001"]), failsWith("invalid-selection"));
    assert.throws(() => buildWorkshopDownloadOwnershipPlan(moduleId, {}, ["18446744073709551616"]), failsWith("invalid-selection"));
  }
});

test("disabled ownership validates canonical records and refuses overflow atomically", () => {
  for (const invalid of ["2000001", [X, X], ["0000001"], [null]]) {
    assert.throws(() => buildWorkshopModEnablementPlan("conanexiles", { mod_workshop_ids: X, steam_workshop_disabled_mod_ids: invalid }, [X], false), failsWith("invalid-settings"));
  }
  const disabled = Array.from({ length: 8192 }, (_, index) => String(3000000 + index));
  const source = { mod_workshop_ids: X, steam_workshop_disabled_mod_ids: disabled };
  assert.throws(() => buildWorkshopModEnablementPlan("barotrauma", source, [X], false), failsWith("ownership-limit"));
  assert.throws(() => buildWorkshopDownloadOwnershipPlan("barotrauma", source, [Z]), failsWith("ownership-limit"));
  assert.equal(source.mod_workshop_ids, X);
  assert.deepEqual(source.steam_workshop_disabled_mod_ids, disabled);
});

test("regular install and removal clear disabled membership, including a Mod with no active load-list entry", () => {
  for (const moduleId of ["barotrauma", "conanexiles", "soulmask"]) {
    const source = { mod_workshop_ids: "", steam_workshop_disabled_mod_ids: [X, Y], saved_options: { [X]: { keep: true } } };
    const removed = buildModSettingsRemovePlan(moduleId, source, [X], {}, null, null);
    assert.equal(removed.canRemove, true);
    assert.deepEqual(removed.removedIds, [X]);
    assert.deepEqual(removed.nextSettings.steam_workshop_disabled_mod_ids, [Y]);
    assert.equal(state(moduleId, removed.nextSettings, X).owned, false);
    const content = { id: X, status: "resolved", item_kind: "item", consumer_app_id: 123, children: [] };
    const added = buildModSettingsApplyPlan(moduleId, source, [X], { [X]: content }, 123);
    assert.equal(added.canApply, true);
    assert.equal(added.nextSettings.mod_workshop_ids, X);
    assert.deepEqual(added.nextSettings.steam_workshop_disabled_mod_ids, [Y]);
    assert.deepEqual(added.nextSettings.saved_options, source.saved_options);
    const last = buildModSettingsRemovePlan(moduleId, removed.nextSettings, [Y], {}, null, null);
    assert.equal(Object.hasOwn(last.nextSettings, "steam_workshop_disabled_mod_ids"), false);
    const inconsistent = { mod_workshop_ids: X, steam_workshop_disabled_mod_ids: [X] };
    const cleaned = buildModSettingsApplyPlan(moduleId, inconsistent, [X], { [X]: content }, 123);
    assert.equal(cleaned.canApply, true, "cleanup of a restored membership must persist even if active already contains the ID");
    assert.equal(Object.hasOwn(cleaned.nextSettings, "steam_workshop_disabled_mod_ids"), false);
  }
});

test("generic PZ single removal requires fresh mapping and protects another retained Workshop owner's local IDs", () => {
  const source = { workshop_items: `${X}\n${Y}`, mods: "Alpha;Shared", map_name: "Muldraugh, KY" };
  const scan = snapshot(item(X, [localMod("Alpha"), localMod("Shared")]), item(Y, [localMod("shared")]));
  const plan = buildModSettingsRemovePlan("projectzomboid", source, [X], {}, 108600, scan);
  assert.equal(plan.canRemove, true);
  assert.equal(plan.nextSettings.mods, "Shared");
  assert.equal(plan.nextSettings.workshop_items, Y);
  assert.throws(() => buildModSettingsRemovePlan("projectzomboid", source, [X], {}, 108600, null), failsWith("local-metadata-missing"));
  assert.equal(buildModSettingsRemovePlan("projectzomboid", source, [Z], {}, 108600, null).canRemove, false);
});

test("batch control states parse a native active list once and retain inactive ownership", () => {
  for (const moduleId of ["arksurvivalevolved", "barotrauma", "conanexiles", "soulmask"]) {
    let activeReads = 0;
    const field = moduleId === "arksurvivalevolved" ? "active_mod_ids" : "mod_workshop_ids";
    const settings = { auto_managed_mod_ids: Y, steam_workshop_disabled_mod_ids: [Y] };
    Object.defineProperty(settings, field, { get() { activeReads += 1; return X; } });
    const states = readWorkshopControlStates(moduleId, settings);
    assert.equal(activeReads, 1);
    assert.deepEqual([...states], [
      [X, { owned: true, enabled: true, partiallyEnabled: false, canToggle: true }],
      [Y, { owned: true, enabled: false, partiallyEnabled: false, canToggle: true }]
    ]);
    assert.equal(states.has(Z), false, "a persisted collection snapshot alone is not active instance ownership");
  }
  assert.equal(readWorkshopControlStates("unturned", { workshop_file_ids: X }).size, 0);
});

test("batch PZ states parse settings and scan once without hiding usable metadata when another item fails", () => {
  const reads = { workshop_items: 0, mods: 0, map_name: 0, items: 0 };
  const values = { workshop_items: `${X}\n${Y}\n${Z}`, mods: "Alpha", map_name: "Muldraugh, KY" };
  const settings = {};
  for (const key of Object.keys(values)) {
    Object.defineProperty(settings, key, { get() { reads[key] += 1; return values[key]; } });
  }
  const scan = { workshop_root_exists: true, get items() {
    reads.items += 1;
    return [item(X, [localMod("Alpha", ["MapA"])]), item(Y, [localMod(null)]),
      item(Z, [localMod("Alpha")]), item(Z, [localMod("Alpha")])];
  } };
  const states = readWorkshopControlStates("projectzomboid", settings, scan);
  assert.deepEqual(reads, { workshop_items: 1, mods: 1, map_name: 1, items: 1 });
  assert.deepEqual(states.get(X), { owned: true, enabled: false, partiallyEnabled: true, canToggle: true });
  for (const id of [Y, Z]) {
    assert.deepEqual(states.get(id), { owned: true, enabled: false, partiallyEnabled: false, canToggle: false });
  }
  assert.deepEqual(readWorkshopControlStates("projectzomboid", { workshop_items: X }, null).get(X),
    { owned: true, enabled: false, partiallyEnabled: false, canToggle: false });
});
