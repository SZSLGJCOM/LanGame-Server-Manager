const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};

const { getArkGmCreatureOptions, searchArkGmCreatureOptions, buildArkCreatureOptions,
  localizeArkCatalogName } = require("../src/views/servers/gm-tools.ts");
const { resolveArkCreatureBlueprint } = require("../src/views/servers/ark-creature-blueprints.ts");
const className = "Chameleon_Character_BP_C";
const blueprint = "/Game/ASA/Dinos/Chameleon/Chameleon_Character_BP.Chameleon_Character_BP_C";

test("Enigmasaur is an ASA-only catalog entry with the official English name", () => {
  const asa = getArkGmCreatureOptions("arksurvivalascended");
  const entries = asa.filter((option) => option.value === className);
  assert.equal(entries.length, 1);
  assert.equal(entries[0].name, "Enigmasaur");
  assert.equal(localizeArkCatalogName(entries[0], "en-US"), "Enigmasaur");
  assert.equal(localizeArkCatalogName(entries[0], "zh-CN"), "Enigmasaur");
  for (const query of ["Enigmasaur", className]) {
    assert.equal(searchArkGmCreatureOptions(query, 10, "arksurvivalascended")[0]?.value, className);
    assert.ok(!searchArkGmCreatureOptions(query, 10, "arksurvivalevolved").some((option) => option.value === className));
  }
  assert.ok(!getArkGmCreatureOptions("arksurvivalevolved").some((option) => option.value === className));
  assert.ok(buildArkCreatureOptions("arksurvivalascended", "{}").some((option) => option.value === className));
  assert.ok(!buildArkCreatureOptions("arksurvivalevolved", "{}").some((option) => option.value === className));
});

test("ASA resolves the Enigmasaur asset without changing ASE or manual full paths", () => {
  assert.equal(resolveArkCreatureBlueprint("arksurvivalascended", className), blueprint);
  assert.equal(resolveArkCreatureBlueprint("arksurvivalevolved", className), className);
  assert.equal(resolveArkCreatureBlueprint("arksurvivalascended", ` ${blueprint} `), blueprint);
  for (const moduleId of ["arksurvivalascended", "arksurvivalevolved"]) {
    assert.equal(resolveArkCreatureBlueprint(moduleId, "Rex_Character_BP_C"),
      "/Game/PrimalEarth/Dinos/Rex/Rex_Character_BP.Rex_Character_BP_C");
    assert.equal(resolveArkCreatureBlueprint(moduleId, "Dodo_Character_BP_C"),
      "/Game/PrimalEarth/Dinos/Dodo/Dodo_Character_BP.Dodo_Character_BP_C");
    assert.equal(resolveArkCreatureBlueprint(moduleId, "/Game/Mods/Manual.Manual_C"), "/Game/Mods/Manual.Manual_C");
  }
  const evidence = JSON.parse(fs.readFileSync(path.resolve(__dirname,
    "../../../modules/arksurvivalascended/reference-fixtures/2026-11-07-enigmasaur_asset_identity.json"), "utf8"));
  assert.equal(evidence.candidate.class_name, className);
  assert.equal(evidence.candidate.class_path, blueprint);
});
