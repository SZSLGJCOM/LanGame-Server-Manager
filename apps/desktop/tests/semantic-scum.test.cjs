const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("", module.filename);
const { parseGuidedSettingsSchema } = require("../src/views/settings/guided-settings.ts");
const { buildConfigurationWorkspaceModel, resolveConfigurationSectionId } = require("../src/views/settings/configuration-workspace-model.ts");
function modelFor(id) {
  const schema = parseGuidedSettingsSchema({
    summary: { id, name: id },
    schema_json: fs.readFileSync(path.resolve(__dirname, "../../../modules", id, "schema.json"), "utf8")
  });
  assert.equal(schema.parseError, null);
  return buildConfigurationWorkspaceModel(schema);
}
function fieldSections(model) {
  return Object.fromEntries(model.items.map((item) => [item.fieldKey, item.sectionId]));
}

test("SCUM separates native network, permissions and maintenance without changing persistence keys", () => {
  const model = modelFor("scum");
  const sections = fieldSections(model);
  for (const key of ["max_ping_check_enabled", "max_ping", "max_number_of_consecutive_high_ping_readings", "master_server_update_send_interval"])
    assert.equal(sections[`server_general.${key}`], "network", key);
  assert.equal(sections["server_general.play_safe_id_protection"], "access");
  for (const key of ["min_server_tick_rate", "max_server_tick_rate", "log_suicides", "item_virtualization_event_processing_time_budget"])
    assert.equal(sections[`server_general.${key}`], "runtime", key);
  assert.equal(sections["server_features.enable_net_watchdog"], "runtime");
  assert.equal(sections["server_general.allow_voting"], "general");
  assert.equal(sections.extra_launch_args, "runtime");
  for (const key of ["partial_wipe", "gold_wipe", "full_wipe"]) {
    assert.equal(sections[`server_general.${key}`], "maintenance", `${key} belongs to maintenance`);
    assert.equal(model.items.find((item) => item.fieldKey === `server_general.${key}`).owner, "maintenance");
    assert.ok(!model.actionableSectionIds.includes("maintenance"));
  }
});

test("SCUM semantic renderers expose every native control exactly once", () => {
  const React = require("react");
  const { renderToStaticMarkup } = require("react-dom/server");
  const { I18nContext } = require("../src/i18n-context.ts");
  const { scumSettingsDefinition } = require("../src/views/settings/modules/scum.ts");
  const { SCUM_EDITABLE_SETTINGS, initializeScumSettings } = require("../src/views/settings/scum-server-settings-inventory.ts");
  const settings = initializeScumSettings({});
  const nativeKeys = Object.entries(scumSettingsDefinition.specializedRenderers)
    .filter(([id]) => id.startsWith("scum-server-"))
    .flatMap(([, renderer]) => {
      const html = renderToStaticMarkup(React.createElement(I18nContext.Provider, {
        value: { locale: "en-US", setLocale() {}, t: (key, _params, fallback) => fallback ?? key }
      }, React.createElement(renderer.Renderer, {
        sectionId: renderer.sectionId, fieldKey: renderer.fieldKey,
        details: { summary: { status: "stopped" } }, settings, disabled: false, onPatch() {}
      })));
      return [...html.matchAll(/data-scum-native-key="([^"]+)"/gu)].map((match) => match[1]);
    });
  assert.equal(nativeKeys.length, 433);
  assert.equal(new Set(nativeKeys).size, 433);
  assert.deepEqual(nativeKeys.sort(), SCUM_EDITABLE_SETTINGS.filter((setting) => !["partial_wipe", "gold_wipe", "full_wipe"].includes(setting.key)).map((setting) => setting.nativeKey).sort());
});
