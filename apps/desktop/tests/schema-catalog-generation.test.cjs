const assert = require("node:assert/strict");
const test = require("node:test");
const { addGeneratedSettingsEntry, mergeModuleCatalog, parseGeneratedChunk, omitOverriddenMessages } = require("../scripts/generate_i18n_schema_catalogs.cjs");

test("generated fallbacks omit only keys overridden later in the runtime locale", () => {
  const earlier = { shared: "Earlier catalog", earlierOnly: "Earlier help" };
  const generated = { shared: "Schema help", authored: "Generated help", missing: "Fallback", blank: "Fallback" };
  const authored = { authored: "Authored help", blank: "" };
  const source = `
    import { EARLIER } from "./earlier";
    import { GENERATED } from "./generated";
    import { COPY as AUTHORED } from "./authored";
    export const MESSAGES = { ...EARLIER, ...GENERATED, ...AUTHORED };
  `;
  const fallback = omitOverriddenMessages(generated, source, "GENERATED", (file, name) => {
    assert.equal(file, "./authored");
    assert.equal(name, "COPY");
    return authored;
  });
  assert.deepEqual(fallback, { shared: "Schema help", missing: "Fallback" });
  assert.deepEqual({ ...earlier, ...fallback, ...authored }, { ...earlier, ...generated, ...authored });
  assert.throws(() => omitOverriddenMessages(generated, "export const MESSAGES = {};", "GENERATED", () => authored),
    /Missing generated catalog spread/);
});

test("scoped schema generation removes withdrawn help and preserves other modules", () => {
  const catalog = mergeModuleCatalog({
    "settings.schema.arksurvivalevolved.option.title": "Option",
    "settings.schema.arksurvivalevolved.option.description": "Withdrawn source prose",
    "settings.schema.minecraft.option.description": "Independent project help",
    "ark.settings.shared.title": "Shared title"
  }, {
    "settings.schema.arksurvivalevolved.option.title": "Current option"
  }, ["arksurvivalevolved"]);
  assert.equal(catalog["settings.schema.arksurvivalevolved.option.title"], "Current option");
  assert.equal(catalog["settings.schema.arksurvivalevolved.option.description"], undefined);
  assert.equal(catalog["settings.schema.minecraft.option.description"], "Independent project help");
  assert.equal(catalog["ark.settings.shared.title"], "Shared title");
});

test("missing field help does not invent a group description or discard existing authored help", () => {
  const en = {};
  const zh = {};
  const key = "settings.schema.arksurvivalevolved.option.description";
  addGeneratedSettingsEntry(en, zh, key, "");
  assert.equal(en[key], undefined);
  assert.equal(zh[key], undefined);
  en[key] = "Project-authored explanation.";
  zh[key] = "项目自行编写的说明。";
  addGeneratedSettingsEntry(en, zh, key, "");
  assert.equal(en[key], "Project-authored explanation.");
  assert.equal(zh[key], "项目自行编写的说明。");
});

test("scoped generation rejects malformed records instead of silently dropping unrelated messages", () => {
  const chunk = (body) => `export const MESSAGES: MessageCatalog = {\n${body}\n};\n`;
  assert.deepEqual(parseGeneratedChunk(chunk('  "help": "Text with a \\"quote\\"",')), { help: 'Text with a "quote"' });
  assert.throws(() => parseGeneratedChunk(chunk('  "help": unknown,')), /Invalid generated message entry/);
  assert.throws(() => parseGeneratedChunk(chunk('  "help": "one",\n  "help": "two"')), /Duplicate generated message/);
});
