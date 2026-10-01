const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const { buildSavedConfigurationSchema } = require("../src/views/settings/saved-configuration-schema.ts");
const { buildConfigurationWorkspaceModel, searchConfigurationItems } = require("../src/views/settings/configuration-workspace-model.ts");
const t = (key, _params, fallback) => fallback ?? key;

function field(key, options = {}) {
  const sectionId = options.sectionId ?? "room";
  return { key, title: options.title ?? key, type: "string", control: "text", required: false,
    sectionId, ...options,
    presentation: { sectionId, owner: options.owner ?? "configuration", state: options.state ?? "editable",
      ...options.presentation } };
}
function schema(fields, options = {}) {
  return { title: "Saved configuration", sections: [{ id: "room", title: "Room" }], fields,
    parseError: null, ...options };
}

test("the shared saved schema keeps actual keys and field controls without initializing defaults", () => {
  const original = field("saved_name", { control: "select", enumOptions: [{ value: "stored", label: "Stored label" }],
    defaultValue: "Current default", defaultSource: "instance_name", icon: "server", sourceKey: "NativeName",
    presentation: { aliases: ["Server title"], restartScope: "server" } });
  const source = schema([original, field("unsaved_players", { defaultValue: 64 }),
    field("unsaved_identity", { defaultSource: "instance_id" })]);
  const settings = { saved_name: "stored" };
  const before = structuredClone(source);
  const saved = buildSavedConfigurationSchema(source, settings, t);
  assert.deepEqual(saved.fields.map((item) => item.key), ["saved_name"]);
  assert.equal(saved.fields[0].control, "select");
  assert.deepEqual(saved.fields[0].enumOptions, original.enumOptions);
  assert.equal(saved.fields[0].sourceKey, "NativeName");
  assert.equal(saved.fields[0].icon, "server");
  assert.deepEqual(saved.fields[0].presentation.aliases, ["Server title"]);
  assert.equal(saved.fields[0].presentation.restartScope, "server");
  assert.equal(Object.hasOwn(saved.fields[0], "defaultValue"), false);
  assert.equal(Object.hasOwn(saved.fields[0], "defaultSource"), false);
  assert.deepEqual(source, before);
  assert.deepEqual(settings, { saved_name: "stored" });
});

test("false, zero, empty text, null, arrays and objects remain exact and searchable", () => {
  const settings = JSON.parse('{"saved_false":false,"saved_zero":0,"saved_empty":"","saved_null":null,"saved_array":[0,false],"unknown_blob":{"nested":[1,"two"]}}');
  const saved = buildSavedConfigurationSchema(schema([field("saved_false", { type: "boolean", control: "checkbox" })]), settings, t);
  const model = buildConfigurationWorkspaceModel(saved);
  assert.deepEqual(model.items.map((item) => item.fieldKey).sort(), Object.keys(settings).sort());
  assert.deepEqual(saved.fields.map((item) => [item.key, item.type, item.control]), [
    ["saved_false", "boolean", "checkbox"], ["saved_zero", "number", "number"],
    ["saved_empty", "string", "text"], ["saved_null", "string", "textarea"],
    ["saved_array", "string", "textarea"], ["unknown_blob", "string", "textarea"]
  ]);
  for (const key of Object.keys(settings)) {
    const item = model.items.find((candidate) => candidate.fieldKey === key);
    assert.equal(item.owner, "configuration", key);
    assert.ok(model.actionableSectionIds.includes(item.sectionId), key);
    assert.deepEqual(searchConfigurationItems(model, key, "en-US").map((match) => match.fieldKey), [key]);
  }
  assert.deepEqual(settings, { saved_false: false, saved_zero: 0, saved_empty: "", saved_null: null,
    saved_array: [0, false], unknown_blob: { nested: [1, "two"] } });
});

test("saved fields retain schema ownership boundaries and shared connection navigation", () => {
  const source = schema([], { presentationFields: [
    field("saved_room"), field("unsaved_room", { defaultValue: "current default" }),
    field("listen_address", { sectionId: "network", owner: "instance_network" }),
    field("workshop_ids", { sectionId: "mods", owner: "mods", state: "specialized" }),
    field("whitelist_entries", { sectionId: "players", owner: "player_access", state: "specialized" }),
    field("custom_launch", { sectionId: "maintenance", owner: "maintenance", state: "specialized" })
  ], sections: [{ id: "room", title: "Room" }, { id: "mods", title: "Mods" },
    { id: "players", title: "Players" }, { id: "maintenance", title: "Maintenance" }] });
  const settings = { saved_room: "Archived room", listen_address: "192.168.1.42", workshop_ids: "22334455",
    whitelist_entries: "archive-player", custom_launch: "--archive", unknown_saved: { value: false } };
  const saved = buildSavedConfigurationSchema(source, settings, t);
  assert.deepEqual(saved.fields.map((item) => item.key), ["saved_room", "listen_address", "unknown_saved"]);
  assert.ok(saved.fields.every((item) => item.presentation.owner === "configuration" && item.presentation.state === "editable"));
  assert.equal(saved.sections.filter((section) => section.id === "network").length, 1);
  const model = buildConfigurationWorkspaceModel(saved);
  assert.equal(model.items.find((item) => item.fieldKey === "listen_address").sectionId, "network");
});

test("unknown keys with colliding normalized names remain distinct and reuse an existing saved section", () => {
  const keys = ["a/b", "a:b", "a b", "中文", "💾", "__proto__"];
  const settings = JSON.parse(JSON.stringify(Object.fromEntries(keys.map((key, index) => [key, index]))));
  const source = schema([], { sections: [{ id: "room", title: "Room" },
    { id: "saved_settings", title: "Existing saved settings", order: 99 }] });
  const saved = buildSavedConfigurationSchema(source, settings, t);
  assert.equal(saved.sections.filter((section) => section.id === "saved_settings").length, 1);
  assert.equal(saved.sections.find((section) => section.id === "saved_settings").title, "Existing saved settings");
  assert.deepEqual(saved.fields.map((item) => item.key), keys);
  const model = buildConfigurationWorkspaceModel(saved);
  assert.equal(new Set(model.items.map((item) => item.fieldKey)).size, keys.length);
  assert.ok(model.items.every((item) => item.sectionId === "saved_settings"));
});

test("only current runtime policy keys belong to Maintenance without guessing ownership from prefixes", () => {
  const retired = ["restart_policy", "auto_restart_enabled", "auto_restart_backoff_ms", "auto_restart_only_nonzero_exit", "crash_restart_limit"];
  const keys = ["runtime_performance", "runtime_restart", ...retired];
  const settings = Object.fromEntries(keys.map((key) => [key, false]));
  Object.assign(settings, { runtime_custom_note: "runtime note", mod_custom_note: "mod note", player_custom_note: "player note" });
  const saved = buildSavedConfigurationSchema(schema([]), settings, t);
  assert.deepEqual(saved.fields.map((item) => item.key), [...retired, "runtime_custom_note", "mod_custom_note", "player_custom_note"]);
});

test("shared saved schema preserves parse failures and shared topology validation", () => {
  const parseFailure = buildSavedConfigurationSchema(schema([], { parseError: "Malformed module schema" }), { saved_value: "x" }, t);
  assert.equal(parseFailure.parseError, "Malformed module schema");
  const cases = [
    [schema([], { sections: [{ id: "duplicate", title: "One" }, { id: "duplicate", title: "Two" }] }), /duplicate configuration section/],
    [schema([field("saved_value", { sectionId: "missing" })]), /references unknown section missing/],
    [schema([], { sections: [{ id: "a", title: "A", parentId: "b" }, { id: "b", title: "B", parentId: "a" }] }), /configuration section cycle/]
  ];
  for (const [source, expected] of cases) {
    const saved = buildSavedConfigurationSchema(source, { saved_value: "archived" }, t);
    assert.throws(() => buildConfigurationWorkspaceModel(saved), expected);
  }
});
