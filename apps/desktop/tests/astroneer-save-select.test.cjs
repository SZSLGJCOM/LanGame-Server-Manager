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
const { parseAstroneerSaveCatalog, readAstroneerSaveCatalog } = require("../src/astroneer-saves.ts");
const { parseGuidedSettingsSchema, writeGuidedFieldValue } = require("../src/views/settings/guided-settings.ts");
const { astroneerSettingsDefinition } = require("../src/views/settings/modules/astroneer.ts");
const { AstroneerSaveSelect } = require("../src/views/settings/AstroneerSaveSelect.tsx");
const { buildMockAstroneerSaveCatalog } = require("../src/api-mock/astroneer-saves.ts");
const schemaJson = fs.readFileSync(path.resolve(__dirname, "../../../modules/astroneer/schema.json"), "utf8");
const slot = { descriptive_name: "CUSTOM JOURNEY", latest_saved_at: "2026.10.09-11.22.33", versions: 2, total_bytes: 2048 };
const catalog = { instance_id: "native-instance", configured_name: "Retained old slot", entries: [slot] };

test("ASTRONEER reads the native catalog without replacing an unmatched configured name", () => {
  assert.deepEqual(parseAstroneerSaveCatalog(catalog, catalog.instance_id), catalog);
  assert.deepEqual(parseAstroneerSaveCatalog({ ...catalog, entries: [] }, catalog.instance_id), { ...catalog, entries: [] });
  for (const result of [null, [], { ...catalog, instance_id: "other-instance" }, { ...catalog, entries: [slot, slot] },
    { ...catalog, entries: [{ ...slot, versions: 0 }] }, { ...catalog, entries: [{ ...slot, total_bytes: Infinity }] },
    { ...catalog, entries: [{ ...slot, descriptive_name: "" }] }]) {
    assert.throws(() => parseAstroneerSaveCatalog(result, catalog.instance_id), /Invalid ASTRONEER/);
  }
});

test("ASTRONEER save selection belongs to Room and patches only its original setting", () => {
  const schema = parseGuidedSettingsSchema({ summary: { id: "astroneer", name: "ASTRONEER" }, schema_json: schemaJson },
    "en-US", (key, _params, fallback) => fallback ?? key);
  assert.equal(schema.parseError, null);
  const field = schema.fields.find((entry) => entry.key === "active_save_file_name");
  assert.equal(field.sectionId, "room");
  assert.equal(field.defaultValue, "SAVE_1");
  assert.equal(field.presentation.owner, "configuration");
  assert.equal(field.presentation.state, "specialized");
  const renderer = astroneerSettingsDefinition.specializedRenderers[field.presentation.rendererId];
  assert.equal(renderer.Renderer, AstroneerSaveSelect);
  assert.equal(renderer.sectionId, "room");
  assert.equal(renderer.keepMounted, true, "navigation retains the catalog and focusable selector");
  assert.equal(renderer.fieldKey, undefined, "the real select owns its focus ID");
  const original = { active_save_file_name: "Retained old slot", server_name: "Private world", future_setting: { keep: true } };
  assert.deepEqual(writeGuidedFieldValue(original, field, slot.descriptive_name), {
    ...original, active_save_file_name: slot.descriptive_name
  });
  assert.ok(schema.fields.every((entry) => Object.hasOwn(JSON.parse(schemaJson).properties, entry.key)),
    "the selector adds no synthetic persisted fields");
});

test("ASTRONEER preview does not invent an active save or accept a different game", () => {
  const details = { summary: { id: "preview", module_id: "astroneer" }, settings_json: "{}" };
  assert.equal(buildMockAstroneerSaveCatalog(details).configured_name, "SAVE_1");
  assert.equal(buildMockAstroneerSaveCatalog({ ...details, settings_json: JSON.stringify({ active_save_file_name: "Absent" }) }).configured_name, "Absent");
  assert.throws(() => buildMockAstroneerSaveCatalog({ ...details, summary: { ...details.summary, module_id: "romestead" } }), /ASTRONEER/);
});

test("ASTRONEER catalog API uses the existing native transport and propagates read failures", async () => {
  const previousWindow = globalThis.window;
  const hadIsTauri = Object.hasOwn(globalThis, "isTauri");
  const previousIsTauri = globalThis.isTauri;
  const calls = [];
  let fail = false;
  globalThis.isTauri = true;
  globalThis.window = { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command, args) => {
    calls.push({ command, args });
    if (fail) throw new Error("Native catalog read failure");
    return structuredClone(catalog);
  } } };
  try {
    assert.deepEqual(await readAstroneerSaveCatalog(catalog.instance_id), catalog);
    assert.deepEqual(calls[0], { command: "read_astroneer_save_catalog", args: { instanceId: catalog.instance_id } });
    fail = true;
    await assert.rejects(readAstroneerSaveCatalog(catalog.instance_id), /Native catalog read failure/);
  } finally {
    if (previousWindow === undefined) delete globalThis.window; else globalThis.window = previousWindow;
    if (hadIsTauri) globalThis.isTauri = previousIsTauri; else delete globalThis.isTauri;
  }
});
