const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const {
  parseSource,
  transpileTypeScript
} = require("../scripts/typescript_source_tools.cjs");

const repoRoot = path.resolve(__dirname, "..", "..", "..");
const desktopRoot = path.join(repoRoot, "apps", "desktop");
const modulesRoot = path.join(repoRoot, "modules");
const moduleAssetsPath = path.join(desktopRoot, "src", "api-mock", "module-assets.ts");

function registerTypeScriptRequireExtension(extension) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    module._compile(transpileTypeScript(source, filename), filename);
  };
}

registerTypeScriptRequireExtension(".ts");
registerTypeScriptRequireExtension(".tsx");
require.extensions[".css"] = function compileEmptyCss(module) {
  module._compile("", module.filename);
};

const {
  CANONICAL_GAME_CONFIG_ACCEPTANCE_MODULE_IDS,
  resolveGameConfigAcceptanceSelection,
  verifyGameConfigAcceptanceRegistry
} = require(path.join(desktopRoot, "tests", "helpers", "configuration-module-acceptance.ts"));
const { listSettingsModuleIds } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "module-registry.ts"
));

function parseManifestId(manifestText) {
  const match = manifestText.match(/^id\s*=\s*"([^"\r\n]+)"\s*$/m);
  assert.ok(match, "module.toml must declare a top-level id");
  return match[1];
}

function readBundledModuleRecords(root) {
  return fs.readdirSync(root, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && fs.existsSync(path.join(root, entry.name, "module.toml")))
    .map((entry) => {
      const moduleRoot = path.join(root, entry.name);
      const manifestPath = path.join(moduleRoot, "module.toml");
      const schemaPath = path.join(moduleRoot, "schema.json");
      return {
        directoryId: entry.name,
        hasManifest: fs.existsSync(manifestPath),
        hasSchema: fs.existsSync(schemaPath),
        manifestId: fs.existsSync(manifestPath) ? parseManifestId(fs.readFileSync(manifestPath, "utf8")) : null,
        schemaValid: fs.existsSync(schemaPath) ? Boolean(JSON.parse(fs.readFileSync(schemaPath, "utf8"))) : false
      };
    })
    .sort((left, right) => left.directoryId.localeCompare(right.directoryId));
}

function readFixtureBindings(root) {
  return fs.readdirSync(root, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .flatMap((entry) => {
      const fixtureRoot = path.join(root, entry.name, "config-fixtures");
      if (!fs.existsSync(fixtureRoot)) {
        return [];
      }
      return fs.readdirSync(fixtureRoot, { withFileTypes: true })
        .filter((fixture) => fixture.isFile() && fixture.name.endsWith(".json"))
        .map((fixture) => ({
          ownerId: entry.name,
          declaredId: JSON.parse(fs.readFileSync(path.join(fixtureRoot, fixture.name), "utf8")).module_id
        }));
    });
}

function unwrapTypeScriptExpression(expression) {
  let current = expression;
  while (current?.type === "TsAsExpression" || current?.type === "TsTypeAssertion") {
    current = current.expression;
  }
  return current;
}

function readSchemaRegistryBindings(sourcePath) {
  const sourceText = fs.readFileSync(sourcePath, "utf8");
  const sourceFile = parseSource(sourceText, sourcePath);
  const importedSchemaModuleIds = new Map();

  for (const statement of sourceFile.body) {
    if (statement.type !== "ImportDeclaration") {
      continue;
    }
    const defaultImport = statement.specifiers.find((specifier) => specifier.type === "ImportDefaultSpecifier");
    if (!defaultImport) {
      continue;
    }
    const importPath = statement.source.value;
    const match = importPath.match(/\/modules\/([^/]+)\/schema\.json$/);
    if (match) {
      importedSchemaModuleIds.set(defaultImport.local.value, match[1]);
    }
  }

  for (const statement of sourceFile.body) {
    const declaration = statement.type === "ExportDeclaration" ? statement.declaration : statement;
    if (declaration?.type !== "VariableDeclaration") {
      continue;
    }
    for (const variable of declaration.declarations) {
      if (variable.id.type !== "Identifier" || variable.id.value !== "mockModuleSchemasById") {
        continue;
      }
      const object = unwrapTypeScriptExpression(variable.init);
      assert.equal(object.type, "ObjectExpression", "mock schema registry must be an object literal");
      return object.properties.map((property) => {
        assert.equal(property.type, "KeyValueProperty", "mock schema registry entries must be property assignments");
        const ownerId = property.key.value;
        const imported = unwrapTypeScriptExpression(property.value);
        assert.equal(imported.type, "Identifier", `${ownerId} must reference an imported schema identifier`);
        const importedIdentifier = imported.value;
        const declaredId = importedSchemaModuleIds.get(importedIdentifier);
        assert.ok(declaredId, `schema registry entry ${ownerId} must reference a bundled schema import`);
        return { ownerId, declaredId };
      });
    }
  }

  assert.fail("mockModuleSchemasById registry was not found");
}

function writeModule(root, directoryId, manifestId = directoryId, includeSchema = true) {
  const moduleRoot = path.join(root, directoryId);
  fs.mkdirSync(moduleRoot, { recursive: true });
  fs.writeFileSync(path.join(moduleRoot, "module.toml"), `id = "${manifestId}"\n`);
  if (includeSchema) {
    fs.writeFileSync(path.join(moduleRoot, "schema.json"), "{\"type\":\"object\"}\n");
  }
}

function withTemporaryModules(callback) {
  const temporaryRoot = fs.mkdtempSync(path.join(os.tmpdir(), "langame-config-acceptance-"));
  try {
    return callback(temporaryRoot);
  } finally {
    fs.rmSync(temporaryRoot, { recursive: true, force: true });
  }
}

function registryInput(records, overrides = {}) {
  return {
    canonicalModuleIds: ["alpha", "beta"],
    bundledModuleIds: records.map((record) => record.directoryId),
    manifestBindings: records
      .filter((record) => record.hasManifest)
      .map((record) => ({ ownerId: record.directoryId, declaredId: record.manifestId })),
    schemaRegistryBindings: records
      .filter((record) => record.hasSchema && record.schemaValid)
      .map((record) => ({ ownerId: record.directoryId, declaredId: record.directoryId })),
    presentationRegistryIds: ["alpha", "beta"],
    fixtureBindings: [],
    environment: {},
    ...overrides
  };
}

test("targeted acceptance module selection rejects empty, duplicate, or unknown selections", () => {
  const canonicalModuleIds = ["minecraft", "rust"];

  for (const requested of ["", "   ", ", ,"]) {
    assert.throws(
      () =>
        resolveGameConfigAcceptanceSelection({
          canonicalModuleIds,
          fixtureModuleIds: [],
          environment: { GAME_CONFIG_ACCEPTANCE_MODULES: requested }
        }),
      /must not be empty/i
    );
  }
  assert.deepEqual(
    resolveGameConfigAcceptanceSelection({
      canonicalModuleIds,
      fixtureModuleIds: ["minecraft", "rust"],
      environment: { GAME_CONFIG_ACCEPTANCE_MODULES: "minecraft, rust" }
    }),
    ["minecraft", "rust"]
  );
  assert.throws(
    () =>
      resolveGameConfigAcceptanceSelection({
        canonicalModuleIds,
        fixtureModuleIds: ["minecraft"],
        environment: { GAME_CONFIG_ACCEPTANCE_MODULES: "rust" }
      }),
    /no fixtures/i
  );
  assert.throws(
    () =>
      resolveGameConfigAcceptanceSelection({
        canonicalModuleIds,
        fixtureModuleIds: [],
        environment: { GAME_CONFIG_ACCEPTANCE_MODULES: "rust,rust" }
      }),
    /duplicate/i
  );
  assert.throws(
    () =>
      resolveGameConfigAcceptanceSelection({
        canonicalModuleIds,
        fixtureModuleIds: [],
        environment: { GAME_CONFIG_ACCEPTANCE_MODULES: "unknown" }
      }),
    /unknown/i
  );
});

test("registry verification checks directory, manifest, schema, and presentation identities", () => {
  withTemporaryModules((temporaryRoot) => {
    writeModule(temporaryRoot, "alpha");
    writeModule(temporaryRoot, "beta");
    const records = readBundledModuleRecords(temporaryRoot);

    assert.deepEqual(
      verifyGameConfigAcceptanceRegistry(registryInput(records, {
        fixtureBindings: [
          { ownerId: "alpha", declaredId: "alpha" },
          { ownerId: "beta", declaredId: "beta" }
        ]
      })),
      ["alpha", "beta"]
    );
    assert.throws(
      () => verifyGameConfigAcceptanceRegistry(registryInput(records, { presentationRegistryIds: ["alpha"] })),
      /presentation registry/i
    );
    assert.throws(
      () => verifyGameConfigAcceptanceRegistry(registryInput(records, {
        schemaRegistryBindings: [
          { ownerId: "alpha", declaredId: "alpha" },
          { ownerId: "beta", declaredId: "wrong" }
        ]
      })),
      /schema registry bindings/i
    );

    writeModule(temporaryRoot, "gamma", "wrong-id");
    const mismatchedRecords = readBundledModuleRecords(temporaryRoot);
    assert.throws(
      () => verifyGameConfigAcceptanceRegistry(registryInput(mismatchedRecords, {
        canonicalModuleIds: ["alpha", "beta", "gamma"],
        presentationRegistryIds: ["alpha", "beta", "gamma"]
      })),
      /manifest/i
    );

    fs.rmSync(path.join(temporaryRoot, "gamma"), { recursive: true, force: true });
    fs.rmSync(path.join(temporaryRoot, "beta", "schema.json"));
    const incompleteRecords = readBundledModuleRecords(temporaryRoot);
    assert.throws(
      () => verifyGameConfigAcceptanceRegistry(registryInput(incompleteRecords)),
      /schema registry bindings/i
    );
  });
});

test("module discovery separates shared sources while rejecting missing and unexpected game manifests", () => {
  withTemporaryModules((temporaryRoot) => {
    writeModule(temporaryRoot, "alpha");
    writeModule(temporaryRoot, "beta");
    const sharedRoot = path.join(temporaryRoot, "shared-source");
    fs.mkdirSync(sharedRoot);
    fs.writeFileSync(path.join(sharedRoot, "plugin.cpp"), "// Shared native plugin source.\n");
    assert.deepEqual(readBundledModuleRecords(temporaryRoot).map((record) => record.directoryId), ["alpha", "beta"]);

    fs.unlinkSync(path.join(temporaryRoot, "beta", "module.toml"));
    assert.throws(() => verifyGameConfigAcceptanceRegistry(registryInput(readBundledModuleRecords(temporaryRoot))),
      /bundled module directories.*missing: beta/);
    writeModule(temporaryRoot, "beta");
    writeModule(temporaryRoot, "unexpected");
    assert.throws(() => verifyGameConfigAcceptanceRegistry(registryInput(readBundledModuleRecords(temporaryRoot))),
      /bundled module directories.*unexpected: unexpected/);
  });
});

test("fixture cohorts fail closed by default and preserve explicit non-empty targeting", () => {
  const records = [
    { directoryId: "alpha", hasManifest: true, hasSchema: true, manifestId: "alpha", schemaValid: true },
    { directoryId: "beta", hasManifest: true, hasSchema: true, manifestId: "beta", schemaValid: true }
  ];

  assert.throws(
    () => verifyGameConfigAcceptanceRegistry(registryInput(records)),
    /fixtures/i
  );
  assert.throws(
    () => verifyGameConfigAcceptanceRegistry(registryInput(records, {
      fixtureBindings: [{ ownerId: "alpha", declaredId: "alpha" }]
    })),
    /fixtures/i
  );
  assert.throws(
    () => verifyGameConfigAcceptanceRegistry(registryInput(records, {
      fixtureBindings: [{ ownerId: "unknown", declaredId: "unknown" }]
    })),
    /fixture/i
  );
  assert.throws(
    () =>
      verifyGameConfigAcceptanceRegistry(
        registryInput(records, { environment: { REQUIRE_ALL_GAME_CONFIG_ACCEPTANCE: "1" } })
      ),
    /fixtures/i
  );
  assert.deepEqual(
    verifyGameConfigAcceptanceRegistry(
      registryInput(records, {
        fixtureBindings: [
          { ownerId: "alpha", declaredId: "alpha" },
          { ownerId: "beta", declaredId: "beta" }
        ]
      })
    ),
    ["alpha", "beta"]
  );
  assert.deepEqual(
    verifyGameConfigAcceptanceRegistry(
      registryInput(records, {
        fixtureBindings: [
          { ownerId: "alpha", declaredId: "alpha" },
          { ownerId: "beta", declaredId: "beta" }
        ],
        environment: { REQUIRE_ALL_GAME_CONFIG_ACCEPTANCE: "1" }
      })
    ),
    ["alpha", "beta"]
  );
  assert.throws(
    () => verifyGameConfigAcceptanceRegistry(registryInput(records, {
      fixtureBindings: [
        { ownerId: "alpha", declaredId: "beta" },
        { ownerId: "beta", declaredId: "alpha" }
      ],
      environment: { REQUIRE_ALL_GAME_CONFIG_ACCEPTANCE: "1" }
    })),
    /match their module directories/i
  );
});

test("bundled modules, manifest IDs, schema IDs, presentation registry IDs, and fixture IDs stay aligned", () => {
  const records = readBundledModuleRecords(modulesRoot);
  const fixtureBindings = readFixtureBindings(modulesRoot);
  const selectedModuleIds = verifyGameConfigAcceptanceRegistry({
    canonicalModuleIds: CANONICAL_GAME_CONFIG_ACCEPTANCE_MODULE_IDS,
    bundledModuleIds: records.map((record) => record.directoryId),
    manifestBindings: records
      .filter((record) => record.hasManifest)
      .map((record) => ({ ownerId: record.directoryId, declaredId: record.manifestId })),
    schemaRegistryBindings: readSchemaRegistryBindings(moduleAssetsPath),
    presentationRegistryIds: listSettingsModuleIds(),
    fixtureBindings,
    environment: process.env
  });

  if (process.env.REQUIRE_ALL_GAME_CONFIG_ACCEPTANCE === "1") {
    assert.deepEqual(selectedModuleIds, CANONICAL_GAME_CONFIG_ACCEPTANCE_MODULE_IDS);
  }
});
