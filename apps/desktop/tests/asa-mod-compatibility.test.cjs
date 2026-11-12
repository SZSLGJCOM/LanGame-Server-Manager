const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { readAsaModCompatibility, buildAsaModCompatibilityPlan } = require("../src/views/servers/asa-mod-compatibility.ts");

test("ASA compatibility reads existing defaults, actual PC flags and custom platforms without rewriting them", () => {
  for (const settings of [{}, { server_platform: "ALL" }, { server_platform: " all ", custom_launch_flags: '-Note="-pconlymods"' }]) {
    assert.equal(readAsaModCompatibility(settings).mode, "crossplay");
  }
  for (const flags of ["-pconlymods", "'-PCONLYMODS'", '-pc"only"mods', "-pconlymods=true"]) {
    assert.equal(readAsaModCompatibility({ server_platform: "PC", custom_launch_flags: flags }).mode, "steam");
  }
  for (const settings of [{ server_platform: "PC+PS5" }, { server_platform: "PC" }, { custom_launch_flags: "-pconlymods" }]) {
    assert.equal(readAsaModCompatibility(settings).mode, "custom");
  }
});

test("Steam choice sets both native options while preserving all Mod lists and unrelated launch arguments", () => {
  const settings = { server_platform: "ALL", mod_ids_csv: "1786680\n947033", passive_mod_ids_csv: "877752", ark_mod_map_overrides: { keep: true },
    custom_launch_flags: String.raw`-NoBattlEye -Note="mentions -pconlymods with \"quotes\""` };
  const original = structuredClone(settings);
  const next = buildAsaModCompatibilityPlan(settings, "steam");
  assert.equal(next.server_platform, "PC");
  assert.equal(next.custom_launch_flags, `${settings.custom_launch_flags} -pconlymods`);
  assert.deepEqual({ ...next, server_platform: settings.server_platform, custom_launch_flags: settings.custom_launch_flags }, settings);
  assert.deepEqual(settings, original);
  assert.equal(readAsaModCompatibility(next).mode, "steam");
});

test("cross-platform choice removes only the actual PC-only flags, including duplicate quoted tokens", () => {
  const flags = String.raw`-NoBattlEye  "-pconlymods" -Note="keep -pconlymods" '-PCONLYMODS' -log`;
  const next = buildAsaModCompatibilityPlan({ server_platform: "PC", custom_launch_flags: flags, mod_ids_csv: "1786680" }, "crossplay");
  assert.equal(next.server_platform, "ALL");
  assert.equal(next.custom_launch_flags, '-NoBattlEye   -Note="keep -pconlymods"  -log');
  assert.equal(next.mod_ids_csv, "1786680", "The operator decides which incompatible Mods to disable");
  assert.equal(readAsaModCompatibility(next).mode, "crossplay");
  assert.equal(buildAsaModCompatibilityPlan({ custom_launch_flags: "-pconlymods -pconlymods" }, "steam").custom_launch_flags, " -pconlymods");
});

test("ambiguous raw flags remain untouched instead of silently changing platform precedence", () => {
  for (const [raw, problem] of [['-Note="unclosed', "invalidFlags"], ["-serverplatform=PC+PS5", "rawPlatform"], ["'-ServerPlatform' ALL", "rawPlatform"]]) {
    const settings = { custom_launch_flags: raw, server_platform: "ALL" };
    assert.equal(readAsaModCompatibility(settings).problem, problem);
    assert.throws(() => buildAsaModCompatibilityPlan(settings, "steam"), new RegExp(problem));
    assert.equal(settings.custom_launch_flags, raw);
  }
});
