const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) require.extensions[extension] = (module, filename) =>
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { parseAsaModManifest } = require("../src/views/servers/asa-mod-manifest.ts");
const { buildAsaModPlan, parseAsaModIds } = require("../src/views/servers/mod-workbench-asa.ts");

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
test("ASA manifest merge uses real membership planning and preserves passive loading and options", () => {
  const current = { mod_ids_csv: "100", passive_mod_ids_csv: "200", curseforge_disabled_mod_ids: ["300"],
    curseforge_removed_mod_ids: ["400"], options: { retained: true }, unrelated: "keep" };
  const before = structuredClone(current), parsed = parseAsaModManifest("0300,400,300");
  const next = buildAsaModPlan(current, parsed.ids, "add");
  assert.deepEqual(parseAsaModIds(next.mod_ids_csv), ["100", "300", "400"]);
  assert.equal(next.passive_mod_ids_csv, "200"); assert.deepEqual(next.options, current.options);
  assert.equal(next.unrelated, "keep"); assert.deepEqual(next.curseforge_disabled_mod_ids, []);
  assert.deepEqual(next.curseforge_removed_mod_ids, []); assert.deepEqual(current, before);
});
