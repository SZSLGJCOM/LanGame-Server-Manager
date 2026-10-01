const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { parseSource, sourceText, transpileTypeScript, visitSyntax } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) =>
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}
const { buildEnabledRows, canReorderEnabledRow, canToggleModEnabledRow } = require("../src/views/servers/mod-workbench-model.ts");
const workshopControls = require("../src/views/servers/mod-workbench-workshop-controls.ts");
const workshopInventory = require("../src/views/servers/mod-workbench-workshop-inventory.ts");
const { ManualModInventoryList } = require("../src/views/servers/ManualModInventoryList.tsx");
const { WorkshopStatus } = require("../src/views/servers/WorkshopStatus.tsx");
const { I18nContext } = require("../src/i18n-context.ts");
const translation = { locale: "en-US", setLocale() {}, t: (key, _params, fallback) => fallback ?? key };

const filename = path.resolve(__dirname, "../src/views/servers/ModWorkbench.tsx");
const source = fs.readFileSync(filename, "utf8");
const syntax = parseSource(source, filename);

function selectionEffect(dependency) {
  let callback;
  visitSyntax(syntax, (node) => {
    if (node.type !== "CallExpression" || node.callee.value !== "useEffect") return;
    const dependencies = node.arguments[1]?.expression;
    if (dependencies?.elements?.some((item) => item?.expression?.value === dependency)) {
      assert.equal(callback, undefined, `multiple selection effects for ${dependency}`);
      callback = sourceText(source, node.arguments[0].expression);
    }
  });
  assert.ok(callback, `missing selection effect for ${dependency}`);
  return callback;
}

const cases = [
  { name: "enabled", selected: "selectedEnabledRowKey", setter: "setSelectedEnabledRowKey", list: "enabledRows", itemKey: "key" },
  { name: "manual", selected: "selectedInventoryPath", setter: "setSelectedInventoryPath", list: "unconfiguredInventoryItems", itemKey: "path" }
];

for (const entry of cases) {
  const callback = selectionEffect(entry.selected);
  function run(selection, ids) {
    const updates = [];
    const context = {
      showsManualInventory: entry.name === "manual",
      selectedInventoryPath: null, unconfiguredInventoryItems: [],
      setSelectedInventoryPath() { assert.fail("changed another list's selection"); },
      [entry.selected]: selection,
      [entry.list]: ids.map((id) => ({ [entry.itemKey]: id })),
      // Pending React transitions can schedule even equal values; record every dispatch.
      [entry.setter]: (value) => updates.push(value)
    };
    vm.runInNewContext(`(${callback})()`, context, { filename });
    return updates;
  }

  test(`${entry.name} selection remains idle for empty or already selected inventory`, () => {
    assert.deepEqual(run(null, []), []);
    assert.deepEqual(run("second", ["first", "second"]), []);
  });

  test(`${entry.name} selection follows removal and newly available inventory`, () => {
    assert.deepEqual(run("removed", []), [null]);
    assert.deepEqual(run(null, ["first", "second"]), ["first"]);
    assert.deepEqual(run("removed", ["first", "second"]), ["first"]);
  });
}

function loadFunctions(names, context) {
  const declarations = new Map();
  visitSyntax(syntax, (node) => {
    if (node.type === "FunctionDeclaration" && names.includes(node.identifier?.value)) {
      assert.equal(declarations.has(node.identifier.value), false);
      declarations.set(node.identifier.value, sourceText(source, node));
    }
  });
  for (const name of names) assert.ok(declarations.has(name), `missing ${name}`);
  const exports = {};
  vm.runInNewContext(transpileTypeScript(
    `${[...declarations.values()].join("\n")}\nObject.assign(exports, { ${names.join(", ")} });`, filename
  ), { require, exports, ...context }, { filename });
  return exports;
}

const renderedComponents = new WeakMap();
function renderComponent(node) {
  if (!renderedComponents.has(node)) {
    let tree;
    // Call the real component within a React render so its real useI18n hook reads
    // the provider, while retaining the returned event handlers for interaction tests.
    function Capture() { tree = node.type(node.props); return tree; }
    renderToStaticMarkup(React.createElement(I18nContext.Provider, { value: translation }, React.createElement(Capture)));
    renderedComponents.set(node, tree);
  }
  return renderedComponents.get(node);
}

function elements(node) {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!node || typeof node !== "object" || !node.props) return [];
  if (typeof node.type === "function") return [node, ...elements(renderComponent(node))];
  return [node, ...elements(node.props.children)];
}

function rowKeys(tree) {
  return elements(tree).filter((node) => node.type === "div"
    && /(?:^| )mw-entry-row(?: |$)/.test(node.props.className ?? "")).map((node) => node.key);
}

const configuredRows = buildEnabledRows([{
  key: "workshop", fieldLabel: "workshop_items", label: "Workshop", kind: "steam-item",
  ids: ["123456", "234567"], values: ["123456", "234567"]
}]);
const stagedManualItem = { path: "instance/mods/staged", name: "Staged mod", inferred_id: "staged", file_count: 1, total_bytes: 512 };

function renderList(overrides = {}) {
  const actions = [];
  const context = {
    ...workshopControls, ...workshopInventory, settings: {}, pzSnapshot: null, readOnly: false,
    moduleId: "unturned", enabledRows: configuredRows, isManualEnablementModule: false,
    showsManualInventory: false, unconfiguredInventoryItems: [], selectedInventoryItem: null,
    manualInventory: { items: [], target_exists: true, target_path: "instance/mods" }, manualInventoryError: null,
    activeDetailSource: "enabled", selectedEnabledRow: configuredRows[0], lookupMap: {},
    draggedEnabledRow: null, enabledDropTargetKey: null,
    modEnablementDisabled: false, modMutationsDisabled: false,
    ShellIcon: () => null, SteamWorkshopPreview: () => null, WorkshopStatus, ManualModInventoryList,
    t: translation.t,
    canReorderEnabledRow, canToggleModEnabledRow,
    handleSelectEnabledRow: (key) => actions.push(["select", key]),
    handleDisableEnabledRow: (row) => actions.push(["disable", row.key]),
    handleSetDstModsEnabled: (ids, enabled) => actions.push(["set-dst-enabled", Array.from(ids), enabled]),
    handleDeleteEnabledRow: (row) => actions.push(["remove", row.key]),
    setSelectedInventoryPath: (value) => actions.push(["manual", value]),
    setActiveDetailSource: (value) => actions.push(["detail", value]),
    handleEnableManualInventoryItem: (item) => actions.push(["enable", item.path]),
    handleEnabledRowPointerDown: () => {}, handleEnabledRowPointerEnter: () => {},
    handleEnabledRowPointerUp: () => {}, handleEnabledRowPointerCancel: () => {},
    ...overrides
  };
  context.workshopControlStates = context.moduleId === "palworld"
    ? workshopInventory.palworldWorkshopStates(context.settings, context.manualInventory)
    : workshopControls.readWorkshopControlStates(context.moduleId, context.settings, context.pzSnapshot);
  const { renderModList } = loadFunctions(["renderModList", "formatEnabledRowDisplayId"], context);
  return { tree: renderModList(), actions };
}

test("My Mods remains scoped to configured entries across browse and machine-cache changes", () => {
  const machineOnlyItem = { id: "999999", item_id: "999999", installed: true, expected_path_exists: true };
  for (const browseLoading of [false, true]) {
    const unrelatedInventory = {
      browseLoading, browseResult: { items: [machineOnlyItem] },
      workshopInstallationResult: { items: [machineOnlyItem] }, downloadResult: { items: [machineOnlyItem] },
      machineCachedSteamIdSet: new Set(["999999"]), unconfiguredInventoryItems: [stagedManualItem]
    };
    assert.deepEqual(rowKeys(renderList(unrelatedInventory).tree), configuredRows.map((row) => row.key));
    const empty = renderList({ ...unrelatedInventory, enabledRows: [], selectedEnabledRow: null }).tree;
    assert.deepEqual(rowKeys(empty), []);
    assert.equal(empty.props.className, "mw-empty");
  }
});

test("Unturned configured rows have no misleading enable toggle and remove only their instance entry", () => {
  const { tree, actions } = renderList();
  const controls = elements(tree);
  assert.equal(controls.some((node) => node.type === "input"), false);
  const remove = controls.find((node) => node.props.className === "mw-entry-remove-button");
  assert.equal(remove.props.disabled, false);
  remove.props.onClick();
  assert.deepEqual(actions, [
    ["select", configuredRows[0].key], ["remove", configuredRows[0].key]
  ]);
  const locked = elements(renderList({ modEnablementDisabled: true, modMutationsDisabled: true }).tree);
  assert.equal(locked.some((node) => node.type === "input"), false);
  const lockedRemovals = locked.filter((node) => node.props.className === "mw-entry-remove-button");
  assert.equal(lockedRemovals.length, configuredRows.length);
  for (const control of lockedRemovals) assert.equal(control.props.disabled, true);
});

test("recoverable DST and manual configured rows retain their real disable control", () => {
  for (const moduleId of ["dontstarve", "palworld"]) {
    const row = buildEnabledRows([{
      key: moduleId === "dontstarve" ? "dst-enabled" : "palworld-mod_package_names",
      fieldLabel: moduleId === "dontstarve" ? "modoverrides.lua" : "mod_package_names",
      label: "Mod", kind: "game-mod", ids: ["123456"], values: ["123456"]
    }])[0];
    const context = { moduleId, enabledRows: [row], selectedEnabledRow: row,
      isManualEnablementModule: moduleId === "palworld" };
    const { tree, actions } = renderList(context);
    const toggle = elements(tree).find((node) => node.type === "input");
    assert.ok(toggle, moduleId);
    assert.equal(toggle.props.checked, true);
    assert.equal(toggle.props.disabled, false);
    toggle.props.onChange();
    assert.deepEqual(actions, [["select", row.key], ["disable", row.key]]);
    const locked = renderList({ ...context, modEnablementDisabled: true, modMutationsDisabled: true });
    assert.equal(elements(locked.tree).find((node) => node.type === "input").props.disabled, true);
  }
});

test("file-based Mod inventory stays visible and selectable without inventing an enable switch", () => {
  const { tree, actions } = renderList({
    moduleId: "minecraft", enabledRows: [], selectedEnabledRow: null,
    showsManualInventory: true, unconfiguredInventoryItems: [stagedManualItem],
    selectedInventoryItem: stagedManualItem, activeDetailSource: "inventory"
  });
  assert.deepEqual(rowKeys(tree), [stagedManualItem.path]);
  const row = elements(tree).find((node) => node.key === stagedManualItem.path);
  assert.equal(row.props.className, "mw-entry-row mw-entry-row--active");
  assert.equal(elements(row).some((node) => node.type === "input"), false);
  elements(row).find((node) => node.type === "button").props.onClick();
  assert.deepEqual(actions, [["manual", stagedManualItem.path], ["detail", "inventory"]]);
});

test("manual inventory distinguishes loading, failed reads and a successfully empty result", () => {
  const context = { enabledRows: [], selectedEnabledRow: null, showsManualInventory: true };
  const loading = renderList({ ...context, manualInventory: null }).tree;
  assert.equal(loading.props["aria-busy"], true);
  assert.equal(elements(loading).some((node) => node.props.children === "Reading installed Mods…"), true);
  let retries = 0;
  const failed = renderList({ ...context, manualInventory: null, manualInventoryError: "read failed",
    setScanNonce: (update) => { retries = update(retries); } }).tree;
  assert.equal(failed.props["aria-busy"], false);
  const retry = elements(failed).find((node) => node.type === "button");
  assert.equal(retry.props.children, "Retry");
  retry.props.onClick();
  assert.equal(retries, 1);
  const empty = renderList(context).tree;
  assert.equal(empty.props["aria-busy"], undefined);
  assert.equal(elements(empty).some((node) => node.props.children === "No Mods are configured for this instance."), true);
});

test("staged manual rows remain unchecked, cannot drag, and enable only their selected package", () => {
  const { tree, actions } = renderList({
    isManualEnablementModule: true, showsManualInventory: true, unconfiguredInventoryItems: [stagedManualItem],
    selectedInventoryItem: stagedManualItem
  });
  assert.deepEqual(rowKeys(tree), [...configuredRows.map((row) => row.key), stagedManualItem.path]);
  const stagedRow = elements(tree).find((node) => node.key === stagedManualItem.path);
  const selector = elements(stagedRow).find((node) => node.type === "button");
  for (const event of ["onPointerDown", "onPointerEnter", "onPointerUp", "onDragStart", "draggable"]) {
    assert.equal(selector.props[event], undefined, event);
  }
  const toggle = elements(stagedRow).find((node) => node.type === "input");
  assert.equal(toggle.props.checked, false);
  assert.equal(toggle.props.disabled, false);
  toggle.props.onChange();
  assert.deepEqual(actions, [["manual", stagedManualItem.path], ["detail", "inventory"], ["enable", stagedManualItem.path]]);
  for (const overrides of [{ modMutationsDisabled: true }, { inferred_id: null }]) {
    const locked = renderList({
      enabledRows: [], isManualEnablementModule: true, showsManualInventory: true,
      unconfiguredInventoryItems: [{ ...stagedManualItem, ...overrides }], ...overrides
    });
    assert.equal(elements(locked.tree).find((node) => node.type === "input").props.disabled, true);
  }
});

test("pointer sorting starts only for an unlocked, sortable row with the primary button", () => {
  for (const { locked, button, row, allowed } of [
    { locked: false, button: 0, row: configuredRows[0], allowed: true },
    { locked: true, button: 0, row: configuredRows[0], allowed: false },
    { locked: false, button: 2, row: configuredRows[0], allowed: false },
    { locked: false, button: 0, row: { ...configuredRows[0], entry: { ...configuredRows[0].entry, key: "dst-enabled" } }, allowed: false }
  ]) {
    const updates = [];
    const { handleEnabledRowPointerDown } = loadFunctions(["handleEnabledRowPointerDown"], {
      modMutationsDisabled: locked, canReorderEnabledRow,
      setDraggedEnabledRow: (value) => updates.push(value),
      setEnabledDropTargetKey: (value) => updates.push(value)
    });
    handleEnabledRowPointerDown({ button }, row);
    assert.deepEqual(updates, allowed ? [row, null] : []);
  }
});

test("Workshop store actions route installation through the install flow and management to the instance", () => {
  const actions = [];
  const item = { id: "123456" };
  const { handleWorkshopStoreAction } = loadFunctions(["handleWorkshopStoreAction"], {
    handleManageWorkshopItem: (value) => actions.push(["manage", value]),
    handleInstallWorkshopItems: (ids) => actions.push(["install", Array.from(ids)])
  });
  handleWorkshopStoreAction(item, "install");
  handleWorkshopStoreAction(item, "manage");
  assert.deepEqual(actions, [["install", [item.id]], ["manage", item]]);
});

test("manual selection reconciliation cannot act on a Workshop-only instance", () => {
  vm.runInNewContext(`(${selectionEffect("selectedInventoryPath")})()`, {
    showsManualInventory: false,
    unconfiguredInventoryItems: [stagedManualItem], selectedInventoryPath: "unrelated",
    setSelectedInventoryPath() { assert.fail("changed inactive manual selection"); }
  }, { filename });
});

test("instance-owned disabled DST rows stay editable, unchecked and removable", () => {
  const row = buildEnabledRows([{ key: "dst-disabled", label: "Mod", fieldLabel: "modoverrides.lua",
    kind: "game-mod", ids: ["123456"], values: ["123456"] }])[0];
  const { tree, actions } = renderList({ moduleId: "dontstarve", enabledRows: [row], selectedEnabledRow: row });
  const controls = elements(tree);
  const toggle = controls.find((node) => node.type === "input");
  assert.equal(toggle.props.checked, false);
  assert.equal(toggle.props.disabled, false);
  toggle.props.onChange();
  controls.find((node) => node.props.className === "mw-entry-remove-button").props.onClick();
  assert.deepEqual(actions, [["select", row.key], ["set-dst-enabled", [row.id], true], ["select", row.key], ["remove", row.key]]);
  assert.equal(canReorderEnabledRow({ ...row, entry: { ...row.entry, values: ["123456", "234567"] } }), false);
});

test("reenabling an owned DST Mod is offline and preserves its saved options", async () => {
  const model = require("../src/views/servers/mod-workbench-model.ts");
  const plans = require("../src/views/servers/mod-workbench-plans.ts");
  const settings = { shared_workshop_mod_ids: "123456", master_enabled_workshop_mod_ids: "",
    caves_enabled_workshop_mod_ids: "", master_mod_configuration_options: { "123456": { difficulty: 10 } } };
  let completion, saved, currentAtSave;
  const { handleSetDstModsEnabled } = loadFunctions(["assertDstModOwnership", "handleSetDstModsEnabled"], {
    ...model, ...plans, ...require("../src/views/servers/mod-workbench-dst-policy.ts"),
    ...require("../src/views/servers/mod-workbench-collections.ts"),
    moduleId: "dontstarve", lookupMap: {}, expectedAppId: 322330,
    t: (_key, _params, fallback) => fallback,
    launchModMutation(kind, operation) { assert.equal(kind, "enablement"); completion = operation(); },
    readWritableInstanceState: async () => ({ settings }),
    persistSettings: async (next, expected, _options, validateCurrent) => {
      assert.equal(expected, settings);
      assert.equal(typeof validateCurrent, "function");
      validateCurrent(currentAtSave ?? settings);
      saved = next;
    },
    startTransition: (callback) => callback(), setSelectedEnabledRowKey() {}, setApplyMessage() {}, setScanNonce() {}
  });
  handleSetDstModsEnabled(["123456"], true);
  await completion;
  assert.equal(saved.master_enabled_workshop_mod_ids, "123456");
  assert.equal(saved.caves_enabled_workshop_mod_ids, "123456");
  assert.deepEqual(saved.master_mod_configuration_options, settings.master_mod_configuration_options);
  saved = null;
  handleSetDstModsEnabled(["999999"], true);
  await assert.rejects(completion, /not configured/);
  assert.equal(saved, null, "machine-only or stale rows cannot claim instance ownership");
  currentAtSave = {};
  handleSetDstModsEnabled(["123456"], true);
  await assert.rejects(completion, /not configured/);
  assert.equal(saved, null, "ownership removed after reading must still be checked before saving");
  currentAtSave = null;
  settings.master_modoverrides_lua = "return build_mods()";
  handleSetDstModsEnabled(["123456"], true);
  await assert.rejects(completion, /modoverrides/);
  assert.equal(saved, null, "raw Lua added since rendering must still block structured enablement");
});

test("disabling a native enabled-only DST entry retains its instance ownership", async () => {
  const model = require("../src/views/servers/mod-workbench-model.ts");
  const settings = { master_enabled_workshop_mod_ids: "123456" };
  let completion, saved;
  const { handleSetDstModsEnabled } = loadFunctions(["assertDstModOwnership", "handleSetDstModsEnabled"], {
    ...model, ...require("../src/views/servers/mod-workbench-plans.ts"),
    ...require("../src/views/servers/mod-workbench-dst-policy.ts"),
    ...require("../src/views/servers/mod-workbench-collections.ts"),
    moduleId: "dontstarve", lookupMap: {}, expectedAppId: 322330,
    t: (_key, _params, fallback) => fallback,
    launchModMutation(_kind, operation) { completion = operation(); },
    readWritableInstanceState: async () => ({ settings }),
    persistSettings: async (next, expected, _options, validateCurrent) => {
      assert.equal(expected, settings);
      validateCurrent(settings);
      saved = next;
    },
    startTransition: (callback) => callback(), setSelectedEnabledRowKey() {}, setApplyMessage() {}, setScanNonce() {}
  });
  handleSetDstModsEnabled(["123456"], false);
  await completion;
  const rows = model.buildEnabledRows(model.buildConfigurableEntries("dontstarve", model.buildConfiguredEntries("dontstarve", saved)));
  assert.deepEqual(rows.map((row) => [row.id, row.entry.key]), [["123456", "dst-disabled"]]);
});

test("Palworld collection enablement reads the current instance inventory and saves all selected packages once without downloading", async () => {
  const model = require("../src/views/servers/mod-workbench-model.ts");
  const targetPath = "C:\\Fixture\\current-instance\\Mods";
  const inventory = { module_id: "palworld", target_exists: true, target_path: targetPath, items: [
    { name: "123456", path: `${targetPath}\\123456`, file_count: 2, inferred_id: "FirstPackage" },
    { name: "234567", path: `${targetPath}\\234567`, file_count: 1, inferred_id: "SecondPackage" }
  ] };
  const initial = { mod_package_names: "ExistingPackage", mod_configuration_options: { FirstPackage: { difficulty: 10 } },
    steam_workshop_collections: [{ id: "345678", title: "Fixture collection", member_ids: ["123456", "234567"] }] };
  let settings = initial, completion, reads = 0;
  const saves = [];
  const { handleSetCollectionMembersEnabled } = loadFunctions(["workshopControlError", "assertWorkshopControlBase", "handleSetCollectionMembersEnabled"], {
    ...model, ...workshopControls, ...workshopInventory,
    ...require("../src/views/servers/mod-settings-patch.ts"),
    moduleId: "palworld", isManualEnablementModule: true,
    manualEnablement: { setting_key: "mod_package_names" }, manualInventory: { target_path: targetPath, items: [] },
    props: { details: { summary: { id: "fixture-instance" } } }, t: translation.t,
    launchModMutation(kind, operation) { assert.equal(kind, "enablement"); completion = operation(); },
    readWritableInstanceState: async () => ({ settings }),
    readManualModInventory: async (id) => { assert.equal(id, "fixture-instance"); reads += 1; return inventory; },
    downloadSteamWorkshopItems() { assert.fail("enablement must use installed packages without downloading"); },
    persistSettings: async (next, expected, _options, validateCurrent) => {
      assert.equal(expected, settings); validateCurrent(settings); saves.push(next); settings = next;
    },
    setApplyMessage() {}, setScanNonce() {}
  });
  for (const enabled of [true, false]) {
    const previousSaves = saves.length;
    handleSetCollectionMembersEnabled(["123456", "234567", "123456"], enabled);
    await completion;
    assert.equal(saves.length, previousSaves + 1, "the whole selection is committed in one settings save");
    assert.deepEqual(model.parseDelimitedEntries(settings.mod_package_names), enabled
      ? ["ExistingPackage", "FirstPackage", "SecondPackage"] : ["ExistingPackage"]);
    assert.deepEqual(settings.mod_configuration_options, initial.mod_configuration_options);
    assert.deepEqual(settings.steam_workshop_collections, initial.steam_workshop_collections);
  }
  assert.equal(reads, 2);
  assert.equal(initial.mod_package_names, "ExistingPackage", "the source settings remain unchanged");
});

test("Palworld package-row controls ignore a removed source sharing the same PackageName", () => {
  const model = require("../src/views/servers/mod-workbench-model.ts");
  const removedId = "123456", ownedId = "234567", root = "C:\\Fixture\\Mods";
  const inventory = { module_id: "palworld", target_exists: true, target_path: root, items: [removedId, ownedId].map((id) => ({
    name: id, path: `${root}\\${id}`, file_count: 2, inferred_id: "SharedPackage"
  })) };
  let settings = { mod_package_names: "SharedPackage", steam_workshop_removed_mod_ids: [removedId],
    fixture_mod_options: { SharedPackage: { difficulty: 10 } } };
  const actions = [];
  function apply(ids, action) {
    assert.deepEqual(Array.from(ids), [ownedId], "the removed Workshop source is never included in a package-row action");
    actions.push(action);
    settings = workshopInventory.buildPalworldWorkshopPlan(settings, inventory, ids, action).nextSettings;
    assert.ok(settings.steam_workshop_removed_mod_ids.includes(removedId));
    assert.deepEqual(settings.fixture_mod_options, { SharedPackage: { difficulty: 10 } });
  }
  const handlers = loadFunctions(["formatEnabledRowDisplayId", "handleDisableEnabledRow", "handleDeleteEnabledRow", "handleEnableManualInventoryItem"], {
    ...model, ...workshopInventory, moduleId: "palworld", isSteamWorkshopModule: true, isManualEnablementModule: true,
    manualEnablement: { setting_key: "mod_package_names" }, manualInventory: inventory,
    workshopControlStates: workshopInventory.palworldWorkshopStates(settings, inventory),
    handleSetCollectionMembersEnabled: (ids, enabled) => apply(ids, enabled ? "enable" : "disable"),
    handleRemoveManagedWorkshopMembers: (ids) => apply(ids, "remove"),
    handleDisableEnabledSettingValue() { assert.fail("an owned Workshop package must retain ownership-aware controls"); },
    setRetainedWorkshopIds() {}, t: translation.t
  });
  const row = model.buildEnabledRows([{ key: "palworld-mod_package_names", fieldLabel: "mod_package_names",
    label: "PackageName", kind: "game-mod", ids: ["SharedPackage"], values: ["SharedPackage"] }])[0];
  handlers.handleDisableEnabledRow(row);
  assert.equal(settings.mod_package_names, "");
  handlers.handleEnableManualInventoryItem(inventory.items[1]);
  assert.equal(settings.mod_package_names, "SharedPackage");
  handlers.handleDeleteEnabledRow(row);
  assert.equal(settings.mod_package_names, "");
  assert.deepEqual(settings.steam_workshop_removed_mod_ids, [removedId, ownedId]);
  assert.deepEqual(actions, ["disable", "enable", "remove"]);
  assert.equal(inventory.items.length, 2, "both payloads remain available for explicit recovery");
});
