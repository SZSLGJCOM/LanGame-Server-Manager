const ts = require("@typescript/typescript6");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const {
  parseSource,
  sourceText,
  transpileTypeScript,
  visitSyntax
} = require("../scripts/typescript_source_tools.cjs");
const vm = require("node:vm");

const root = path.resolve(__dirname, "..", "..", "..");
const modulesDir = path.join(root, "modules");
const apiMockModuleAssetsSource = fs.readFileSync(
  path.join(root, "apps", "desktop", "src", "api-mock", "module-assets.ts"),
  "utf8"
);
const moduleManifestSource = fs.readFileSync(
  path.join(root, "apps", "desktop", "src", "api-mock", "module-manifest.ts"),
  "utf8"
);
const moduleDetailsSource = fs.readFileSync(
  path.join(root, "apps", "desktop", "src", "api-mock", "module-details.ts"),
  "utf8"
);
const livePlayerMockSource = fs.readFileSync(
  path.join(root, "apps", "desktop", "src", "api-mock", "live-players.ts"),
  "utf8"
);

function extractNamedFunction(source, filename, functionName) {
  const sourceFile = parseSource(source, filename);
  let declaration = null;
  visitSyntax(sourceFile, (node) => {
    if (ts.isFunctionDeclaration(node) && node.name?.text === functionName) {
      declaration = node;
      return false;
    }
  });
  assert.ok(declaration, `${functionName} is missing from ${filename}`);
  return sourceText(source, declaration);
}

function loadMockBindAddressParser(tomlById) {
  const helperNames = [
    "readMockTomlString",
    "readMockTomlInteger",
    "readMockTomlStringArray",
    "readMockTomlTable"
  ];
  const modelSource = [
    "const mockModuleTomlById = __tomlById;",
    ...helperNames.map((name) => extractNamedFunction(apiMockModuleAssetsSource, "module-assets.ts", name)),
    extractNamedFunction(moduleManifestSource, "module-manifest.ts", "parseMockBindAddressFromModuleToml"),
    "export { parseMockBindAddressFromModuleToml };"
  ].join("\n\n");
  const transpiled = transpileTypeScript(modelSource, "mock-bind-address-model.ts");
  const module = { exports: {} };
  vm.runInNewContext(transpiled, {
    __tomlById: tomlById,
    exports: module.exports,
    module
  });
  return module.exports.parseMockBindAddressFromModuleToml;
}

function loadMockPortGroupsParser(tomlById) {
  const helperNames = [
    "readMockTomlString",
    "readMockTomlIntegerMap",
    "readMockTomlStringArray",
    "readMockTomlArrayTables"
  ];
  const modelSource = [
    "const mockModuleTomlById = __tomlById;",
    ...helperNames.map((name) => extractNamedFunction(apiMockModuleAssetsSource, "module-assets.ts", name)),
    extractNamedFunction(moduleManifestSource, "module-manifest.ts", "parseMockPortGroupsFromModuleToml"),
    "export { parseMockPortGroupsFromModuleToml };"
  ].join("\n\n");
  const transpiled = transpileTypeScript(modelSource, "mock-port-groups-model.ts");
  const module = { exports: {} };
  vm.runInNewContext(transpiled, {
    __tomlById: tomlById,
    exports: module.exports,
    module
  });
  return module.exports.parseMockPortGroupsFromModuleToml;
}

function plain(value) {
  return JSON.parse(JSON.stringify(value));
}

function readModuleIds() {
  return fs.readdirSync(modulesDir)
    .filter((name) => fs.existsSync(path.join(modulesDir, name, "module.toml")))
    .filter((name) => fs.existsSync(path.join(modulesDir, name, "schema.json")))
    .sort();
}

function readRecordKeys(recordName) {
  const match = apiMockModuleAssetsSource.match(
    new RegExp(String.raw`const\s+${recordName}\s*:\s*Record<string,\s*[^>]+>\s*=\s*\{([\s\S]*?)\n\};`)
  );
  assert.ok(match, `${recordName} record is missing from module-assets.ts`);
  return Array.from(match[1].matchAll(/^\s+([a-z0-9]+):/gm), (item) => item[1]).sort();
}

test("mock module schemas cover every real game module", () => {
  assert.deepEqual(readRecordKeys("mockModuleSchemasById"), readModuleIds());
});

test("mock module TOML manifests cover every real game module", () => {
  assert.deepEqual(readRecordKeys("mockModuleTomlById"), readModuleIds());
});

test("mock module details project the manifest bind-address policy into runtime capabilities", () => {
  assert.match(
    moduleManifestSource,
    /function parseMockBindAddressFromModuleToml\([\s\S]*?readMockTomlTable\(toml, "runtime\.bind_address"\)/
  );
  let runtime;
  visitSyntax(parseSource(moduleDetailsSource, "module-details.ts"), (node) => {
    if (ts.isVariableDeclaration(node) && node.name?.text === "runtime") {
      runtime = node.initializer;
      return false;
    }
  });
  assert.ok(runtime && ts.isObjectLiteralExpression(runtime), "module details must construct runtime capabilities");
  const bindAddress = runtime.properties.find((property) =>
    ts.isPropertyAssignment(property) && property.name.text === "bind_address");
  assert.ok(bindAddress, "runtime capabilities must include bind_address");
  assert.match(sourceText(moduleDetailsSource, bindAddress.initializer),
    /^parseMockBindAddressFromModuleToml\(\s*summary\.id\s*\)$/);
});

test("mock module details project player-list capability from the covered TOML manifests", () => {
  assert.match(
    livePlayerMockSource,
    /readMockTomlTable\(toml, "runtime\.player_list"\)/
  );
  assert.match(
    moduleDetailsSource,
    /player_list:\s*parseMockPlayerListFromModuleToml\(summary\.id\)/
  );
});

test("mock port groups preserve fixed offsets and legacy shared-port groups", () => {
  const parsePortGroups = loadMockPortGroupsParser({
    corekeeper: fs.readFileSync(path.join(modulesDir, "corekeeper", "module.toml"), "utf8"),
    returntomoria: fs.readFileSync(path.join(modulesDir, "returntomoria", "module.toml"), "utf8")
  });

  assert.deepEqual(plain(parsePortGroups("corekeeper")), [{
    id: "game_query",
    members: ["game", "query"],
    member_offsets: { game: 0, query: 1 }
  }]);
  assert.deepEqual(plain(parsePortGroups("returntomoria")), [{
    id: "game_transport",
    members: ["game", "game_tcp"]
  }]);
});

test("mock bind-address model reads strict Abiotic, Terraria, and required-setting policies", () => {
  const parseBindAddress = loadMockBindAddressParser({
    abioticfactor: fs.readFileSync(path.join(modulesDir, "abioticfactor", "module.toml"), "utf8"),
    terraria: fs.readFileSync(path.join(modulesDir, "terraria", "module.toml"), "utf8"),
    corekeeper: fs.readFileSync(path.join(modulesDir, "corekeeper", "module.toml"), "utf8")
  });

  assert.deepEqual(plain(parseBindAddress("abioticfactor")), {
    mode: "strict",
    port_names: ["game"],
    required_setting_key: null,
    startup_timeout_ms: 60000
  });
  assert.deepEqual(plain(parseBindAddress("terraria")), {
    mode: "strict",
    port_names: ["game"],
    required_setting_key: null,
    startup_timeout_ms: 60000
  });
  assert.deepEqual(plain(parseBindAddress("corekeeper")), {
    mode: "strict",
    port_names: ["game"],
    required_setting_key: "direct_connection_enabled",
    startup_timeout_ms: 90000
  });
});

test("mock bind-address model defaults missing or unrelated tables to unsupported", () => {
  const parseBindAddress = loadMockBindAddressParser({
    query_only: [
      "[runtime.player_query]",
      'mode = "strict"',
      'port_names = ["query"]',
      "startup_timeout_ms = 120000"
    ].join("\n")
  });
  const unsupported = {
    mode: "unsupported",
    port_names: [],
    required_setting_key: null,
    startup_timeout_ms: 60000
  };

  assert.deepEqual(plain(parseBindAddress("query_only")), unsupported);
  assert.deepEqual(plain(parseBindAddress("missing")), unsupported);
});
