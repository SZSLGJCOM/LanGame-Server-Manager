const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const root = path.resolve(__dirname, "..");
const context = { locale: "en-US", t: (key, _params, fallback) => fallback ?? key };
for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    const load = module.require.bind(module);
    module.require = (id) => /\/i18n$/.test(id) ? { ...load(id), useI18n: () => context } : load(id);
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("", module.filename);
require.extensions[".png"] = (module, filename) => module._compile(`module.exports = ${JSON.stringify(filename)};`, filename);

const { dontStarveSettingsDefinition: definition } = require("../src/views/settings/modules/dontstarve.ts");
const { DontStarveCavesNotice, DontStarvePresetNotice } = require("../src/views/settings/modules/dontstarve-preset-notice.tsx");
const { parseGuidedSettingsSchema } = require("../src/views/settings/guided-settings.ts");
const { GuidedSettingsForm } = require("../src/views/settings/GuidedSettingsForm.tsx");
const schemaJson = fs.readFileSync(path.resolve(root, "../../modules/dontstarve/schema.json"), "utf8");
const moduleDetails = { summary: { id: "dontstarve", name: "DST" }, schema_json: schemaJson };
const schema = parseGuidedSettingsSchema(moduleDetails, context.locale, context.t);
const modsSchema = parseGuidedSettingsSchema(moduleDetails, context.locale, context.t, { surface: "mods" });
const field = (key) => [...schema.fields, ...modsSchema.fields].find((candidate) => candidate.key === key);
const initialize = (settings) => definition.initializeSettings(settings, context);
const patch = (settings, next) => definition.applySettingsPatch(settings, next, context);
const disabled = (key, settings) => {
  assert.ok(field(key), `the ${key} guided field must be present`);
  return definition.isFieldDisabled(field(key), settings);
};

function button(element) {
  if (!React.isValidElement(element)) return undefined;
  if (element.type === "button") return element;
  if (typeof element.type === "function") return button(element.type(element.props));
  return React.Children.toArray(element.props.children).map(button).find(Boolean);
}

test("ordinary world Lua synchronizes through the real module initialization and edit hooks", () => {
  const original = {
    enable_caves: false,
    master_ocean_bullkelp: "ocean_default",
    master_worldgenoverride_lua: 'return { override_enabled = true, preset = "SURVIVAL_TOGETHER", overrides = { world_size = "default", custom_marker = "preserve" } }',
    caves_worldgenoverride_lua: 'return { override_enabled = true, preset = "DST_CAVE", overrides = { world_size = "default" } }'
  };
  const snapshot = structuredClone(original);
  const projected = initialize(original);
  assert.equal(projected.master_ocean_bullkelp, "default", "an absent Lua key does not preserve a previously ineffective form value");
  assert.equal(disabled("master_day", projected), false);
  assert.deepEqual(definition.getSettingsValidationIssues(projected, context), []);
  const edited = patch(projected, { master_day: "onlyday" });
  assert.equal(initialize(edited).master_day, "onlyday");
  assert.match(edited.master_worldgenoverride_lua, /custom_marker/);
  assert.match(edited.master_worldgenoverride_lua, /preserve/);
  assert.equal(edited.caves_worldgenoverride_lua, original.caves_worldgenoverride_lua);
  assert.equal(edited.enable_caves, false);
  assert.deepEqual(original, snapshot, "initialization and editing do not mutate their input");
  assert.equal(disabled("master_world_overrides_extra", edited), true, "an overridden extra input cannot accept ineffective edits");
  assert.equal(disabled("master_worldgenoverride_lua", edited), false);
});

test("Caves controls retain values and only an explicit enable action activates them", () => {
  const settings = { enable_caves: false, caves_weather: "often", caves_enabled_workshop_mod_ids: "123456789" };
  const before = structuredClone(settings);
  for (const key of ["caves_enabled_workshop_mod_ids", "master_enabled_workshop_mod_ids"]) {
    assert.equal(schema.fields.some((candidate) => candidate.key === key), false);
    assert.ok(modsSchema.fields.some((candidate) => candidate.key === key));
  }
  for (const key of ["caves_weather", "caves_worldgen_preset", "caves_enabled_workshop_mod_ids", "caves_worldgenoverride_lua"]) {
    assert.equal(disabled(key, settings), true, key);
  }
  for (const key of ["enable_caves", "master_day", "master_enabled_workshop_mod_ids"]) {
    assert.equal(disabled(key, settings), false, key);
  }
  const html = renderToStaticMarkup(React.createElement(GuidedSettingsForm, {
    schema, settings, moduleDetails, selectedSectionId: "cavessettings", onChange() {}, onBatchChange() {}
  }));
  assert.match(html, /id="configuration-dontstarve-caves-weather-input"[^>]*disabled/);
  assert.match(html, /<option[^>]*selected=""[^>]*>Often<\/option>/);
  let update;
  const notice = DontStarveCavesNotice({ settings, disabled: false, onPatch: (value) => { update = value; } });
  assert.match(renderToStaticMarkup(notice), /settings are preserved/);
  assert.equal(update, undefined);
  assert.deepEqual(settings, before);
  button(notice).props.onClick();
  assert.deepEqual(update, { enable_caves: true });
  const enabled = patch(settings, update);
  assert.equal(enabled.caves_weather, "often");
  assert.equal(enabled.caves_enabled_workshop_mod_ids, "123456789");
  assert.equal(disabled("caves_weather", enabled), false);
  assert.equal(DontStarveCavesNotice({ settings: enabled }), null);
});

test("complex scripts disable only their world controls and link to the exact active Lua field", () => {
  for (const shard of ["master", "caves"]) {
    for (const source of ["worldgenoverride_lua", "world_overrides_extra"]) {
      const key = `${shard}_${source}`;
      const settings = { enable_caves: true, [key]: source === "worldgenoverride_lua" ? "return make_world()" : "day = choose_day()," };
      assert.equal(disabled(shard === "master" ? "master_day" : "caves_weather", settings), true);
      assert.equal(disabled(shard === "master" ? "caves_weather" : "master_day", settings), false);
      assert.equal(disabled("enable_caves", settings), false);
      assert.equal(disabled(key, settings), false);
      let target;
      const notice = DontStarvePresetNotice({
        settings, sectionId: `${shard}gen`, disabled: false,
        onNavigateField: (value) => { target = value; }
      });
      assert.match(renderToStaticMarkup(notice), /other settings remain editable/);
      button(notice).props.onClick();
      assert.equal(target, key);
      assert.deepEqual(definition.getSettingsValidationIssues(settings, context), []);
    }
  }
});

test("inactive Caves fields do not block the workspace autosave gate, but active fields still validate", () => {
  const filename = path.join(root, "src/views/settings/ConfigurationWorkspace.tsx");
  const loaded = new Module(filename, module);
  loaded.filename = filename;
  const load = Module.createRequire(filename);
  let saveBlocked;
  loaded.require = (id) => id === "../../i18n" ? { ...load(id), useI18n: () => context }
    : id === "./useAutoSaveInstanceSettings" ? {
      useAutoSaveInstanceSettings(options) { saveBlocked = options.disabled; return { status: { state: "saved" }, retry() {} }; }
    } : id === "./useModuleConfigurationIcons" ? {
      useModuleConfigurationIcons: () => ({ icons: {}, error: null, missing: false, loading: false, retryAvailable: false })
    } : id === "./useInstancePortRegistration" ? {
      useInstancePortRegistration: () => ({ ports: [], defaultPorts: [], setPorts() {} })
    } : load(id);
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  function render(settings) {
    return renderToStaticMarkup(React.createElement(loaded.exports.ConfigurationWorkspace, {
      details: {
        summary: { id: "dst-test", module_id: "dontstarve", name: "DST", bind_ip: "0.0.0.0", autostart: false },
        settings_json: JSON.stringify(settings), ports: [], backup_retention_count: 5,
        config_file_path: "D:/instances/dst-test/instance.json"
      },
      moduleDetails, bindAddressCandidates: [], runtime: null, launchPlan: null, launchPlanError: null,
      onSave() {}
    }));
  }
  render({ enable_caves: false, caves_settings_preset: "invalid preset id", caves_mod_configuration_options: { "123456789": { retained: null } } });
  assert.equal(saveBlocked, false, "disabled Caves schema and module issues must both be excluded");
  render({ enable_caves: true, caves_settings_preset: "invalid preset id" });
  assert.equal(saveBlocked, true, "enabling the shard restores validation of the active values");
  render({ enable_caves: false, master_settings_preset: "invalid preset id" });
  assert.equal(saveBlocked, true, "inactive Caves must not suppress a Master validation error");
  const presetLimit = JSON.parse(schemaJson).properties.caves_settings_preset.maxLength;
  assert.ok(presetLimit > 0);
  render({ enable_caves: false, caves_settings_preset: "a".repeat(presetLimit + 1) });
  assert.equal(saveBlocked, true, "retained inactive settings still respect the storage size limit");
});
