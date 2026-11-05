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
require.extensions[".css"] = (module) => module._compile("", module.filename);

const { parseGuidedSettingsSchema, readGuidedFieldValue, writeGuidedFieldValue } = require("../src/views/settings/guided-settings.ts");
const { ConfigurationField } = require("../src/views/settings/ConfigurationField.tsx");
const { buildConfigurationWorkspaceModel, configurationNavigationRoots } = require("../src/views/settings/configuration-workspace-model.ts");
const { resolveSettingsModuleDefinition } = require("../src/views/settings/module-registry.ts");
const { ModSourceListEditors } = require("../src/views/servers/ModSourceListEditors.tsx");
const { InstanceSettingsSaveProvider } = require("../src/views/settings/InstanceSettingsSaveContext.tsx");
const { I18nContext } = require("../src/i18n-context.ts");
const { EN_US_ABIOTIC_FACTOR_MESSAGES } = require("../src/i18n/games/abioticfactor.en.ts");
const { ZH_CN_ABIOTIC_FACTOR_MESSAGES } = require("../src/i18n/games/abioticfactor.zh-cn.ts");
const schemaJson = fs.readFileSync(path.resolve(__dirname, "../../../modules/abioticfactor/schema.json"), "utf8");
const definition = resolveSettingsModuleDefinition("abioticfactor");
const modKeys = ["mod_ids", "mods_directory", "mod_install_timeout_seconds", "override_mod_warnings", "sideload_mods", "sideload_mods_path"];
const hostKeys = ["modio_token", "allow_local_only_mods"];
const moduleDetails = { summary: { id: "abioticfactor", name: "Abiotic Factor" }, schema_json: schemaJson };

function localized(locale, catalog) {
  const t = (key, _params, fallback) => catalog[key] ?? fallback ?? key;
  const schema = parseGuidedSettingsSchema(moduleDetails, locale, t);
  const mods = parseGuidedSettingsSchema(moduleDetails, locale, t, { surface: "mods" });
  assert.equal(schema.parseError, null);
  assert.equal(mods.parseError, null);
  return { t, schema, mods, fields: new Map([...schema.fields, ...mods.fields].map((field) => [field.key, field])) };
}

test("Abiotic keeps admission and the protected token in Access and edits its distribution settings in My Mods", () => {
  for (const [locale, catalog] of [["en-US", EN_US_ABIOTIC_FACTOR_MESSAGES], ["zh-CN", ZH_CN_ABIOTIC_FACTOR_MESSAGES]]) {
    const { t, schema, mods, fields } = localized(locale, catalog);
    const model = buildConfigurationWorkspaceModel(schema);
    const navigation = configurationNavigationRoots(model.roots);
    assert.ok(navigation.every((section) => section.id !== "official-mods" && section.id !== "mods"));
    assert.equal(model.actionableSectionIds.includes("mods"), false);
    for (const key of modKeys) {
      assert.equal(schema.fields.some((field) => field.key === key), false, key);
      assert.equal(model.items.find((item) => item.fieldKey === key)?.owner, "mods", key);
    }
    assert.deepEqual(mods.fields.map((field) => field.key).sort(), [...modKeys].sort());
    for (const key of hostKeys) {
      const field = fields.get(key);
      assert.ok(field, key);
      assert.equal(field.presentation.owner, "configuration");
      assert.equal(field.presentation.state, "editable");
      assert.equal(field.sectionId, "access");
      assert.equal(field.title, catalog[`settings.schema.abioticfactor.${key}.title`]);
      const markup = renderToStaticMarkup(React.createElement(ConfigurationField, {
        field, settings: {}, value: readGuidedFieldValue(field, {}), onPatch() {}, t,
        copy: { revealSecret: "Show value", concealSecret: "Hide value", restartScopes: { server: "Restart server" } }
      }));
      assert.ok(markup.includes(field.title), key);
      if (key === "modio_token") {
        assert.match(markup, /type="password"/);
        assert.equal(field.presentation.resourceUrl, "https://mod.io/me/access#tokens");
      }
    }
    assert.equal(fields.get("allow_local_only_mods").defaultValue, true);
    assert.equal(fields.get("mod_install_timeout_seconds").defaultValue, 900);
    assert.equal(fields.get("mods_directory").defaultValue, "AbioticFactor/Saved/mod.io");
    assert.equal(fields.get("sideload_mods_path").defaultValue, "AbioticFactor/Mods");
  }
});

test("My Mods includes the bilingual native form and safe public catalog, keeping the saved credential out of the page", () => {
  for (const [locale, catalog] of [["en-US", EN_US_ABIOTIC_FACTOR_MESSAGES], ["zh-CN", ZH_CN_ABIOTIC_FACTOR_MESSAGES]]) {
    const { t } = localized(locale, catalog);
    const markup = renderToStaticMarkup(React.createElement(I18nContext.Provider,
      { value: { locale, setLocale() {}, t } }, React.createElement(InstanceSettingsSaveProvider, null,
        React.createElement(ModSourceListEditors, {
          moduleDetails, instanceId: "abiotic-fixture", settings: { modio_token: "private-fixture-credential" },
          disabled: false, readOnly: false, onSave: async () => { throw new Error("render only"); }
        }))));
    for (const key of modKeys) {
      assert.ok(markup.includes(`data-field-key="${key}"`));
      assert.ok(markup.includes(catalog[`settings.schema.abioticfactor.${key}.title`]));
    }
    assert.match(markup, /<textarea/);
    assert.ok(markup.includes(catalog["abiotic.settings.mods.browseCatalog"]));
    assert.match(markup, /href="https:\/\/mod\.io\/g\/abiotic-factor"/);
    assert.match(markup, /target="_blank"/);
    assert.match(markup, /rel="noopener noreferrer"/);
    assert.match(markup, /aria-description="Open the official page in your browser"/);
    assert.doesNotMatch(markup, /private-fixture-credential|modio_token|allow_local_only_mods|me\/access#tokens/);
  }
});

test("Abiotic Mod edits remain real instance settings and reject unsafe or unusable combinations", () => {
  const { t, fields } = localized("en-US", EN_US_ABIOTIC_FACTOR_MESSAGES);
  const valid = writeGuidedFieldValue({}, fields.get("mod_ids"), "6472275;6472552\n6456360");
  assert.equal(valid.mod_ids, "6472275;6472552\n6456360");
  const validate = (key, value, settings = {}) => definition.getFieldValidationMessage({
    field: fields.get(key), value, settings: { ...settings, [key]: value }, locale: "en-US", t
  });
  assert.equal(validate("mod_ids", valid.mod_ids), undefined);
  for (const ids of ["6472275;bad", "0", "+1", "18446744073709551616", "6472275 -SideloadMods"]) {
    assert.ok(validate("mod_ids", ids));
  }
  assert.ok(validate("mod_ids", "", { sideload_mods: true }));
  assert.ok(validate("sideload_mods", true, { mod_ids: "" }));
  assert.ok(validate("modio_token", "fixture\n-token"));
  for (const directory of ["D:/shared", "../other", "AbioticFactor/Saved/../Mods", "AbioticFactor/Saved/NUL", "AbioticFactor/Saved/cache."]) {
    assert.ok(validate("mods_directory", directory));
  }
  assert.ok(validate("mods_directory", "AbioticFactor/Saved/custom", { mod_ids: "6472275" }));
  assert.ok(validate("modio_token", "", { mods_directory: "AbioticFactor/Saved/custom", mod_ids: "6472275" }));
  assert.equal(validate("mods_directory", "AbioticFactor/Saved/custom", { mod_ids: "6472275", modio_token: "fixture-token" }), undefined);
  assert.equal(validate("sideload_mods_path", "AbioticFactor/Mods/curated"), undefined);
  assert.ok(validate("sideload_mods_path", "AbioticFactor/Saved/cache"));
});

test("Abiotic retains its separate unofficial UE4SS importer and isolated native Mod folders", () => {
  const manifest = fs.readFileSync(path.resolve(__dirname, "../../../modules/abioticfactor/module.toml"), "utf8");
  assert.match(manifest, /provider = "nexus"/);
  assert.match(manifest, /target_label = "AbioticFactor\/Binaries\/Win64\/ue4ss\/Mods"/);
  assert.match(manifest, /"AbioticFactor\/Mods"/);
  assert.match(manifest, /[Oo]fficial mod\.io Mods[^\n]*in My Mods/);
});
