const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const root = path.resolve(__dirname, "..", "..", "..");
const modulesDir = path.join(root, "modules");
const registrySource = fs.readFileSync(
  path.join(root, "apps", "desktop", "src", "views", "settings", "module-registry.ts"),
  "utf8"
);
const apiMockModuleAssetsSource = fs.readFileSync(
  path.join(root, "apps", "desktop", "src", "api-mock", "module-assets.ts"),
  "utf8"
);
const storeSyncSource = fs.readFileSync(
  path.join(root, "scripts", "fetch_module_store_data.py"),
  "utf8"
);
const moduleSettingCoverageSource = fs.readFileSync(
  path.join(root, "scripts", "verify_module_setting_coverage.py"),
  "utf8"
) + fs.readFileSync(
  path.join(root, "scripts", "verify_module_setting_coverage_constants.py"),
  "utf8"
);

const requestedSurvivalModules = [
  {
    id: "returntomoria",
    steamAppId: 3349480,
    executable: "MoriaServer.exe",
    requiredSettings: ["server_name", "extra_launch_args"]
  },
  {
    id: "astroneer",
    steamAppId: 728470,
    executable: "AstroServer.exe"
  },
  {
    id: "theforest",
    steamAppId: 556450,
    executable: "TheForestDedicatedServer.exe"
  },
];

function read(filePath) {
  return fs.readFileSync(filePath, "utf8");
}

function assertRecordContains(source, recordName, moduleId) {
  const match = source.match(
    new RegExp(String.raw`const\s+${recordName}\s*:\s*Record<string,\s*[^>]+>\s*=\s*\{([\s\S]*?)\n\};`)
  );
  assert.ok(match, `${recordName} record is missing`);
  assert.match(match[1], new RegExp(`^\\s+${moduleId}:`, "m"), `${recordName} missing ${moduleId}`);
}

test("requested persistent survival dedicated servers are wired through the module surface", () => {
  for (const moduleSpec of requestedSurvivalModules) {
    const moduleRoot = path.join(modulesDir, moduleSpec.id);
    const moduleTomlPath = path.join(moduleRoot, "module.toml");
    const schemaPath = path.join(moduleRoot, "schema.json");
    const ledgerPath = path.join(moduleRoot, "config-sources.toml");
    const settingsDefinitionPath = path.join(
      root,
      "apps",
      "desktop",
      "src",
      "views",
      "settings",
      "modules",
      `${moduleSpec.id}.ts`
    );
    const templatesDir = path.join(moduleRoot, "templates");

    assert.ok(fs.existsSync(moduleTomlPath), `${moduleSpec.id} module.toml missing`);
    assert.ok(fs.existsSync(schemaPath), `${moduleSpec.id} schema.json missing`);
    assert.ok(fs.existsSync(ledgerPath), `${moduleSpec.id} config-sources.toml missing`);
    assert.ok(fs.existsSync(settingsDefinitionPath), `${moduleSpec.id} settings definition missing`);
    assert.ok(
      fs.existsSync(templatesDir) && fs.readdirSync(templatesDir, { recursive: true }).some((name) => String(name).endsWith(".hbs")),
      `${moduleSpec.id} should render at least one support template`
    );

    const moduleToml = read(moduleTomlPath);
    assert.match(moduleToml, new RegExp(`^id = "${moduleSpec.id}"$`, "m"));
    assert.match(moduleToml, new RegExp(`^steam_app_id = ${moduleSpec.steamAppId}$`, "m"));
    assert.match(moduleToml, new RegExp(`executable = "${moduleSpec.executable.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}"`));
    assert.match(moduleToml, /\[player_management\]/, `${moduleSpec.id} should declare player-management status`);

    const schema = JSON.parse(read(schemaPath));
    for (const key of moduleSpec.requiredSettings ?? ["server_name", "max_players", "extra_launch_args"]) {
      assert.ok(schema.properties?.[key], `${moduleSpec.id} schema missing ${key}`);
      assert.ok(schema.properties[key]["x-lsgm-source"], `${moduleSpec.id}.${key} missing source metadata`);
    }

    assert.match(registrySource, new RegExp(`${moduleSpec.id}SettingsDefinition`), `${moduleSpec.id} missing settings registry entry`);
    assert.match(apiMockModuleAssetsSource, new RegExp(`schema${moduleSpec.id}`, "i"), `${moduleSpec.id} schema import missing in api mock assets`);
    assert.match(apiMockModuleAssetsSource, new RegExp(`toml${moduleSpec.id}`, "i"), `${moduleSpec.id} toml import missing in api mock assets`);
    assertRecordContains(apiMockModuleAssetsSource, "mockModuleSchemasById", moduleSpec.id);
    assertRecordContains(apiMockModuleAssetsSource, "mockModuleTomlById", moduleSpec.id);
    assert.match(storeSyncSource, new RegExp(`"${moduleSpec.id}"`), `${moduleSpec.id} missing remote store-media app override`);
    assert.match(
      moduleSettingCoverageSource,
      new RegExp(`"${moduleSpec.id}"`),
      `${moduleSpec.id} missing strict module-setting coverage`
    );
  }
});
