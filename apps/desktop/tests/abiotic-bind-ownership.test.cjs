const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");

function registerTypeScriptRequireExtension(extension) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    module._compile(transpileTypeScript(source, filename), filename);
  };
}

registerTypeScriptRequireExtension(".ts");
registerTypeScriptRequireExtension(".tsx");
const originalResolveFilename = Module._resolveFilename;
Module._resolveFilename = function resolveRawImports(request, parent, isMain, options) {
  if (typeof request === "string" && request.endsWith("?raw")) {
    const resolved = originalResolveFilename.call(this, request.slice(0, -4), parent, isMain, options);
    return `${resolved}?raw`;
  }
  return originalResolveFilename.call(this, request, parent, isMain, options);
};
require.extensions[".toml?raw"] = function compileRawToml(module, filename) {
  const source = fs.readFileSync(filename.slice(0, -4), "utf8");
  module._compile(`module.exports = ${JSON.stringify(source)};`, filename);
};

const { invokeMock } = require(path.join(desktopRoot, "src", "api-mock.ts"));

test("the mock API loads data models without importing interface components or styles", () => {
  const sourcePrefix = path.join(desktopRoot, "src") + path.sep;
  const interfaceDependencies = Object.keys(require.cache).filter((filename) =>
    filename.startsWith(sourcePrefix) && /\.(?:tsx|css)$/.test(filename));
  assert.deepEqual(interfaceDependencies, []);
});

async function updateNetwork(details, bindIp, settings) {
  return invokeMock("update_instance_record_if_current", {
    input: {
      id: details.summary.id,
      bind_ip: bindIp,
      auto_backup_on_stop: details.auto_backup_on_stop,
      backup_retention_count: details.backup_retention_count,
      settings_json: JSON.stringify(settings),
      ports: details.ports
    },
    expectedSettingsJson: details.settings_json
  });
}

test("Abiotic mock launch matches runtime bind ownership for specific and wildcard addresses", async () => {
  const provisioning = await invokeMock("create_instance_record", {
    input: { name: "Mock Facility", module_id: "abioticfactor" }
  });
  let details = await invokeMock("read_instance_details_from_storage", {
    instanceId: provisioning.summary.id
  });
  const settings = {
    ...JSON.parse(details.settings_json),
    multihome_address: "10.66.0.99",
    use_local_ips: true
  };

  details = await updateNetwork(details, "192.168.31.42", settings);
  let launch = await invokeMock("preview_instance_launch", { instanceId: details.summary.id });
  assert.deepEqual(
    launch.args.filter((arg) => arg.startsWith("-MultiHome=")),
    ["-MultiHome=192.168.31.42"]
  );
  assert.equal(launch.args.filter((arg) => arg === "-UseLocalIPs").length, 1);

  details = await updateNetwork(details, "0.0.0.0", settings);
  launch = await invokeMock("preview_instance_launch", { instanceId: details.summary.id });
  assert.deepEqual(launch.args.filter((arg) => arg.startsWith("-MultiHome=")), []);
  assert.equal(launch.args.filter((arg) => arg === "-UseLocalIPs").length, 1);
});

test("Abiotic obsolete MultiHome translations are absent from both catalog layers", () => {
  const gamesCatalogRoot = path.join(desktopRoot, "src", "i18n", "games");
  const obsoleteKey = "settings.schema.abioticfactor.multihome_address";
  const offenders = fs
    .readdirSync(gamesCatalogRoot)
    .filter((fileName) => fileName.endsWith(".ts"))
    .filter((fileName) => fs.readFileSync(path.join(gamesCatalogRoot, fileName), "utf8").includes(obsoleteKey));

  assert.deepEqual(offenders, []);
});
