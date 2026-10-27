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
const { resolveSettingsModuleDefinition, listConfigurationSpecializedRenderers } = require("../src/views/settings/module-registry.ts");
const { I18nContext } = require("../src/i18n-context.ts");
const { EN_US_ABIOTIC_FACTOR_MESSAGES } = require("../src/i18n/games/abioticfactor.en.ts");
const { ZH_CN_ABIOTIC_FACTOR_MESSAGES } = require("../src/i18n/games/abioticfactor.zh-cn.ts");
const schemaJson = fs.readFileSync(path.resolve(__dirname, "../../../modules/abioticfactor/schema.json"), "utf8");
const definition = resolveSettingsModuleDefinition("abioticfactor");
const keys = ["mod_ids", "modio_token", "mods_directory", "mod_install_timeout_seconds",
  "override_mod_warnings", "sideload_mods", "sideload_mods_path", "allow_local_only_mods"];

function localized(locale, catalog) {
  const t = (key, _params, fallback) => catalog[key] ?? fallback ?? key;
  const schema = parseGuidedSettingsSchema({ summary: { id: "abioticfactor", name: "Abiotic Factor" }, schema_json: schemaJson }, locale, t);
  assert.equal(schema.parseError, null);
  return { t, schema, fields: new Map(schema.fields.map((field) => [field.key, field])) };
}

test("Abiotic official Mods have a rendered bilingual configuration entry and private token control", () => {
  for (const [locale, catalog, sectionTitle] of [
    ["en-US", EN_US_ABIOTIC_FACTOR_MESSAGES, "Official Mods"],
    ["zh-CN", ZH_CN_ABIOTIC_FACTOR_MESSAGES, "官方模组"]
  ]) {
    const { t, schema, fields } = localized(locale, catalog);
    assert.ok(schema.sections.some((section) => section.id === "official-mods" && section.title === sectionTitle));
    const groups = definition.buildFieldGroups("official-mods", schema.fields.filter((field) => field.sectionId === "official-mods"), locale, t);
    assert.deepEqual(groups.flatMap((group) => group.fields.map((field) => field.key)).sort(), [...keys].sort());
    for (const key of keys) {
      const field = fields.get(key);
      assert.ok(field, key);
      assert.equal(field.presentation.owner, "configuration");
      assert.equal(field.presentation.state, "editable");
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
      if (key === "mod_ids") {
        assert.match(markup, /<textarea/);
        assert.equal(field.presentation.resourceUrl, undefined, "Mod IDs keep a normal input label, not a credential acquisition link");
      }
    }
    assert.equal(fields.get("allow_local_only_mods").defaultValue, true);
    assert.equal(fields.get("mod_install_timeout_seconds").defaultValue, 900);
    assert.equal(fields.get("mods_directory").defaultValue, "AbioticFactor/Saved/mod.io");
    assert.equal(fields.get("sideload_mods_path").defaultValue, "AbioticFactor/Mods");
  }
});

test("Abiotic catalog browsing is a bilingual section reference with external-navigation protections", () => {
  const registrations = listConfigurationSpecializedRenderers(definition, "official-mods");
  const source = registrations.find((registration) => registration.id === "abiotic-official-mod-source");
  assert.ok(source, "the catalog reference must be mounted by the real configuration workspace");
  assert.equal(source.placement, "before-fields");
  for (const [locale, catalog] of [["en-US", EN_US_ABIOTIC_FACTOR_MESSAGES], ["zh-CN", ZH_CN_ABIOTIC_FACTOR_MESSAGES]]) {
    const { t } = localized(locale, catalog);
    const markup = renderToStaticMarkup(React.createElement(I18nContext.Provider,
      { value: { locale, setLocale() {}, t } }, React.createElement(source.Renderer)));
    assert.ok(markup.includes(catalog["abiotic.settings.mods.browseCatalog"]));
    assert.match(markup, /href="https:\/\/mod\.io\/g\/abiotic-factor"/);
    assert.match(markup, /target="_blank"/);
    assert.match(markup, /rel="noopener noreferrer"/);
    assert.match(markup, /aria-description="Open the official page in your browser"/);
    assert.doesNotMatch(markup, /<input|<textarea/, "browsing is independent of any setting value or credential");
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
  assert.ok(validate("modio_token", "fixture\n-token"));
  for (const directory of ["D:/shared", "../other", "AbioticFactor/Saved/../Mods", "AbioticFactor/Saved/NUL", "AbioticFactor/Saved/cache."]) {
    assert.ok(validate("mods_directory", directory));
  }
  assert.ok(validate("mods_directory", "AbioticFactor/Saved/custom", { mod_ids: "6472275" }));
  assert.equal(validate("mods_directory", "AbioticFactor/Saved/custom", { mod_ids: "6472275", modio_token: "fixture-token" }), undefined);
  assert.equal(validate("sideload_mods_path", "AbioticFactor/Mods/curated"), undefined);
  assert.ok(validate("sideload_mods_path", "AbioticFactor/Saved/cache"));
});

test("Abiotic retains its separate unofficial UE4SS importer and isolated native Mod folders", () => {
  const manifest = fs.readFileSync(path.resolve(__dirname, "../../../modules/abioticfactor/module.toml"), "utf8");
  assert.match(manifest, /provider = "nexus"/);
  assert.match(manifest, /target_label = "AbioticFactor\/Binaries\/Win64\/ue4ss\/Mods"/);
  assert.match(manifest, /"AbioticFactor\/Mods"/);
  assert.match(manifest, /Official mod\.io Mods use the Official Mods section in Configuration/);
});
