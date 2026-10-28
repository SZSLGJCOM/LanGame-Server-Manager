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

const root = path.resolve(__dirname, "../../..");
const {
  parseGuidedSettingsSchema,
  readGuidedFieldValue,
  writeGuidedFieldValue,
  validateGuidedSettingsObject
} = require("../src/views/settings/guided-settings.ts");
const { isOptionalBooleanOverride } = require("../src/views/settings/guided-setting-values.ts");
const { ConfigurationField } = require("../src/views/settings/ConfigurationField.tsx");
const { rustSettingsDefinition } = require("../src/views/settings/modules/rust.ts");
const { returntomoriaSettingsDefinition } = require("../src/views/settings/modules/returntomoria.ts");

function parsed(game, locale = "en-US") {
  const suffix = locale === "zh-CN" ? "zh-cn" : "en";
  const exports = require(`../src/i18n/games/${game}.${suffix}.ts`);
  const catalog = Object.values(exports)[0];
  const t = (key, _params, fallback) => catalog[key] ?? fallback ?? key;
  const schema = parseGuidedSettingsSchema({
    summary: { id: game, name: game },
    schema_json: fs.readFileSync(path.join(root, `modules/${game}/schema.json`), "utf8")
  }, locale, t);
  assert.equal(schema.parseError, null);
  return { schema, t };
}

function issuesFor(schema, key, value) {
  return validateGuidedSettingsObject(schema, { [key]: value })
    .filter((issue) => issue.fieldKey === key);
}

test("Rust helipad override offers native, enabled and disabled without defaulting to false", () => {
  const { schema } = parsed("rust");
  const field = schema.fields.find((entry) => entry.key === "vehicle_pad_repairs_required");
  assert.ok(field);
  assert.equal(isOptionalBooleanOverride(field), true);
  assert.equal(field.defaultValue, undefined);
  assert.equal(readGuidedFieldValue(field, {}), undefined);
  const html = renderToStaticMarkup(React.createElement(ConfigurationField, {
    field, value: undefined, settings: {}, onPatch() {},
    copy: {
      concealSecret: "Hide", revealSecret: "Show", useGameDefault: "Use game setting",
      restartScopes: { none: "None", server: "Restart server", world: "Restart world", cluster: "Restart cluster" }
    }
  }));
  assert.match(html, /<select/);
  assert.match(html, /value="" selected=""/);
  assert.match(html, /value="true"/);
  assert.match(html, /value="false"/);
  const disabled = writeGuidedFieldValue({}, field, false);
  assert.equal(disabled.vehicle_pad_repairs_required, false);
  assert.equal(readGuidedFieldValue(field, disabled), false);
  const enabled = writeGuidedFieldValue(disabled, field, true);
  assert.equal(readGuidedFieldValue(field, enabled), true);
  const unset = writeGuidedFieldValue(enabled, field, undefined);
  assert.deepEqual(unset, {});
  assert.equal(readGuidedFieldValue(field, unset), undefined);
  const ordinaryFalse = schema.fields.find((entry) => entry.key === "pve");
  const ordinaryTrue = schema.fields.find((entry) => entry.key === "upkeep_group_scaling");
  assert.equal(isOptionalBooleanOverride(ordinaryFalse), false);
  assert.equal(readGuidedFieldValue(ordinaryFalse, {}), false);
  assert.equal(readGuidedFieldValue(ordinaryTrue, {}), true);
});

test("Rust population controls keep zero and fractions and can remove an override", () => {
  const { schema, t } = parsed("rust");
  const low = schema.fields.find((entry) => entry.key === "powergrid_fuse_decay_low_pop");
  const scale = schema.fields.find((entry) => entry.key === "powergrid_fuse_decay_low_pop_scale");
  assert.equal(low.defaultValue, undefined);
  assert.equal(scale.defaultValue, undefined);
  assert.equal(writeGuidedFieldValue({}, low, "0").powergrid_fuse_decay_low_pop, 0);
  assert.equal(writeGuidedFieldValue({}, scale, "0.125").powergrid_fuse_decay_low_pop_scale, 0.125);
  assert.deepEqual(writeGuidedFieldValue({ powergrid_fuse_decay_low_pop: 10 }, low, ""), {});
  assert.equal(issuesFor(schema, low.key, 2147483648).length, 1);
  assert.equal(issuesFor(schema, low.key, 1.5).length, 1);
  assert.equal(issuesFor(schema, scale.key, 1e39).length, 1);
  const groups = rustSettingsDefinition.buildFieldGroups("gamemode", schema.fields.filter((field) => field.sectionId === "gamemode"), "en-US", t);
  assert.deepEqual(groups.find((group) => group.id === "heavy-fuse-decay").fields.map((field) => field.key),
    ["powergrid_fuse_decay_low_pop", "powergrid_fuse_decay_high_pop", "powergrid_fuse_decay_low_pop_scale"]);
});

test("Moria worker threads use a bounded performance control and the native launch argument", () => {
  const { schema, t } = parsed("returntomoria");
  const field = schema.fields.find((entry) => entry.key === "server_worker_threads");
  assert.equal(field.type, "integer");
  assert.equal(field.defaultValue, undefined);
  assert.match(field.description, /native default 4/);
  assert.equal(field.minimum, 1);
  assert.equal(field.maximum, 4);
  for (const value of [1, 2, 3, 4]) assert.deepEqual(issuesFor(schema, field.key, value), []);
  for (const value of [0, 5, 2.5]) assert.ok(issuesFor(schema, field.key, value).length);
  const groups = returntomoriaSettingsDefinition.buildFieldGroups("advanced", schema.fields.filter((entry) => entry.sectionId === "advanced"), "en-US", t);
  assert.ok(groups.find((group) => group.id === "performance").fields.some((entry) => entry.key === field.key));
  const manifest = fs.readFileSync(path.join(root, "modules/returntomoria/module.toml"), "utf8");
  assert.match(manifest, /\{\{returntomoria\.worker_thread_args\}\}/);
});

test("RimWorld image URLs share HTTP(S) validation and distinguish clear from unset", () => {
  const { schema } = parsed("rimworld");
  const field = schema.fields.find((entry) => entry.key === "server_image_url");
  assert.equal(field.defaultValue, undefined);
  assert.equal(field.maxLength, 2048);
  for (const value of ["", "http://images.example.test/server.png", "https://images.example.test/殖民地.png?size=400"]) {
    assert.deepEqual(issuesFor(schema, field.key, value), []);
  }
  for (const value of ["file:///image.png", "javascript:alert(1)", "https://", "https://images.example.test/a b.png", "https://images.example.test/\nimage.png", "https://images.example.test/\0image.png", "https://images.example.test/\x9fimage.png"]) {
    assert.ok(issuesFor(schema, field.key, value).length, JSON.stringify(value));
  }
  assert.ok(issuesFor(schema, field.key, `https://images.example.test/${"x".repeat(2048)}`).length);
  assert.deepEqual(writeGuidedFieldValue({}, field, ""), { server_image_url: "" });
  const manifest = fs.readFileSync(path.join(root, "modules/rimworld/module.toml"), "utf8");
  assert.match(manifest, /26\.10\.20\.1/);
  assert.match(manifest, /1\.7 unstable/);
});

test("new controls have explicit English and Chinese labels and descriptions", () => {
  for (const [game, keys] of Object.entries({
    rust: ["vehicle_pad_repairs_required", "powergrid_fuse_decay_low_pop", "powergrid_fuse_decay_high_pop", "powergrid_fuse_decay_low_pop_scale"],
    returntomoria: ["server_worker_threads"], rimworld: ["server_image_url"]
  })) {
    for (const locale of ["en-US", "zh-CN"]) {
      const { schema } = parsed(game, locale);
      for (const key of keys) {
        const field = schema.fields.find((entry) => entry.key === key);
        assert.ok(field.title.length > 2);
        assert.ok(field.description.length > 10);
        if (locale === "zh-CN") assert.match(field.title, /[\u4e00-\u9fff]/);
      }
    }
  }
});
