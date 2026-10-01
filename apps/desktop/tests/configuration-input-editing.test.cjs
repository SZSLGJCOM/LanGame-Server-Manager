const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
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
require.extensions[".png"] = (module, filename) => module._compile(`module.exports = ${JSON.stringify(filename)};`, filename);
const { parseGuidedSettingsSchema, readGuidedFieldValue, writeGuidedFieldValue,
  isOptionalBooleanOverride, validateGuidedSettingsObject } = require("../src/views/settings/guided-settings.ts");
const { ConfigurationField } = require("../src/views/settings/ConfigurationField.tsx");
const { resolveSettingsModuleDefinition } = require("../src/views/settings/module-registry.ts");

function numberField(type = "number") {
  return { key: "rate", title: "Rate", type, control: "number", required: false,
    defaultValue: 1, presentation: { state: "editable", owner: "configuration" } };
}

function issues(field, settings) {
  return validateGuidedSettingsObject({ fields: [field], sections: [] }, settings);
}

test("Core Keeper preserves existing 32-character passwords through input and save validation", () => {
  const schema = parseGuidedSettingsSchema({
    summary: { id: "corekeeper", name: "Core Keeper" },
    schema_json: fs.readFileSync(path.resolve(__dirname, "../../../modules/corekeeper/schema.json"), "utf8")
  }, "en-US");
  const field = schema.fields.find((candidate) => candidate.key === "join_password");
  assert.ok(field);
  assert.equal(field.maxLength, undefined);
  const existingPassword = "a".repeat(32);
  const html = renderToStaticMarkup(React.createElement(ConfigurationField, {
    field, value: existingPassword, settings: { direct_connection_enabled: true }, onPatch() {},
    copy: { concealSecret: "Hide", revealSecret: "Show", restartScopes: {} }
  }));
  assert.doesNotMatch(html, /maxLength=/i);
  assert.ok(html.includes(`value="${existingPassword}"`));
  for (const length of [28, 29, 32]) {
    const settings = { join_password: "a".repeat(length) };
    assert.equal(issues(field, settings).length, 0);
    assert.equal(settings.join_password.length, length, "validation must preserve existing passwords");
  }
  const editedPassword = "b".repeat(32);
  const draft = writeGuidedFieldValue({ join_password: existingPassword }, field, editedPassword);
  assert.equal(draft.join_password, editedPassword);
  assert.deepEqual(issues(field, draft), []);
});

test("Terraria world filenames agree between browser pattern and save validation", () => {
  const schema = parseGuidedSettingsSchema({
    summary: { id: "terraria", name: "Terraria" },
    schema_json: fs.readFileSync(path.resolve(__dirname, "../../../modules/terraria/schema.json"), "utf8")
  }, "en-US");
  const field = schema.fields.find((candidate) => candidate.key === "world_file");
  assert.ok(field);
  const html = renderToStaticMarkup(React.createElement(ConfigurationField, {
    field, value: "world.wld", settings: { world_file: "world.wld" }, onPatch() {},
    copy: { concealSecret: "Hide", revealSecret: "Show", restartScopes: {} }
  }));
  const inputPattern = html.match(/<input\b[^>]*pattern="([^"]+)"/)?.[1];
  assert.equal(inputPattern, field.pattern, "the input must retain the schema's filename constraint");
  // HTML inputs compile pattern in Unicode sets mode, unlike the save validator.
  const browserPattern = new RegExp(`^(?:${inputPattern})$`, "v");
  for (const [filenames, valid] of [
    [["world.wld", "journey-grove.wld", "世界 01.wld", "world (copy) [2].wld"], true],
    [["../world.wld", "..\\world.wld", "/world.wld", "C:\\saves\\world.wld", "saves/world.wld",
      "world.txt", "worldwld", ".wld"], false]
  ]) {
    for (const filename of filenames) {
      assert.equal(browserPattern.test(filename), valid, `browser: ${filename}`);
      assert.equal(issues(field, { world_file: filename }).length === 0, valid, `save: ${filename}`);
    }
  }
});

test("numeric controls preserve raw DOM text before validation, including suggestion inputs", () => {
  const filename = path.resolve(__dirname, "../src/views/settings/ConfigurationField.tsx");
  const loaded = new Module(filename, module);
  const originalRequire = Module.createRequire(filename);
  loaded.filename = filename;
  loaded.require = (id) => id === "react" ? {
    ...React, useState: (initial) => [initial, () => {}], useRef: (initial) => ({ current: initial }), useEffect() {}
  } : id === "./ConfigurationFieldHelp" ? {
    useConfigurationFieldHelp: () => ({ anchorRef() {}, interactionProps: {}, helpNode: null })
  } : originalRequire(id);
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  function findInput(element) {
    if (!React.isValidElement(element)) return null;
    if (element.type === "input") return element;
    if (typeof element.type === "function") return findInput(element.type(element.props));
    for (const child of React.Children.toArray(element.props.children)) {
      const found = findInput(child);
      if (found) return found;
    }
    return null;
  }
  const copy = { concealSecret: "Hide", revealSecret: "Show", restartScopes: {} };
  for (const type of ["number", "integer"]) {
    for (const suggestions of [undefined, [{ label: "Normal", value: 1 }]]) {
      const field = { ...numberField(type), suggestions, minimum: 0, maximum: 10, step: 0.1 };
      let settings = { rate: 1 };
      const props = { field, copy, settings, value: 1,
        onPatch: (patch) => { settings = writeGuidedFieldValue(settings, field, patch.rate); } };
      const input = findInput(loaded.exports.ConfigurationField(props));
      assert.ok(input);
      assert.equal(input.props.type, "text", "number inputs sanitize unfinished decimals before onChange");
      assert.equal(input.props.inputMode, type === "integer" ? "numeric" : "decimal");
      assert.equal(input.props.step, undefined, "text controls must not expose native numeric stepping");
      input.props.onChange({ target: { value: "0." } });
      assert.equal(settings.rate, "0.");
      assert.equal(issues(field, settings)[0]?.reason, "number");
      const html = renderToStaticMarkup(React.createElement(ConfigurationField, {
        ...props, settings, value: readGuidedFieldValue(field, settings)
      }));
      assert.match(html, /<input[^>]*type="text"/);
      assert.match(html, /<input[^>]*value="0\."/);
      input.props.onChange({ target: { value: "2" } });
      assert.equal(settings.rate, 2);
      assert.deepEqual(issues(field, settings), []);
    }
  }
});

test("incomplete decimal and exponent edits stay visible and cannot be saved", () => {
  const field = numberField();
  for (const text of ["0.", "1e", "1e-", "-", "invalid", "Infinity"]) {
    const draft = writeGuidedFieldValue({ rate: 1 }, field, text);
    assert.equal(readGuidedFieldValue(field, draft), text);
    assert.equal(issues(field, draft)[0]?.reason, "number", text);
  }
  for (const [text, value] of [["0.5", 0.5], ["1e-2", 0.01], ["-2.5", -2.5]]) {
    const draft = writeGuidedFieldValue({ rate: "0." }, field, text);
    assert.equal(draft.rate, value);
    assert.deepEqual(issues(field, draft), []);
  }
});

test("integer controls reject fractional and unsafe values without silently rounding them", () => {
  const field = numberField("integer");
  const draft = writeGuidedFieldValue({ rate: 4 }, field, "1.5");
  assert.equal(draft.rate, 1.5);
  assert.match(issues(field, draft)[0]?.message, /whole number/);
  assert.equal(issues(field, { rate: Number.MAX_SAFE_INTEGER + 1 })[0]?.reason, "number");
  assert.deepEqual(issues(field, writeGuidedFieldValue(draft, field, "15")), []);
});

test("native numeric settings reject coercible non-number values at validation", () => {
  for (const value of [true, [], {}, "2", " "]) {
    assert.equal(issues(numberField(), { rate: value })[0]?.reason, "number");
  }
});

test("optional ASA boolean controls preserve an explicit false and can return to game default", () => {
  const schema = parseGuidedSettingsSchema({
    summary: { id: "arksurvivalascended", name: "ASA" },
    schema_json: fs.readFileSync(path.resolve(__dirname, "../../../modules/arksurvivalascended/schema.json"), "utf8")
  }, "en-US", (_key, _params, fallback) => fallback ?? "");
  const field = schema.fields.find((candidate) => candidate.key === "prevent_template_on_saddle");
  assert.ok(field);
  assert.equal(field.sectionId, "building");
  assert.equal(field.defaultValue, undefined);
  const definition = resolveSettingsModuleDefinition("arksurvivalascended");
  const groups = definition.buildFieldGroups("building", schema.fields.filter((item) => item.sectionId === "building"), "en-US", (_key, _params, fallback) => fallback);
  assert.equal(groups.find((group) => group.fields.includes(field))?.id, "structure-rules");
  const initial = { server_name: "ASA" };
  const enabled = writeGuidedFieldValue(initial, field, true);
  const disabled = writeGuidedFieldValue(enabled, field, false);
  const inherited = writeGuidedFieldValue(disabled, field, undefined);
  assert.equal(disabled.prevent_template_on_saddle, false);
  assert.deepEqual(inherited, initial);
  const copy = { concealSecret: "Hide", revealSecret: "Show", restartScopes: {} };
  for (const [settings, expected] of [[initial, ""], [enabled, "true"], [disabled, "false"]]) {
    const html = renderToStaticMarkup(React.createElement(ConfigurationField, {
      field, settings, value: readGuidedFieldValue(field, settings), copy, onPatch() {}
    }));
    assert.match(html, /<select/);
    assert.match(html, new RegExp(`<option value="${expected}" selected="">`));
  }
});

test("every bundled module preserves omitted boolean overrides and valid numeric defaults", () => {
  const modulesRoot = path.resolve(__dirname, "../../../modules");
  const modules = fs.readdirSync(modulesRoot).filter((id) => fs.existsSync(path.join(modulesRoot, id, "schema.json")));
  assert.equal(modules.length, 32);
  const copy = { concealSecret: "Hide", revealSecret: "Show", restartScopes: {} };
  for (const id of modules) {
    const schema = parseGuidedSettingsSchema({
      summary: { id, name: id }, schema_json: fs.readFileSync(path.join(modulesRoot, id, "schema.json"), "utf8")
    }, "en-US", (_key, _params, fallback) => fallback ?? "");
    assert.deepEqual(validateGuidedSettingsObject(schema, {}).filter((issue) => issue.reason === "number"), [], id);
    for (const field of schema.fields.filter((field) => field.control === "checkbox" && isOptionalBooleanOverride(field))) {
      const state = { unrelated: "preserve", [field.key]: false };
      assert.deepEqual(writeGuidedFieldValue(state, field, undefined), { unrelated: "preserve" }, `${id}:${field.key}`);
      const html = renderToStaticMarkup(React.createElement(ConfigurationField, {
        field, settings: {}, value: readGuidedFieldValue(field, {}), copy, onPatch() {}
      }));
      assert.ok(html.includes(`<option value="" selected="">${field.preserveNativeWhenUnset
        ? "Keep native setting (current value not read)" : "Use game default"}</option>`), `${id}:${field.key}`);
    }
  }
});

test("an incomplete numeric draft survives locale changes and blocks autosave until completed", () => {
  const filename = path.resolve(__dirname, "../src/views/settings/ConfigurationWorkspace.tsx");
  const loaded = new Module(filename, module);
  const originalRequire = Module.createRequire(filename);
  const hooks = [];
  let cursor = 0;
  let locale = "en-US";
  let save;
  const t = (_key, _params, fallback) => fallback ?? "";
  loaded.filename = filename;
  loaded.require = (id) => id === "react" ? {
    ...React, useMemo: (compute) => compute(), useLayoutEffect() {},
    useRef(initial) {
      const slot = cursor++;
      if (!(slot in hooks)) hooks[slot] = { current: initial };
      return hooks[slot];
    },
    useState(initial) {
      const slot = cursor++;
      if (!(slot in hooks)) hooks[slot] = typeof initial === "function" ? initial() : initial;
      return [hooks[slot], (next) => { hooks[slot] = typeof next === "function" ? next(hooks[slot]) : next; }];
    }
  } : id === "../../i18n" ? { ...originalRequire(id), useI18n: () => ({ locale, t }) }
    : id === "./useAutoSaveInstanceSettings" ? {
      useAutoSaveInstanceSettings(options) { save = options; return { status: { state: "saved" }, retry() {} }; }
    } : id === "./useModuleConfigurationIcons" ? {
      useModuleConfigurationIcons: () => ({ icons: {}, missing: false, loading: false, retryAvailable: false })
    } : id === "./useInstancePortRegistration" ? {
      useInstancePortRegistration: () => ({ ports: [], defaultPorts: [], setPorts() {} })
    } : originalRequire(id);
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  const props = {
    details: { summary: { id: "pal-test", module_id: "palworld", name: "Pal", bind_ip: "0.0.0.0", autostart: false },
      settings_json: '{"server_name":"Pal","exp_rate":1}', ports: [], backup_retention_count: 3 },
    moduleDetails: { summary: { id: "palworld", name: "Palworld" },
      schema_json: fs.readFileSync(path.resolve(__dirname, "../../../modules/palworld/schema.json"), "utf8") },
    bindAddressCandidates: [], runtime: null, launchPlan: null, launchPlanError: null, onSave() {}
  };
  function findForm(element) {
    if (!React.isValidElement(element)) return null;
    if (element.type.name === "GuidedSettingsForm") return element;
    for (const child of React.Children.toArray(element.props.children)) {
      const found = findForm(child);
      if (found) return found;
    }
    return null;
  }
  function render() {
    cursor = 0;
    return findForm(loaded.exports.ConfigurationWorkspace(props));
  }
  const form = render();
  assert.ok(form);
  const rate = form.props.schema.fields.find((field) => field.key === "exp_rate");
  assert.ok(rate);
  form.props.onChange(rate, "0.");
  render();
  assert.equal(JSON.parse(save.settingsJson).exp_rate, "0.");
  assert.equal(save.disabled, true);
  locale = "zh-CN";
  const translated = render();
  assert.equal(JSON.parse(save.settingsJson).exp_rate, "0.");
  assert.equal(save.disabled, true);
  translated.props.onChange(rate, "0.5");
  render();
  assert.equal(JSON.parse(save.settingsJson).exp_rate, 0.5);
  assert.equal(save.disabled, false);
});

test("configuration search waits for IME composition before handling Enter or Escape", () => {
  const filename = path.resolve(__dirname, "../src/views/settings/ConfigurationSearchNavigation.tsx");
  const loaded = new Module(filename, module);
  const originalRequire = Module.createRequire(filename);
  let query = "难度";
  const selected = [];
  loaded.filename = filename;
  loaded.require = (id) => id === "react" ? {
    ...React, useMemo: (compute) => compute(), useRef: () => ({ current: null }),
    useState: () => [query, (value) => { query = value; }]
  } : id === "../../i18n" ? {
    useI18n: () => ({ locale: "zh-CN", t: (_key, _params, fallback) => fallback })
  } : id === "./configuration-workspace-model" ? {
    ...originalRequire(id),
    searchConfigurationItems: () => [{ fieldKey: "difficulty", title: "难度", owner: "configuration", state: "editable", breadcrumb: [] }]
  } : originalRequire(id);
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  const render = () => loaded.exports.ConfigurationSearchNavigation({
    model: { roots: [] }, selectedSectionId: "", onSelectSection() {}, onSelectField: (key) => selected.push(key)
  }).props.children[0].props.children[0];
  for (const key of ["Enter", "Escape"]) {
    render().props.onKeyDown({ key, nativeEvent: { isComposing: true }, preventDefault() { assert.fail("IME keys belong to the input method"); } });
    assert.equal(query, "难度");
    assert.deepEqual(selected, []);
  }
  render().props.onKeyDown({ key: "Enter", nativeEvent: { isComposing: false }, preventDefault() {} });
  assert.deepEqual(selected, ["difficulty"]);
  assert.equal(query, "");
});

test("closed native enums reject unsupported values without restricting free-form suggestions", () => {
  for (const [type, allowed, rejected] of [
    ["string", ["None", "Hard"], ["UnrecognizedPreset", "hard", 1, ""]],
    ["integer", [0, 2, 6], [1, 7, "2", 1.5]],
    ["boolean", [true], [false, "true"]]
  ]) {
    const field = { key: "preset", title: "Preset", type, control: "select", required: false,
      enumOptions: allowed.map((value) => ({ value, label: String(value) })) };
    for (const value of allowed) assert.deepEqual(issues(field, { preset: value }), []);
    for (const value of rejected) assert.equal(issues(field, { preset: value })[0]?.reason, "enum");
    assert.deepEqual(issues(field, {}), []);
    assert.deepEqual(issues(field, { preset: null }), []);
    assert.deepEqual(issues(field, { preset: undefined }), []);
    if (type !== "boolean") assert.equal(issues({ ...field, required: true }, {})[0]?.reason, "required");
  }
  const openField = { key: "map", title: "Map", type: "string", control: "text", required: false,
    suggestions: [{ value: "KnownMap", label: "Known map" }] };
  assert.deepEqual(issues(openField, { map: "CustomModMap" }), []);
  assert.deepEqual(issues({ ...openField, enumOptions: [{ value: "", label: "Default" }] }, { map: "" }), []);
});
