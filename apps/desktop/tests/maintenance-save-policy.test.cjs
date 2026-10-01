const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const Module = require("node:module");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("", module.filename);

const root = path.resolve(__dirname, "../../..");
const settingsRoot = path.join(root, "apps/desktop/src/views/settings");
const {
  parseGuidedSettingsSchema, validateGuidedSettingsObject, writeGuidedFieldValue
} = require(path.join(settingsRoot, "guided-settings.ts"));
const {
  listSettingsModuleIds, resolveSettingsModuleDefinition
} = require(path.join(settingsRoot, "module-registry.ts"));
const { resolveConfigurationFieldPresentation } = require(path.join(settingsRoot, "configuration-presentation.ts"));
const { nativeSavePolicyKeys } = require(path.join(settingsRoot, "save-policy.ts"));
const { NativeSavePolicyFields } = require("../src/views/servers/NativeSavePolicyFields.tsx");
const { EN_US_EXTRA_MESSAGES } = require("../src/i18n-messages-en-extra.ts");
const copy = (key, _params, fallback) => EN_US_EXTRA_MESSAGES[key] ?? fallback ?? key;

// Independent inventory of the current native contracts, plus Unturned's
// manager-driven Save command. Similar names (world selection and log rotation)
// must not be moved into Maintenance by a broad name filter.
const expectedFields = {
  arksurvivalascended: ["auto_save_period_minutes"],
  arksurvivalevolved: ["auto_save_period_minutes", "max_num_of_save_backups"],
  astroneer: ["auto_save_interval_seconds", "backup_save_interval_seconds"],
  dontstarve: ["autosaver_enabled", "max_snapshots"],
  humanitz: ["save_interval_seconds"],
  palworld: ["auto_save_span", "use_backup_save_data"],
  projectzomboid: ["save_world_every_minutes", "backups_count", "backups_on_start", "backups_on_version_change", "backups_period"],
  rust: ["save_interval_seconds"],
  satisfactory: ["rotating_autosaves"],
  sonsoftheforest: ["save_interval"],
  soulmask: ["save_interval_seconds", "backup_interval_seconds"],
  terraria: ["worldrollbackstokeep"],
  theforest: ["autosave_interval_minutes"],
  unturned: ["managed_save_interval_seconds"],
  valheim: ["save_interval_seconds", "backup_count", "backup_short_seconds", "backup_long_seconds"],
  vrising: ["autosave_count", "autosave_interval_seconds", "autosave_smart_keep"]
};
const expectedOtherMaintenanceFields = { returntomoria: ["upgrade_optional_dlc_array"] };
const translate = (_key, _params, fallback) => fallback ?? "";
const modules = listSettingsModuleIds().map((id) => ({
  id,
  definition: resolveSettingsModuleDefinition(id),
  schema: JSON.parse(fs.readFileSync(path.join(root, "modules", id, "schema.json"), "utf8"))
}));

function parse(module, surface, locale = "en-US") {
  const parsed = parseGuidedSettingsSchema({
    summary: { id: module.id, name: module.id }, schema_json: JSON.stringify(module.schema)
  }, locale, translate, { surface });
  assert.equal(parsed.parseError, null, `${module.id} ${surface} ${locale}`);
  return parsed;
}

test("all 32 games assign save policies and explicit world maintenance to their sole Maintenance owner", () => {
  assert.equal(modules.length, 32);
  assert.equal(Object.values(expectedFields).flat().length, 30);
  for (const module of modules) {
    const expected = [...(expectedFields[module.id] ?? [])].sort();
    assert.deepEqual([...nativeSavePolicyKeys(module.id)].sort(), expected, module.id);
    const owned = Object.entries(module.schema.properties).flatMap(([key, property]) => {
      const presentation = resolveConfigurationFieldPresentation(key, property, module.definition);
      if (presentation.owner !== "maintenance") return [];
      assert.equal(presentation.state, "editable", `${module.id}.${key}`);
      assert.equal(presentation.rendererId, undefined, `${module.id}.${key} uses its scalar editor`);
      return [key];
    }).sort();
    const expectedMaintenance = [...expected, ...(expectedOtherMaintenanceFields[module.id] ?? [])].sort();
    assert.deepEqual(owned, expectedMaintenance, `${module.id} ownership must match actual schema fields`);
    for (const locale of ["en-US", "zh-CN"]) {
      const maintenance = parse(module, "maintenance", locale);
      assert.deepEqual(maintenance.fields.map((field) => field.key).sort(), expectedMaintenance, `${module.id} ${locale}`);
      for (const surface of ["general", "mods", "player_access"]) {
        const other = parse(module, surface, locale);
        assert.deepEqual(other.fields.filter((field) => expectedMaintenance.includes(field.key)), [], `${module.id} duplicates save policy in ${surface}`);
      }
    }
  }
});

test("the four settings surfaces partition every editable scalar field without hiding native controls", () => {
  const surfacesByOwner = { configuration: "general", mods: "mods", player_access: "player_access", maintenance: "maintenance" };
  for (const module of modules) {
    const fieldsBySurface = Object.fromEntries(Object.values(surfacesByOwner).map((surface) => [
      surface, new Set(parse(module, surface).fields.map((field) => field.key))
    ]));
    for (const [key, property] of Object.entries(module.schema.properties)) {
      const presentation = resolveConfigurationFieldPresentation(key, property, module.definition);
      const types = Array.isArray(property.type) ? property.type : [property.type];
      if (!types.some((type) => ["string", "number", "integer", "boolean"].includes(type))) continue;
      if (!["editable", "specialized"].includes(presentation.state)) continue;
      const observed = Object.entries(fieldsBySurface).filter(([, fields]) => fields.has(key)).map(([surface]) => surface);
      assert.deepEqual(observed, [surfacesByOwner[presentation.owner]], `${module.id}.${key} must have one editable owner`);
    }
  }
});

test("Maintenance retains native numeric schema boundaries and rejects invalid numeric drafts", () => {
  let numericFields = 0;
  for (const module of modules) {
    const schema = parse(module, "maintenance");
    assert.deepEqual(validateGuidedSettingsObject(schema, {}, undefined, translate), [], `${module.id} defaults`);
    for (const field of schema.fields) {
      if (!["integer", "number"].includes(field.type)) continue;
      numericFields++;
      const property = module.schema.properties[field.key];
      const issues = (value) => validateGuidedSettingsObject(schema, { [field.key]: value }, undefined, translate)
        .filter((issue) => issue.fieldKey === field.key);
      assert.equal(field.minimum, property.minimum, `${module.id}.${field.key} minimum`);
      assert.equal(field.maximum, property.maximum, `${module.id}.${field.key} maximum`);
      for (const value of [NaN, Infinity, -Infinity, "unfinished", "1."]) {
        assert.ok(issues(value).some((issue) => issue.reason === "number"), `${module.id}.${field.key}: ${value}`);
      }
      if (field.minimum !== undefined) {
        assert.deepEqual(issues(field.minimum), [], `${module.id}.${field.key} accepts its minimum`);
        assert.ok(issues(field.minimum - 1).some((issue) => issue.reason === "minimum"), `${module.id}.${field.key} rejects below minimum`);
      }
      if (field.maximum !== undefined) {
        assert.deepEqual(issues(field.maximum), [], `${module.id}.${field.key} accepts its maximum`);
        assert.ok(issues(field.maximum + 1).some((issue) => issue.reason === "maximum"), `${module.id}.${field.key} rejects above maximum`);
      }
      if (field.type === "integer") {
        assert.ok(issues((field.minimum ?? 0) + 0.5).some((issue) => issue.reason === "number"), `${module.id}.${field.key} rejects fractions`);
        assert.ok(issues(Number.MAX_SAFE_INTEGER + 1).some((issue) => issue.reason === "number"), `${module.id}.${field.key} rejects unsafe integers`);
      } else {
        assert.deepEqual(issues((field.minimum ?? 0) + 0.5), [], `${module.id}.${field.key} preserves fractional intervals`);
      }
    }
  }
  assert.equal(numericFields, 25);
});

test("native boolean and retention-expression edits retain their values and unrelated settings", () => {
  for (const module of modules) {
    const schema = parse(module, "maintenance");
    for (const field of schema.fields) {
      if (!nativeSavePolicyKeys(module.id).includes(field.key) || !["boolean", "string"].includes(field.type)) continue;
      const value = field.type === "boolean" ? !field.defaultValue : "10:2:1,60:1:1,1440:5:0";
      const original = { server_name: "Retain instance identity", unmodeled_operator_note: "keep", [field.key]: field.defaultValue };
      const saved = writeGuidedFieldValue(original, field, value);
      assert.deepEqual(saved, { ...original, [field.key]: value }, `${module.id}.${field.key}`);
      assert.equal(original[field.key], field.defaultValue, "the original snapshot stays unchanged");
      assert.deepEqual(validateGuidedSettingsObject(schema, saved, undefined, translate), [], `${module.id}.${field.key}`);
    }
  }
});

test("games without native save policies expose only independently owned maintenance fields", () => {
  const withoutNativePolicy = modules.filter((module) => !expectedFields[module.id]);
  assert.equal(withoutNativePolicy.length, 16);
  for (const module of withoutNativePolicy) {
    const maintenance = parse(module, "maintenance");
    assert.deepEqual(maintenance.fields.map((field) => field.key), expectedOtherMaintenanceFields[module.id] ?? [], module.id);
    assert.deepEqual(validateGuidedSettingsObject(maintenance, {}, undefined, translate), [], module.id);
    assert.ok(parse(module, "general").fields.length > 0, `${module.id} keeps native configuration access`);
  }
});

test("every game renders an Autosave section with explicit frequency or its native timing", () => {
  for (const module of modules) {
    const fields = parse(module, "maintenance").fields;
    const markup = renderToStaticMarkup(React.createElement(NativeSavePolicyFields, {
      instanceId: `instance-${module.id}`, moduleId: module.id, fields, settings: {},
      issues: [], disabled: false, t: copy, onChange() {}
    }));
    assert.match(markup, /<legend>Autosave<\/legend>/, module.id);
    if (!fields.some((field) => expectedFields[module.id]?.includes(field.key) &&
      !["rotating_autosaves", "worldrollbackstokeep"].includes(field.key))) {
      assert.match(markup, /Save frequency/, module.id);
    }
    if (module.id === "barotrauma") {
      assert.match(markup, /On campaign stage and round transitions/);
      assert.match(markup, /Managed by the game/);
      assert.doesNotMatch(markup, /type="checkbox"/, "there is no invented campaign autosave switch");
    }
    if (module.id === "dontstarve") assert.match(markup, /At the end of each game day/);
  }
});

test("Unturned exposes an autosave switch and remembers the interval within the instance edit", () => {
  const filename = path.resolve(__dirname, "../src/views/servers/NativeSavePolicyFields.tsx");
  const loaded = new Module(filename, module);
  loaded.filename = filename;
  let ref;
  const requireFromFile = Module.createRequire(filename);
  loaded.require = (id) => id === "react" ? { ...React,
    useRef(initial) { return ref ??= { current: initial }; }
  } : requireFromFile(id);
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  const props = {
    instanceId: "unturned-a", moduleId: "unturned",
    fields: parse(modules.find((module) => module.id === "unturned"), "maintenance").fields,
    settings: { managed_save_interval_seconds: 600 }, issues: [], disabled: false, t: copy,
    onChange(field, value) { props.settings = writeGuidedFieldValue(props.settings, field, value); }
  };
  function descendants(node) {
    return React.isValidElement(node) ? [node, ...React.Children.toArray(node.props.children).flatMap(descendants)] : [];
  }
  function render() {
    const nodes = descendants(loaded.exports.NativeSavePolicyFields(props));
    return {
      toggle: nodes.find((node) => node.type === "input" && node.props.type === "checkbox"),
      interval: nodes.find((node) => typeof node.type === "function" && node.type.name === "ConfigurationField")
    };
  }
  let view = render();
  assert.equal(view.toggle.props.checked, true);
  assert.equal(view.interval.props.value, 600);
  view.toggle.props.onChange({ target: { checked: false } });
  view = render();
  assert.equal(props.settings.managed_save_interval_seconds, 0);
  assert.equal(view.interval.props.disabled, true);
  assert.equal(view.interval.props.value, 600, "the chosen frequency remains visible while disabled");
  view.toggle.props.onChange({ target: { checked: true } });
  assert.equal(props.settings.managed_save_interval_seconds, 600);
  props.instanceId = "unturned-b";
  props.settings = { managed_save_interval_seconds: 0 };
  view = render();
  view.toggle.props.onChange({ target: { checked: true } });
  assert.equal(props.settings.managed_save_interval_seconds, 300, "another instance never inherits the previous interval");
  props.disabled = true;
  assert.equal(render().toggle.props.disabled, true);
});

test("archived Unturned autosave keeps null and missing intervals without an enabled toggle", () => {
  const fields = parse(modules.find((module) => module.id === "unturned"), "maintenance").fields;
  for (const [settings, expected] of [[{ managed_save_interval_seconds: null }, "null"], [{}, "Not saved"]]) {
    const markup = renderToStaticMarkup(React.createElement(NativeSavePolicyFields, {
      instanceId: "unturned-archive", moduleId: "unturned", fields, settings,
      issues: [], disabled: true, readOnly: true, t: copy,
      onChange() { assert.fail("archived intervals cannot mutate"); }
    }));
    assert.doesNotMatch(markup, /type="checkbox"/);
    assert.ok(markup.includes(expected), `the saved interval remains visible as ${expected}`);
  }
  for (const [value, enabled] of [[0, false], [600, true]]) {
    const markup = renderToStaticMarkup(React.createElement(NativeSavePolicyFields, {
      instanceId: "unturned-archive", moduleId: "unturned", fields,
      settings: { managed_save_interval_seconds: value }, issues: [], disabled: true, readOnly: true,
      t: copy, onChange() { assert.fail("archived intervals cannot mutate"); }
    }));
    const toggle = markup.match(/<input[^>]*type="checkbox"[^>]*>/)?.[0];
    assert.ok(toggle);
    assert.equal(toggle.includes('checked=""'), enabled);
  }
});
