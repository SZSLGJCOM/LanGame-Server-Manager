const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}
const { inventoryWorkshopIds, palworldWorkshopState, palworldWorkshopStates, buildPalworldWorkshopPlan,
  isRemovedWorkshopInventoryItem, restoreRemovedWorkshopIds } = require("../src/views/servers/mod-workbench-workshop-inventory.ts");
const X = "111111", Y = "222222";
const item = (name, inferred_id, overrides = {}) => ({ name, inferred_id, path: `C:\\instance\\Mods\\${name}`, file_count: 2, item_type: "directory", ...overrides });
const inventory = (...items) => ({ module_id: "palworld", target_exists: true, target_path: "C:\\instance\\Mods", items });

test("Palworld stop, remove and restore have distinct persistent membership while preserving options and files", () => {
  const files = inventory(item(X, "First"), item(Y, "Second"));
  const initial = { mod_package_names: "First\nSecond", options: { First: { value: 42 } },
    steam_workshop_collections: [{ id: "333333", title: "Pack", member_ids: [X, Y] }] };
  const original = structuredClone(initial), originalFiles = structuredClone(files);
  const disabled = buildPalworldWorkshopPlan(initial, files, [X], "disable").nextSettings;
  assert.equal(disabled.mod_package_names, "Second");
  assert.deepEqual(palworldWorkshopState(disabled, files, X), { owned: true, enabled: false, canToggle: true, partiallyEnabled: false });
  const removed = buildPalworldWorkshopPlan(disabled, files, [X], "remove").nextSettings;
  assert.deepEqual(removed.steam_workshop_removed_mod_ids, [X]);
  assert.equal(palworldWorkshopState(removed, files, X).owned, false);
  assert.equal(isRemovedWorkshopInventoryItem(files.items[0], files, removed), true);
  assert.throws(() => buildPalworldWorkshopPlan(removed, files, [X], "enable"), (error) => error.code === "not-owned");
  const restored = restoreRemovedWorkshopIds(removed, [X]);
  assert.equal(palworldWorkshopState(restored, files, X).owned, true);
  assert.equal(palworldWorkshopState(restored, files, X).enabled, false);
  const enabled = buildPalworldWorkshopPlan(restored, files, [X], "enable").nextSettings;
  assert.equal(enabled.mod_package_names, "Second\nFirst");
  assert.deepEqual(enabled.options, initial.options);
  assert.deepEqual(enabled.steam_workshop_collections, initial.steam_workshop_collections);
  assert.deepEqual(initial, original);
  assert.deepEqual(files, originalFiles);
});

test("inventory ownership ignores numeric ancestors, empty directories and paths outside this instance", () => {
  const files = { ...inventory(), target_path: `C:\\${X}\\Mods` };
  for (const path of [`C:\\${X}\\ModsBackup\\${X}`, `C:\\other\\Mods\\${X}`, `C:\\${X}\\Mods\\..\\${X}`]) {
    assert.deepEqual(inventoryWorkshopIds(item(X, "First", { path }), files), []);
  }
  assert.deepEqual(inventoryWorkshopIds(item("Package", "First", { path: `C:\\${X}\\Mods\\Package` }), files), []);
  assert.deepEqual(inventoryWorkshopIds(item(X, "First", { path: `C:\\${X}\\Mods\\${X}`, file_count: 0 }), files), []);
  assert.deepEqual(inventoryWorkshopIds(item(X, "First", { path: `C:\\${X}\\Mods\\${X}` }), files), [X]);
});

test("Palworld removal preserves shared package names including a directory attributed to two Workshop IDs", () => {
  for (const files of [inventory(item(X, "Shared"), item(Y, "shared")), inventory(item(`${X}-${Y}`, "Shared"))]) {
    const plan = buildPalworldWorkshopPlan({ mod_package_names: "Shared" }, files, [X], "remove");
    assert.equal(plan.nextSettings.mod_package_names, "Shared");
    assert.deepEqual(plan.retainedLocalIds, ["Shared"]);
    assert.equal(palworldWorkshopState(plan.nextSettings, files, X).owned, false);
    assert.equal(palworldWorkshopState(plan.nextSettings, files, Y).enabled, true);
  }
});

test("Palworld controls require complete targeted metadata and removal also checks other installed packages", () => {
  const settings = { mod_package_names: "First" };
  for (const files of [null, inventory(item(X, null)), inventory(item(X, "First"), item(Y, null)),
    inventory(item(X, "First"), item(Y, "Second", { path: `C:\\other\\${Y}` }))]) {
    assert.throws(() => buildPalworldWorkshopPlan(settings, files, [X], "remove"), (error) => error.code === "local-metadata-missing");
  }
  assert.throws(() => buildPalworldWorkshopPlan(settings, inventory(item(Y, "Second")), [X], "enable"), (error) => error.code === "not-owned");
  const full = { ...settings, steam_workshop_removed_mod_ids: Array.from({ length: 8192 }, (_, index) => String(3000000 + index)) };
  assert.throws(() => buildPalworldWorkshopPlan(full, inventory(item(X, "First")), [X], "remove"), (error) => error.code === "ownership-limit");
  assert.equal(full.mod_package_names, "First");
});

test("multiple Palworld payloads under one Workshop item expose a mixed state until all packages are enabled", () => {
  const files = inventory(item(`${X}-one`, "First"), item(`${X}-two`, "Second"));
  const states = palworldWorkshopStates({ mod_package_names: "First" }, files);
  assert.equal(states.size, 1);
  assert.deepEqual(states.get(X), { owned: true, enabled: false, canToggle: true, partiallyEnabled: true });
  const plan = buildPalworldWorkshopPlan({ mod_package_names: "First" }, files, [X, X], "enable");
  assert.equal(plan.nextSettings.mod_package_names, "First\nSecond");
  assert.equal(palworldWorkshopState(plan.nextSettings, files, X).enabled, true);
});
