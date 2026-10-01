const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const repositoryRoot = path.resolve(__dirname, "..", "..", "..");

function read(...segments) {
  return fs.readFileSync(path.join(repositoryRoot, ...segments), "utf8");
}

function schema(moduleId) {
  return JSON.parse(read("modules", moduleId, "schema.json"));
}

test("advanced extra launch arguments use the shared quote-aware argument resolver", () => {
  for (const moduleId of [
    "barotrauma", "humanitz", "returntomoria", "rimworld", "scum",
    "sonsoftheforest", "soulmask", "squad", "theforest"
  ]) {
    const manifest = read("modules", moduleId, "module.toml");
    assert.match(manifest, /\{\{launch\.extra_args\}\}/, moduleId);
    assert.doesNotMatch(manifest, /\{\{settings\.extra_launch_args\}\}/, moduleId);
  }
  assert.match(
    read("crates", "app-runtime", "src", "launch_templates.rs"),
    /strip_prefix\("launch\."\)[\s\S]*lookup_extra_launch_args_token/
  );
});

test("7DTD XML string settings use the shared XML attribute escaping token", () => {
  const moduleSchema = schema("sevendaystodie");
  const template = read("modules", "sevendaystodie", "templates", "serverconfig.xml.hbs");
  for (const [fieldKey, property] of Object.entries(moduleSchema.properties)) {
    if (property.type !== "string" || property["x-lsgm-source-surface"] !== "config_file") {
      continue;
    }
    assert.match(
      template,
      new RegExp(`value="\\{\\{xml\\.settings\\.${fieldKey}\\}\\}"`),
      `${fieldKey} must be escaped before entering serverconfig.xml`
    );
  }
});

test("Squad exact-build ServerConfig file inventory has one managed or explicit external owner", () => {
  const inventory = read(
    "modules", "squad", "reference-configs", "server-config-files-build-23797339.txt"
  )
    .split(/\r?\n/)
    .filter((line) => line && !line.startsWith("#"))
    .map((line) => line.split("|")[0]);
  assert.equal(inventory.length, 20);

  const templateRoot = path.join(repositoryRoot, "modules", "squad", "templates");
  const managedFiles = new Set(fs.readdirSync(templateRoot).map((name) => name.replace(/\.hbs$/, "")));
  const externalFiles = new Set(["Bans.cfg", "License.cfg"]);
  assert.deepEqual(new Set([...managedFiles, ...externalFiles]), new Set(inventory));

  const moduleSchema = schema("squad");
  assert.equal(moduleSchema.properties.layer_voting?.["x-lsgm-source-key"], "LayerVoting.cfg lines");
  assert.equal(
    moduleSchema.properties.remote_admin_hosts?.["x-lsgm-source-key"],
    "RemoteAdminListHosts.cfg lines"
  );
  const ledger = read("modules", "squad", "config-sources.toml");
  assert.match(ledger, /source = "bans_cfg"[\s\S]*key = "Bans\.cfg runtime entries"[\s\S]*reason = /);
  assert.match(ledger, /source = "license_cfg"[\s\S]*key = "License\.cfg content"[\s\S]*reason = /);
});
