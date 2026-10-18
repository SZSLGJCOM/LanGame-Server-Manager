const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) require.extensions[extension] = (module, filename) =>
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { parseAsaModManifest } = require("../src/views/servers/asa-mod-manifest.ts");
const { buildAsaActiveListPlan, readAsaModMembership } = require("../src/views/servers/mod-workbench-asa.ts");

test("ASA manifest canonicalizes numeric IDs and preserves first occurrence order", () => {
  assert.deepEqual(parseAsaModManifest("001346144,1346145;1346144\n1346146，1346145；1346147、1346146"), {
    ids: ["1346144", "1346145", "1346146", "1346147"], invalidTokens: [], duplicateCount: 3, tooLarge: false
  });
});
test("ASA manifest reports every bad token instead of saving a partial list", () => {
  const invalid = ["0", "-1", "12.5", "1e6", "1346144-dev", "18446744073709551616", "https://example.com/mods/1346144", "slug"];
  const parsed = parseAsaModManifest(["1346144", ...invalid].join("\n"));
  assert.deepEqual(parsed.ids, ["1346144"]); assert.deepEqual(parsed.invalidTokens, invalid);
  assert.equal(parseAsaModManifest("18446744073709551615").ids[0], "18446744073709551615");
});
test("ASA manifest bounds UTF-8 bytes and distinct IDs while counting repeated IDs", () => {
  assert.equal(parseAsaModManifest(" ".repeat(1048576)).tooLarge, false);
  assert.equal(parseAsaModManifest(" ".repeat(1048577)).tooLarge, true);
  assert.equal(parseAsaModManifest("模".repeat(349526)).tooLarge, true);
  const ids = Array.from({ length: 8193 }, (_, index) => String(index + 1));
  assert.equal(parseAsaModManifest(ids.slice(0, 8192).join("\n")).tooLarge, false);
  assert.equal(parseAsaModManifest(ids.join("\n")).tooLarge, true);
  assert.equal(parseAsaModManifest(Array(9000).fill("1346144").join("\n")).duplicateCount, 8999);
});
test("ASA checklist replaces the active list exactly and keeps dropped Mods in My Mods as disabled", () => {
  const current = { mod_ids_csv: "100\ncf-200\n300", passive_mod_ids_csv: "200", curseforge_disabled_mod_ids: ["400"],
    curseforge_disabled_passive_mod_ids: ["500"], curseforge_removed_mod_ids: ["600"], asa_mod_configurations: { 100: { game_ini: "[A]" } },
    ark_mod_map_overrides: { island: { mods: { 300: { active: true } } } }, unrelated: "keep" };
  const before = structuredClone(current), parsed = parseAsaModManifest("0600,400,100,600");
  const next = buildAsaActiveListPlan(current, parsed.ids);
  assert.equal(next.mod_ids_csv, "600\n400\n100", "the written order becomes the load order");
  assert.deepEqual(readAsaModMembership(next).active, ["600", "400", "100"]);
  assert.deepEqual(next.curseforge_disabled_mod_ids, ["200", "300"], "IDs dropped from the list stay in My Mods as disabled");
  assert.deepEqual(next.curseforge_removed_mod_ids, [], "listing a removed Mod brings it back");
  for (const key of ["passive_mod_ids_csv", "curseforge_disabled_passive_mod_ids", "asa_mod_configurations", "ark_mod_map_overrides", "unrelated"]) {
    assert.deepEqual(next[key], current[key], `${key} is not part of the checklist`);
  }
  assert.deepEqual(current, before);
});
test("ASA checklist writes nothing for an unchanged list and accepts an empty list", () => {
  const current = { mod_ids_csv: "100\n300", curseforge_disabled_mod_ids: ["400"] };
  assert.deepEqual(buildAsaActiveListPlan(current, ["100", "300"]), current);
  const cleared = buildAsaActiveListPlan(current, []);
  assert.equal(cleared.mod_ids_csv, "");
  assert.deepEqual(cleared.curseforge_disabled_mod_ids, ["400", "100", "300"]);
  assert.throws(() => buildAsaActiveListPlan(current, ["12.5"]), /invalid-id/);
  assert.throws(() => buildAsaActiveListPlan({ ...current, custom_launch_flags: "-mods=1" }, ["100"]), /raw-mod-flags/);
});
