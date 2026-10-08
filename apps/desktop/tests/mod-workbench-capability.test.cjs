const ts = require("@typescript/typescript6");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { parseSource, sourceText, transpileTypeScript, visitSyntax } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".ts"] = (module, filename) =>
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { MOD_WORKFLOW_CATALOG, moduleHasModWorkbench } = require("../src/views/servers/mod-workbench-capability.ts");
const root = path.resolve(__dirname, "../../..");
const modulesDir = path.join(root, "modules");
const moduleIds = fs.readdirSync(modulesDir).filter((id) => fs.existsSync(path.join(modulesDir, id, "module.toml"))).sort();
const tomlById = Object.fromEntries(moduleIds.map((id) => [id, fs.readFileSync(path.join(modulesDir, id, "module.toml"), "utf8")]));

function declarations(relativePath, names) {
  const filename = path.resolve(__dirname, relativePath);
  const source = fs.readFileSync(filename, "utf8");
  const found = new Map();
  visitSyntax(parseSource(source, filename), (node) => {
    if (ts.isFunctionDeclaration(node) && names.includes(node.name?.text)) {
      found.set(node.name.text, sourceText(source, node));
    }
  });
  return names.map((name) => { assert.ok(found.has(name), name); return found.get(name); }).join("\n");
}

// Execute the app's real manifest projection against every current on-disk TOML.
const exportsObject = {};
vm.runInNewContext(transpileTypeScript([
  declarations("../src/api-mock/module-assets.ts", ["readMockTomlString", "readMockTomlInteger", "readMockTomlBoolean", "readMockTomlStringArray", "readMockTomlTable"]),
  declarations("../src/api-mock/module-manifest.ts", ["parseMockWorkshopFromModuleToml", "parseMockModsFromModuleToml"]),
  "export { parseMockWorkshopFromModuleToml, parseMockModsFromModuleToml };"
].join("\n"), "mod-capability-manifest.ts"), { exports: exportsObject, mockModuleTomlById: tomlById });
const details = (id) => ({
  workshop: exportsObject.parseMockWorkshopFromModuleToml(id),
  mods: exportsObject.parseMockModsFromModuleToml(id)
});

test("all 32 actual modules expose only their declared Mod workspaces", () => {
  assert.equal(moduleIds.length, 32);
  const hidden = ["nightingale", "returntomoria", "romestead", "runescapedragonwilds", "scum", "theforest"];
  for (const id of moduleIds) assert.equal(moduleHasModWorkbench(id, details(id)), !hidden.includes(id), id);
  assert.equal(moduleHasModWorkbench("unknown", {}), false);
  assert.equal(moduleHasModWorkbench(null, details("dontstarve")), false);
  assert.equal(moduleIds.filter((id) => details(id).mods?.manual_staging).length, 20);
  assert.equal(moduleIds.filter((id) => details(id).mods?.enablement).length, 7);
});

test("catalog providers agree with real manifests, including Core Keeper's Thunderstore source", () => {
  for (const [id, entry] of Object.entries(MOD_WORKFLOW_CATALOG)) {
    assert.ok(moduleIds.includes(id), `${id} must be an actual module`);
    const module = details(id);
    const provider = module.workshop?.provider ?? module.mods?.source?.provider;
    if (provider) assert.equal(entry.provider, provider, id);
  }
  assert.equal(MOD_WORKFLOW_CATALOG.corekeeper.provider, "thunderstore");
});

test("server collections apply to ten Steam games, not fourteen file-only games or client dependencies", () => {
  const steam = moduleIds.filter((id) => details(id).workshop?.provider === "steam"
    && MOD_WORKFLOW_CATALOG[id]?.installScope !== "client_only");
  assert.deepEqual(steam, ["arksurvivalevolved", "barotrauma", "conanexiles", "dontstarve", "palworld", "projectzomboid", "soulmask", "squad", "terraria", "unturned"]);
  const fileOnly = moduleIds.filter((id) => {
    const module = details(id);
    return !module.workshop && module.mods?.manual_staging && !module.mods.enablement;
  });
  assert.equal(fileOnly.length, 14);
  assert.ok(fileOnly.every((id) => moduleHasModWorkbench(id, details(id))));
  assert.equal(MOD_WORKFLOW_CATALOG.rimworld.installScope, "client_only");
  assert.equal(moduleHasModWorkbench("rimworld", details("rimworld")), true, "RimWorld keeps the client dependency explanation");
  assert.equal(moduleHasModWorkbench("runescapedragonwilds", details("runescapedragonwilds")), false);
});
