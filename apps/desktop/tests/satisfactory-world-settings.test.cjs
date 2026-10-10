const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) =>
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}
require.extensions[".css"] = (module) => module._compile("", module.filename);
const native = JSON.parse(fs.readFileSync(path.resolve(__dirname, "../../../modules/satisfactory/world-settings.json"), "utf8"));
const bridge = require("../src/satisfactory-world-settings.ts");
const { parseGuidedSettingsSchema } = require("../src/views/settings/guided-settings.ts");
const { buildConfigurationWorkspaceModel } = require("../src/views/settings/configuration-workspace-model.ts");
const { satisfactorySettingsDefinition } = require("../src/views/settings/modules/satisfactory.ts");
const { SatisfactoryWorldSettingsProvider } = require("../src/views/settings/SatisfactoryWorldSettingsContext.tsx");
const { SatisfactoryNativeField } = require("../src/views/settings/SatisfactoryNativeFields.tsx");
const { readSatisfactoryNativeOption } = require("../src/views/settings/SatisfactoryNativeOptionField.tsx");
const schemaJson = fs.readFileSync(path.resolve(__dirname, "../../../modules/satisfactory/schema.json"), "utf8");
const energy = "FG.GameMode.EnergyCostMultiplier";
const purity = "FG.GameMode.NodePuritySettings";
const seed = "FG.GameMode.NodeRandomizationSeed";
const noPower = "FG.GameRules.NoPower";
const rule = (key) => bridge.SATISFACTORY_RULES.find((entry) => entry.key === key);

test("Satisfactory native option values preserve types and never invent unread defaults", () => {
  const boolean = { type: "boolean" };
  for (const value of ["True", "true", "1"]) assert.equal(readSatisfactoryNativeOption(boolean, value), true);
  for (const value of ["False", "false", "0"]) assert.equal(readSatisfactoryNativeOption(boolean, value), false);
  for (const value of [undefined, "", "unknown"]) assert.equal(readSatisfactoryNativeOption(boolean, value), undefined);
  const enumeration = { type: "integer", enumOptions: [{ value: 0 }, { value: 2 }, { value: 3 }] };
  assert.equal(readSatisfactoryNativeOption(enumeration, "3"), 3);
  assert.equal(readSatisfactoryNativeOption(enumeration, "8"), undefined);
});

test("Satisfactory native confirmations and rules render explicit two-state switches", () => {
  const React = require("react");
  const { renderToStaticMarkup } = require("react-dom/server");
  const { I18nContext } = require("../src/i18n-context.ts");
  for (const value of [false, true]) {
    const html = renderToStaticMarkup(React.createElement(I18nContext.Provider, {
      value: { locale: "en-US", t: (key, _params, fallback) => fallback ?? key }
    }, React.createElement(SatisfactoryNativeField, {
      fieldKey: "satisfactory_confirm_create", sectionId: "world_generation", title: "Create and load this separate world",
      kind: "boolean", value, onChange() {}
    })));
    assert.match(html, /type="checkbox"/);
    assert.doesNotMatch(html, /<select|Use game default/);
    assert.equal(html.includes('checked=""'), value);
  }
});

test("Satisfactory retains native percentages and the nonsequential purity enum", () => {
  assert.deepEqual(rule(energy).options.map((entry) => entry.value), ["25", "50", "75", "100", "200", "500"]);
  assert.deepEqual(rule(purity).options.map((entry) => entry.value), ["0", "1", "5", "2", "6", "3", "4"]);
  assert.equal(bridge.SATISFACTORY_RULES.filter((entry) => entry.key.startsWith("FG.GameMode.")).length, 6);
  assert.equal(bridge.SATISFACTORY_RULES.filter((entry) => entry.scope === "world").length, 5);
  assert.equal(bridge.SATISFACTORY_RULES.filter((entry) => entry.scope === "player_defaults").length, 3);
  for (const value of ["0.25", "1.0", "101", "-1"]) assert.equal(bridge.isSatisfactoryRuleValueValid(rule(energy), value), false);
  for (const value of ["5", "6", "0"]) assert.equal(bridge.isSatisfactoryRuleValueValid(rule(purity), value), true);
  for (const value of ["True", "False"]) assert.equal(bridge.isSatisfactoryRuleValueValid(rule(noPower), value), true);
  for (const value of ["true", "1", "0", ""]) assert.equal(bridge.isSatisfactoryRuleValueValid(rule(noPower), value), false);
  for (const value of ["0", "-2147483648", "2147483647"]) assert.equal(bridge.isSatisfactoryRuleValueValid(rule(seed), value), true);
  for (const value of ["-2147483649", "2147483648", "1.5", "NaN"]) assert.equal(bridge.isSatisfactoryRuleValueValid(rule(seed), value), false);
});

test("Satisfactory creation separates generation from starting progress and skips defaults", () => {
  assert.deepEqual(bridge.splitSatisfactoryCreationRules({}), { game_mode_settings: {}, advanced_game_settings: {} });
  assert.deepEqual(bridge.splitSatisfactoryCreationRules({ [energy]: "25", [purity]: "5", [seed]: "-12345",
    "FG.GameRules.StartingTier": "10", "FG.GameRules.GiveAllTiers": "True" }), {
    game_mode_settings: { [energy]: "25", [purity]: "5", [seed]: "-12345" },
    advanced_game_settings: { "FG.GameRules.GiveAllTiers": "True", "FG.GameRules.StartingTier": "10" }
  });
  assert.throws(() => bridge.splitSatisfactoryCreationRules({ [energy]: "0.25" }), /Invalid Satisfactory rule/);
});

test("Satisfactory existing-world patches retain unknown settings and reject creation-only changes", () => {
  const original = { advanced_game_settings: { [noPower]: "True", "FG.Future.Unknown": "retained" } };
  assert.deepEqual(bridge.buildSatisfactoryRulesPatch(original, { [noPower]: "True" }), {});
  assert.deepEqual(bridge.buildSatisfactoryRulesPatch(original, { [noPower]: "False" }), { [noPower]: "False" });
  assert.equal(original.advanced_game_settings["FG.Future.Unknown"], "retained");
  for (const values of [{ [energy]: "25" }, { [noPower]: "1" }, { "FG.Future.Unknown": "False" }]) {
    assert.throws(() => bridge.buildSatisfactoryRulesPatch(original, values), /Invalid Satisfactory rule/);
  }
});

test("Satisfactory native settings have one shared snapshot and dedicated configuration ownership", () => {
  const schema = parseGuidedSettingsSchema({ summary: { id: "satisfactory", name: "Satisfactory" }, schema_json: schemaJson },
    "en-US", (key, _params, fallback) => fallback ?? key);
  assert.equal(schema.parseError, null);
  assert.equal(satisfactorySettingsDefinition.workspaceProvider, SatisfactoryWorldSettingsProvider);
  assert.ok(buildConfigurationWorkspaceModel(schema));
  for (const field of native.settings) {
    const key = field.scope === "creation" ? "native_world_generation" : "native_creative_rules";
    const presentation = schema.presentationFields.find((entry) => entry.key === key);
    assert.ok(presentation, field.key);
    assert.equal(presentation.presentation.owner, "configuration");
    assert.equal(presentation.sectionId, field.scope === "creation" ? "world_generation" : "creative_rules");
    assert.ok(presentation.presentation.aliases.includes(field.key), "native rule keys remain searchable through their owning panel");
    assert.equal(Object.hasOwn(JSON.parse(schemaJson).properties, bridge.satisfactoryRuleId(field.key)), false,
      "native virtual controls never become INI fields");
  }
});

test("Satisfactory native bridge preserves identity, revision and explicit mutation arguments", async () => {
  const previousWindow = globalThis.window;
  const hadIsTauri = Object.hasOwn(globalThis, "isTauri");
  const previousIsTauri = globalThis.isTauri;
  const calls = [];
  globalThis.isTauri = true;
  globalThis.window = { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command, args) => {
    calls.push({ command, args });
    return { instance_id: "api-fixture" };
  } } };
  try {
    const input = { instance_id: "api-fixture", expected_revision: "native-revision" };
    for (const [fn, command, args] of [
      [bridge.readSatisfactoryWorldSettings, "read_satisfactory_world_settings", "api-fixture"],
      [bridge.setupSatisfactoryServer, "setup_satisfactory_server", { instance_id: input.instance_id, server_name: "Private", admin_password: null }],
      [bridge.authorizeSatisfactoryServer, "authorize_satisfactory_server", { instance_id: input.instance_id, admin_password: "fixture-password" }],
      [bridge.writeSatisfactoryRoom, "write_satisfactory_room", { ...input, server_name: "Private", client_password: null, auto_load_session_name: null }],
      [bridge.writeSatisfactoryWorldRules, "write_satisfactory_world_rules", { ...input, acknowledge_enable_advanced_settings: true, advanced_game_settings: { [noPower]: "True" } }],
      [bridge.createSatisfactoryWorld, "create_satisfactory_world", { ...input, session_name: "Fresh", starting_location: "Grass Fields", skip_onboarding: true, acknowledge_enable_advanced_settings: false, game_mode_settings: {}, advanced_game_settings: {} }],
      [bridge.loadSatisfactorySave, "load_satisfactory_save", { ...input, save_name: "Private.auto" }],
      [bridge.readSatisfactoryAdminPassword, "read_satisfactory_admin_password", "api-fixture"]
    ]) {
      await fn(args);
      assert.deepEqual(calls.at(-1), { command, args: typeof args === "string" ? { instanceId: args } : { input: args } });
    }
  } finally {
    if (previousWindow === undefined) delete globalThis.window; else globalThis.window = previousWindow;
    if (hadIsTauri) globalThis.isTauri = previousIsTauri; else delete globalThis.isTauri;
  }
});
