const ts = require("@typescript/typescript6");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { parseSource, sourceText, transpileTypeScript, visitSyntax } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) =>
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}

const filename = path.resolve(__dirname, "../src/views/servers/ModWorkbench.tsx");
const source = fs.readFileSync(filename, "utf8");
const syntax = parseSource(source, filename);
const store = require("../src/views/servers/steam-workshop-store-model.ts");
const model = require("../src/views/servers/mod-workbench-model.ts");
const controls = require("../src/views/servers/mod-workbench-workshop-controls.ts");
const inventoryControls = require("../src/views/servers/mod-workbench-workshop-inventory.ts");

function functionSource(name) {
  let result;
  visitSyntax(syntax, (node) => {
    if (ts.isFunctionDeclaration(node) && node.name?.text === name) {
      assert.equal(result, undefined, `multiple ${name} declarations`);
      result = sourceText(source, node);
    }
  });
  assert.ok(result, `missing ${name}`);
  return result;
}

test("a machine-cached Workshop item still needs the selected instance's install flow", () => {
  const exports = {};
  const context = {
    ...store, ...model, ...controls, exports, Set,
    resolveStoreItemState: store.resolveWorkshopStoreItemState,
    moduleId: "dontstarve", expectedAppId: 322330,
    configuredSteamIdSet: new Set(), machineCachedSteamIdSet: new Set(["123456"]),
    inspectedMachineSteamIdSet: new Set(["123456"]), installingWorkshopIds: new Set(),
    workshopInstallationError: null, workflow: { steamDownloadMode: "steamcmd-cache" }
  };
  vm.runInNewContext(transpileTypeScript(
    `${functionSource("resolveWorkshopStoreItemState")}\nexports.resolve = resolveWorkshopStoreItemState;`,
    filename
  ), context, { filename });
  const state = exports.resolve({
    id: "123456", item_kind: "item", status: "resolved", consumer_app_id: 322330, children: []
  });
  assert.equal(state.installationState, "installed");
  assert.equal(state.lifecycleState, "downloaded");
  assert.equal(state.action, "install");
  context.configuredSteamIdSet.add("123456");
  const configuredState = exports.resolve({
    id: "123456", item_kind: "item", status: "resolved", consumer_app_id: 322330, children: []
  });
  assert.equal(configuredState.lifecycleState, "enabled");
  assert.equal(configuredState.action, "manage");
});

test("My Mods cannot acquire entries from the machine-wide Workshop inventory", () => {
  const render = functionSource("renderModList");
  assert.doesNotMatch(render, /machineCachedSteamIdSet|workshopInstallationResult|downloadResult/);
  assert.doesNotMatch(render, /displayedDisabledWorkshopItems/);
});

test("removed collection membership stays absent despite retained payloads across Workshop games", () => {
  const id = "123456";
  const inventory = { module_id: "palworld", target_exists: true, target_path: "C:\\Instance\\Mods", items: [
    { name: id, path: `C:\\Instance\\Mods\\${id}`, file_count: 1, inferred_id: "SavedPackage" }
  ] };
  for (const moduleId of ["projectzomboid", "palworld", "arksurvivalevolved", "barotrauma", "conanexiles", "soulmask", "unturned", "terraria", "squad"]) {
    const settings = moduleId === "palworld" ? { steam_workshop_removed_mod_ids: [id] } : {};
    const exports = {};
    const context = { ...controls, ...inventoryControls, exports, moduleId, settings, readOnly: false,
      configuredSteamIdSet: new Set(), manualInventory: inventory, manualEnablement: { setting_key: "fixture" },
      workshopControlStates: moduleId === "palworld" ? inventoryControls.palworldWorkshopStates(settings, inventory)
        : controls.readWorkshopControlStates(moduleId, settings),
      matchingWorkshopInventoryItems: (ids) => inventoryControls.workshopInventoryItems(ids, inventory) };
    vm.runInNewContext(transpileTypeScript(`${functionSource("collectionMemberAdded")}\nexports.added = collectionMemberAdded;`, filename), context, { filename });
    assert.equal(exports.added(id), false, `${moduleId}: a retained payload cannot recreate removed instance membership`);
    if (moduleId === "squad") {
      context.configuredSteamIdSet.add(id);
      assert.equal(exports.added(id), true, "an actual Squad deployment can establish membership");
    }
  }
});

test("archived Mod mutations reject before reading the original instance or persisting settings", async () => {
  for (const name of ["readWritableInstanceState", "persistSettings"]) {
    const exports = {};
    const context = { exports, readOnly: true, t: (_key, _params, fallback) => fallback,
      props: { onSaveSettings: () => assert.fail("archive must not save"), details: { summary: { id: "same-id-as-normal" } } },
      readInstanceDetails: () => assert.fail("archive must not read the original instance") };
    vm.runInNewContext(transpileTypeScript(`${functionSource(name)}\nexports.operation = ${name};`, filename), context, { filename });
    await assert.rejects(exports.operation({}), /Restore this instance/);
  }
});
