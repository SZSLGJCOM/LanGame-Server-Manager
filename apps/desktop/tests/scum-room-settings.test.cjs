const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("module.exports = {};", module.filename);

const { I18nContext } = require("../src/i18n-context.ts");
const { parseGuidedSettingsSchema } = require("../src/views/settings/guided-settings.ts");
const {
  buildConfigurationWorkspaceModel,
  resolveConfigurationSectionId,
  searchConfigurationItems
} = require("../src/views/settings/configuration-workspace-model.ts");
const { scumSettingsDefinition } = require("../src/views/settings/modules/scum.ts");
const { validateConfigurationPresentationDefinition } = require("../src/views/settings/configuration-presentation.ts");
const {
  SCUM_EDITABLE_SETTINGS,
  initializeScumSettings,
  patchScumNativeValue,
  validateScumStructuredSettings
} = require("../src/views/settings/scum-server-settings-inventory.ts");

const roomKeys = [
  "server_name", "server_description", "server_password", "max_players", "welcome_message",
  "server_banner_url", "server_playstyle", "message_of_the_day", "message_of_the_day_cooldown"
].map((key) => `server_general.${key}`);
const translate = (key, _params, fallback) => fallback ?? key;
const moduleDetails = {
  summary: { id: "scum", name: "SCUM" },
  schema_json: fs.readFileSync(path.resolve(__dirname, "../../../modules/scum/schema.json"), "utf8")
};

function configurationModel() {
  const schema = parseGuidedSettingsSchema(moduleDetails);
  assert.equal(schema.parseError, null);
  validateConfigurationPresentationDefinition({
    definition: scumSettingsDefinition,
    properties: JSON.parse(moduleDetails.schema_json).properties,
    sections: schema.sections.map((section, index) => ({ ...section, order: section.order ?? index }))
  });
  return buildConfigurationWorkspaceModel(schema);
}

function renderSection(sectionId, settings = initializeScumSettings({})) {
  const registration = Object.values(scumSettingsDefinition.specializedRenderers)
    .find((renderer) => renderer.kind === "module-addon" && renderer.sectionId === sectionId);
  assert.ok(registration, `${sectionId} requires a registered renderer`);
  return renderToStaticMarkup(React.createElement(I18nContext.Provider, {
    value: { locale: "en-US", setLocale() {}, t: translate }
  }, React.createElement(registration.Renderer, {
    sectionId,
    fieldKey: registration.fieldKey,
    details: { summary: { status: "stopped" } },
    moduleDetails,
    settings,
    disabled: false,
    onPatch() {}
  })));
}

function renderedFieldKeys(html) {
  return [...html.matchAll(/data-field-key="([^"]+)"/gu)].map((match) => match[1]);
}

test("SCUM opens room settings and routes each identity field to a matching native renderer", () => {
  const model = configurationModel();
  assert.equal(resolveConfigurationSectionId(model), "room");
  assert.deepEqual(model.items.filter((item) => item.sectionId === "room").map((item) => item.fieldKey), roomKeys);
  for (const fieldKey of roomKeys) {
    const result = searchConfigurationItems(model, fieldKey, "en-US");
    assert.equal(result.find((item) => item.fieldKey === fieldKey)?.sectionId, "room", fieldKey);
    const item = model.items.find((candidate) => candidate.fieldKey === fieldKey);
    const registration = scumSettingsDefinition.specializedRenderers[item.field.presentation.rendererId];
    assert.equal(registration.sectionId, "room", fieldKey);
    assert.equal(item.field.presentation.rendererFieldKey, "server_general", fieldKey);
  }
  assert.equal(model.items.find((item) => item.fieldKey === "server_general.max_ping").sectionId, "network");
});

test("SCUM semantic sections render disjoint controls while preserving every native General setting", () => {
  const roomHtml = renderSection("room");
  const generalHtml = renderSection("general");
  const roomFields = renderedFieldKeys(roomHtml);
  const generalFields = renderedFieldKeys(generalHtml);
  assert.deepEqual(roomFields, roomKeys);
  assert.ok(generalFields.includes("server_general.allow_voting"));
  assert.ok(roomFields.every((key) => !generalFields.includes(key)));
  const otherFields = ["network", "access", "runtime"].flatMap((id) => renderedFieldKeys(renderSection(id)));
  assert.ok(otherFields.includes("server_general.max_ping"));
  assert.ok(!otherFields.includes("server_general.full_wipe"));
  const allFields = [...roomFields, ...generalFields, ...otherFields];
  assert.equal(allFields.filter((key) => key.startsWith("server_general.")).length, 62);
  assert.equal(new Set(allFields).size, allFields.length);
  assert.doesNotMatch(roomHtml, /guided-field-group-title|type="search"/u);
  assert.match(roomHtml, /type="password"/u);
  assert.doesNotMatch(generalHtml, /type="password"/u);
});

test("SCUM room edits preserve native storage and validation navigates back to the room field", () => {
  const settings = initializeScumSettings({ server_general: { server_name: "Existing room", max_ping: 275 } });
  const original = structuredClone(settings);
  const name = SCUM_EDITABLE_SETTINGS.find((setting) => setting.key === "server_name");
  const patch = patchScumNativeValue(settings, name, "Updated room");
  assert.deepEqual(Object.keys(patch), ["server_general"]);
  assert.equal(patch.server_general.server_name, "Updated room");
  assert.equal(patch.server_general.max_ping, 275);
  assert.deepEqual(settings, original);

  const capacity = SCUM_EDITABLE_SETTINGS.find((setting) => setting.key === "max_players");
  const invalid = { ...settings, ...patchScumNativeValue(settings, capacity, 129) };
  const issue = validateScumStructuredSettings(invalid).find((candidate) => candidate.reason === "maximum");
  assert.equal(issue.fieldKey, "server_general.max_players");
  assert.equal(configurationModel().items.find((item) => item.fieldKey === issue.fieldKey).sectionId, "room");
});
