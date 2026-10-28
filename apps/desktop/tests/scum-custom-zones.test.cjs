const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => module._compile(
    transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}
require.extensions[".css"] = (module) => module._compile("", module.filename);
const load = Module._load;
Module._load = function (request, parent, isMain) {
  if (/api-transport$/u.test(request)) return { invokeOrMock: async () => { throw new Error("No transport in unit fixture"); } };
  return load.call(this, request, parent, isMain);
};
const { MockScumCustomZones } = require("../src/api-mock/scum-custom-zones.ts");
const { buildScumCustomZoneDamageInput } = require("../src/scum-custom-zones.ts");
const details = { summary: { id: "scum-zone-fixture", module_id: "scum", status: "Stopped", active_process_count: 0 }, active_run: null };

test("SCUM edits one existing configuration, retains other zones and rejects stale writes", () => {
  const store = new MockScumCustomZones();
  const before = store.read(details);
  const zone = before.configurations[1];
  const damage = { melee: "block", melee_weapon: "allow", projectile: "ignore" };
  const input = buildScumCustomZoneDamageInput(before, zone, damage);
  const after = store.write(details, input);
  assert.deepEqual(after.configurations[0], before.configurations[0]);
  assert.deepEqual(after.configurations[1].damage, damage);
  assert.deepEqual(after.configurations[1].regions, zone.regions);
  assert.notEqual(after.revision, before.revision);
  assert.ok(after.backup_id);
  assert.throws(() => store.write(details, input), /changed/u);
});

test("SCUM cannot save while running, with unknown handling or a missing configuration", () => {
  const store = new MockScumCustomZones();
  const before = store.read(details);
  const input = buildScumCustomZoneDamageInput(before, before.configurations[0], before.configurations[0].damage);
  assert.throws(() => store.write({ summary: { ...details.summary, status: "Running", active_process_count: 1 } }, input));
  assert.throws(() => store.write({ ...details, active_run: { run_id: 7 } }, input));
  assert.throws(() => buildScumCustomZoneDamageInput(before, before.configurations[0], { ...input.damage, melee: "other" }));
  assert.throws(() => buildScumCustomZoneDamageInput(before, { ...before.configurations[0], configuration_id: 90 }, input.damage));
});

test("SCUM configuration exposes the real explicit-save renderer with complete bilingual labels", () => {
  const { scumSettingsDefinition } = require("../src/views/settings/modules/scum.ts");
  const registration = scumSettingsDefinition.specializedRenderers["scum-custom-zones"];
  assert.equal(registration?.sectionId, "custom-zones");
  assert.equal(registration?.saveMode, "explicit");
  for (const [suffix, name] of [["en", "EN_US_SCUM_MESSAGES"], ["zh-cn", "ZH_CN_SCUM_MESSAGES"]]) {
    const catalog = require(`../src/i18n/games/scum.${suffix}.ts`)[name];
    for (const field of ["melee", "melee_weapon", "projectile"]) {
      assert.ok(catalog[`scum.settings.customZones.fields.${field}.title`], `${suffix}: ${field}`);
      assert.ok(catalog[`scum.settings.customZones.fields.${field}.description`]);
    }
    for (const handling of ["ignore", "allow", "block"]) assert.ok(catalog[`scum.settings.customZones.handling.${handling}`]);
  }
  const renderer = fs.readFileSync(path.join(__dirname, "../src/views/settings/ScumCustomZonesRenderer.tsx"), "utf8");
  assert.match(renderer, /writeScumCustomZoneDamage\(input\)/u);
  assert.match(renderer, /generation\.current !== current/u);
});
