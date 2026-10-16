const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const { projectValheimModifier, valheimPresetKeys } = require("../src/views/settings/valheim-world-rules.ts");

test("Valheim next-start projection preserves native keys not replaced by a modifier", () => {
  const hard = [...valheimPresetKeys("hard")];
  const original = [...hard];
  assert.equal(projectValheimModifier(hard, "combat", "combat easy"), "custom");
  assert.equal(projectValheimModifier(["noportals"], "portals", "portals casual"), "custom");
  assert.equal(projectValheimModifier(["deathdeleteitems", "deathskillsreset"], "deathpenalty", "deathpenalty casual"), "custom");
  assert.equal(projectValheimModifier([], "combat", "combat easy"), "easy");
  assert.equal(projectValheimModifier([], "portals", "portals casual"), "casual");
  assert.deepEqual(hard, original, "Prediction never mutates the saved baseline");
});

test("Valheim unknown baselines resolve only modifiers whose native key set is fully determined", () => {
  assert.equal(projectValheimModifier(null, "combat", "combat hard"), "hard");
  assert.equal(projectValheimModifier(null, "resources", "resources most"), "most");
  assert.equal(projectValheimModifier(null, "combat", "combat easy"), null);
  assert.equal(projectValheimModifier(null, "portals", "portals casual"), null);
  assert.equal(projectValheimModifier(null, "deathpenalty", "deathpenalty casual"), null);
  assert.equal(projectValheimModifier(null, "combat", undefined), null);
});

test("Valheim native repeated modifiers apply in order without losing earlier retained keys", () => {
  assert.equal(projectValheimModifier([], "combat", "combat hard\ncombat easy"), "custom");
  assert.equal(projectValheimModifier([], "combat", "combat easy\ncombat hard"), "hard");
  assert.equal(projectValheimModifier(null, "combat", "combat hard\ncombat easy"), "custom");
  assert.equal(projectValheimModifier([], "combat", "combat unknown"), null);
  assert.equal(projectValheimModifier([], "combat", ["combat easy"]), null);
});

test("Valheim projections expand saved preset tags before applying startup modifiers exactly once", () => {
  const taggedHard = Object.freeze(["preset hard"]);
  const taggedHammer = Object.freeze(["preset hammer"]);
  const taggedCustom = Object.freeze(["preset combat_hard:resources_most"]);
  assert.equal(projectValheimModifier(taggedHard, "combat", "combat easy"), "custom");
  assert.equal(projectValheimModifier(taggedHammer, "raids", "raids more"), "more");
  assert.equal(projectValheimModifier(taggedCustom, "resources", "resources less"), "less");
  assert.deepEqual(taggedHard, ["preset hard"]);
  assert.deepEqual(taggedHammer, ["preset hammer"]);
  assert.deepEqual(taggedCustom, ["preset combat_hard:resources_most"]);
});
