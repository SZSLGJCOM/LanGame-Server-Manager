const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const root = path.resolve(__dirname, "..", "..", "..");
const desktopRoot = path.join(root, "apps", "desktop");
const settingsRoot = path.join(desktopRoot, "src", "views", "settings");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    module._compile(transpileTypeScript(source, filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("", module.filename);

const {
  listSettingsModuleIds,
  resolveSettingsModuleDefinition
} = require(path.join(settingsRoot, "module-registry.ts"));
const {
  resolveConfigurationFieldPresentation,
  resolveConfigurationRendererContract
} = require(path.join(settingsRoot, "configuration-presentation.ts"));

function readSchema(moduleId) {
  return JSON.parse(fs.readFileSync(path.join(root, "modules", moduleId, "schema.json"), "utf8"));
}

test("operational Mod lists and DST options belong to the Mods workspace", () => {
  const expected = new Map([
    ["arksurvivalascended.mod_ids_csv", "mod-workbench-asa-mod-ids"],
    ["arksurvivalevolved.active_mod_ids", "mod-workbench-workshop-ids"],
    ["barotrauma.mod_workshop_ids", "mod-workbench-workshop-ids"],
    ["conanexiles.mod_workshop_ids", "mod-workbench-workshop-ids"],
    ["palworld.mod_package_names", "mod-workbench-package-names"],
    ["projectzomboid.workshop_items", "mod-workbench-workshop-ids"],
    ["projectzomboid.mods", "mod-workbench-mod-ids"],
    ["projectzomboid.map_name", "mod-workbench-map-order"],
    ["soulmask.mod_workshop_ids", "mod-workbench-workshop-ids"],
    ["terraria.tmodloader_workshop_item_ids", "mod-workbench-workshop-ids"],
    ["unturned.workshop_file_ids", "mod-workbench-workshop-ids"],
    ...[
      "shared_workshop_mod_ids", "shared_workshop_collection_ids",
      "master_enabled_workshop_mod_ids", "caves_enabled_workshop_mod_ids",
      "islands_enabled_workshop_mod_ids", "volcano_enabled_workshop_mod_ids",
      "master_mod_configuration_options", "caves_mod_configuration_options",
      "islands_mod_configuration_options", "volcano_mod_configuration_options"
    ].map((key) => [`dontstarve.${key}`, "mod-workbench-dst-mods"])
  ]);
  const observed = [];
  for (const moduleId of listSettingsModuleIds()) {
    const definition = resolveSettingsModuleDefinition(moduleId);
    for (const [fieldKey, property] of Object.entries(readSchema(moduleId).properties ?? {})) {
      const presentation = resolveConfigurationFieldPresentation(fieldKey, property, definition);
      if (presentation.owner !== "mods") continue;
      const qualifiedKey = `${moduleId}.${fieldKey}`;
      observed.push(qualifiedKey);
      assert.equal(presentation.state, "specialized", qualifiedKey);
      assert.equal(presentation.rendererId, expected.get(qualifiedKey), qualifiedKey);
      assert.deepEqual(resolveConfigurationRendererContract(presentation.rendererId, definition), {
        kind: "workspace",
        workspace: "mods"
      });
    }
  }
  assert.deepEqual(observed.sort(), [...expected.keys()].sort());
});

test("every manifest-declared Mod enablement field belongs to the Mods workspace", () => {
  for (const moduleId of listSettingsModuleIds()) {
    const manifest = fs.readFileSync(path.join(root, "modules", moduleId, "module.toml"), "utf8");
    const enablement = manifest.split(/^\[mods\.enablement\]\s*$/m)[1]?.split(/^\[/m)[0];
    if (!enablement) continue;
    const fieldKey = enablement.match(/^setting_key\s*=\s*"([^"]+)"/m)?.[1];
    assert.ok(fieldKey, `${moduleId} must declare its Mod enablement setting`);
    const presentation = resolveConfigurationFieldPresentation(
      fieldKey, readSchema(moduleId).properties[fieldKey], resolveSettingsModuleDefinition(moduleId)
    );
    assert.equal(presentation.owner, "mods", `${moduleId}.${fieldKey}`);
  }
});

test("native mod-related server parameters remain editable only in Configuration", () => {
  for (const qualifiedKey of [
    "arksurvivalascended.passive_mod_ids_csv",
    "dontstarve.master_modoverrides_lua",
    "dontstarve.caves_modoverrides_lua",
    "dontstarve.islands_modoverrides_lua",
    "dontstarve.volcano_modoverrides_lua",
    "arksurvivalevolved.auto_managed_mod_ids",
    "arksurvivalevolved.auto_managed_mods",
    "palworld.allow_client_mod",
    "rimworld.steam_workshop_url",
    "terraria.tmodloader_runtime_dir",
    "terraria.tmodloader_enabled_mod_names",
    "unturned.workshop_ignore_children_file_ids",
    "unturned.workshop_query_cache_max_age_seconds",
    "unturned.workshop_max_query_retries",
    "unturned.workshop_use_cached_downloads",
    "unturned.workshop_monitor_updates"
  ]) {
    const [moduleId, fieldKey] = qualifiedKey.split(".");
    const presentation = resolveConfigurationFieldPresentation(
      fieldKey,
      readSchema(moduleId).properties[fieldKey],
      resolveSettingsModuleDefinition(moduleId)
    );
    assert.equal(presentation.owner, "configuration", qualifiedKey);
    assert.equal(presentation.state, "editable", qualifiedKey);
  }

  const modWorkbenchSource = ["ModWorkbench.tsx", "mod-workbench-model.ts", "mod-workbench-plans.ts"]
    .map((fileName) => fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", fileName), "utf8"))
    .join("\n");
  assert.doesNotMatch(modWorkbenchSource, /palworld-client-mods|settings\.allow_client_mod/);
});
