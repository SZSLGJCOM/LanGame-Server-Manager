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
const { collectProjectZomboidLocalIds } = require("../src/views/servers/mod-workbench-plans.ts");
const { workshopInventoryItems } = require("../src/views/servers/mod-workbench-workshop-inventory.ts");

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

const row = (id) => ({ key: `enabled:${id}`, id, value: id });
const directory = (name, overrides = {}) => ({
  name, path: `C:\\Fixture\\current-instance\\Mods\\${name}`, item_type: "directory",
  file_count: 2, total_bytes: 8192, inferred_id: null, ...overrides
});
const inventory = (items = [], targetPath = "C:\\Fixture\\current-instance\\Mods") => ({
  target_path: targetPath, items
});

function navigation(overrides = {}) {
  const selected = { row: null, path: null, source: null, view: null, kind: null, manifest: true };
  const exports = {};
  const context = {
    exports, Set, collectProjectZomboidLocalIds, workshopInventoryItems,
    moduleId: "squad", manualInventory: inventory(), enabledRows: [], pzSnapshot: null,
    // Shared cache entries deliberately exist but cannot establish instance membership.
    machineCachedSteamIdSet: new Set(["123456"]),
    workshopInstallationResult: { items: [{ item_id: "123456", installed: true, path: "C:\\SharedCache\\123456" }] },
    setSelectedEnabledRowKey: (value) => { selected.row = value; },
    setSelectedInventoryPath: (value) => { selected.path = value; },
    setActiveDetailSource: (value) => { selected.source = value; },
    setActiveWorkbenchView: (value) => { selected.view = value; },
    setBrowseKind: (value) => { selected.kind = value; },
    setManifestMode: (value) => { selected.manifest = value; },
    ...overrides
  };
  const declarations = ["matchingWorkshopInventoryItems", "resolveWorkshopContentSelection", "handleManageWorkshopContent"].map(functionSource).join("\n");
  vm.runInNewContext(transpileTypeScript(`${declarations}\nexports.manage = handleManageWorkshopContent; exports.resolve = resolveWorkshopContentSelection;`, filename), context, { filename });
  return { selected, manage: exports.manage, resolve: exports.resolve };
}

test("resolving a collection member's details does not force navigation out of its collection", () => {
  const target = directory("123456");
  const f = navigation({ manualInventory: inventory([directory("FixtureMap"), target]) });
  const before = JSON.stringify(f.selected);
  const selection = f.resolve(["123456"]);
  assert.equal(selection.inventoryItem, target);
  assert.equal(selection.row, null);
  assert.equal(JSON.stringify(f.selected), before);
});

test("Palworld collection members select the matching PackageName row case-insensitively", () => {
  const f = navigation({
    moduleId: "palworld",
    manualInventory: inventory([
      directory("123456", { inferred_id: "FixturePackage" }),
      directory("234567", { inferred_id: "OtherPackage" })
    ]),
    enabledRows: [row("OtherPackage"), row("fixturepackage")]
  });
  f.manage(["123456"]);
  assert.equal(f.selected.row, "enabled:fixturepackage");
  assert.equal(f.selected.path, null);
  assert.equal(f.selected.source, "enabled");
  assert.equal(f.selected.view, "config");
  assert.equal(f.selected.kind, "item");
  assert.equal(f.selected.manifest, false);
});

test("offline Squad collection members select their exact instance inventory directory without online metadata", () => {
  const target = directory("123456");
  const f = navigation({ manualInventory: inventory([directory("FixtureMap"), target, directory("234567")]) });
  // No lookup map exists in this harness: navigation is based on saved membership and local inventory.
  f.manage(["123456"]);
  assert.equal(f.selected.path, target.path);
  assert.equal(f.selected.row, null);
  assert.equal(f.selected.source, "inventory");
  assert.equal(f.selected.view, "config");
  assert.equal(f.selected.kind, "item");
});

test("shared machine caches and empty instance payloads cannot become the selected member inventory", () => {
  const f = navigation({ manualInventory: inventory([
    directory("123456", { file_count: 0, total_bytes: 0 }),
    directory("1234567"), directory("FixtureMap")
  ]) });
  f.manage(["123456"]);
  assert.equal(f.selected.path, null);
  assert.equal(f.selected.row, null);
  assert.equal(f.selected.source, null);
  assert.equal(f.selected.view, "config");
});

test("numeric parent directories and paths outside the instance cannot select an unrelated collection payload", () => {
  const root = "C:\\Fixture\\123456\\Mods";
  const current = inventory([
    directory("OtherPackage", { path: `${root}\\OtherPackage` }),
    directory("123456", { path: "C:\\OtherInstance\\Mods\\123456" }),
    directory("123456", { path: `${root}Backup\\123456` }),
    directory("123456", { path: `${root}\\..\\123456` })
  ], root);
  const f = navigation({ manualInventory: current });
  f.manage(["123456"]);
  assert.equal(f.selected.path, null);
  assert.equal(f.selected.row, null);
  assert.equal(f.selected.source, null);
  const member = directory("FixturePackage", { path: `${root}\\Workshop\\123456\\Payload` });
  current.items.push(member);
  f.manage(["123456"]);
  assert.equal(f.selected.path, member.path, "only the ID inside this instance's payload path can establish the match");
  assert.equal(f.selected.source, "inventory");
});

test("Project Zomboid collection members still resolve actual local Mod and map IDs", () => {
  const f = navigation({
    moduleId: "projectzomboid",
    enabledRows: [row("UnrelatedMod"), row("LocalMod"), row("123456")],
    pzSnapshot: { items: [{ workshop_item_id: "123456", mods: [{ mod_id: "LocalMod", map_ids: ["FixtureMap"] }] }] }
  });
  f.manage(["123456"]);
  assert.equal(f.selected.row, "enabled:LocalMod");
  assert.equal(f.selected.source, "enabled");
  assert.equal(f.selected.path, null);
});
