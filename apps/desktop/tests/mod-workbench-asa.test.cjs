const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript, parseSource, sourceText, visitSyntax } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) require.extensions[extension] = (module, filename) =>
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { canonicalAsaModId, parseAsaModIds, readAsaModMembership, buildAsaModPlan, hasAsaRawModFlags } = require("../src/views/servers/mod-workbench-asa.ts");
const { buildConfiguredEntries, buildEnabledRows } = require("../src/views/servers/mod-workbench-model.ts");
const X = "1346144", Y = "200", Z = "300";
const moduleDetails = { mods: { enablement: { setting_key: "mod_ids_csv", setting_label: "ASA Mod IDs", id_strategy: "numeric_prefix" } } };
const rows = (settings) => buildEnabledRows(buildConfiguredEntries("arksurvivalascended", settings, moduleDetails));

test("ASA ID-only entries survive disable, restart and enable without local files", () => {
  const initial = { mod_ids_csv: X, passive_mod_ids_csv: Y, options: { [X]: { value: 42 } } };
  const before = structuredClone(initial);
  const disabled = buildAsaModPlan(initial, [X], "disable");
  assert.equal(disabled.mod_ids_csv, "");
  assert.deepEqual(disabled.curseforge_disabled_mod_ids, [X]);
  assert.deepEqual(rows(JSON.parse(JSON.stringify(disabled))).map((row) => row.id), [Y, X]);
  assert.equal(readAsaModMembership(disabled).enabled.has(X), false);
  const restored = buildAsaModPlan(disabled, [X], "enable");
  assert.equal(restored.mod_ids_csv, X);
  assert.equal(restored.passive_mod_ids_csv, Y);
  assert.deepEqual(restored.options, initial.options);
  assert.deepEqual(initial, before);
});

test("ASA passive and combined modes fully stop and restore their exact loading lists", () => {
  const initial = { mod_ids_csv: `${X},${Y}`, passive_mod_ids_csv: `${Y}\n${Z}` };
  assert.deepEqual(rows(initial).map((row) => row.id), [X, Y, Z]);
  const disabled = buildAsaModPlan(initial, [Y, Z], "disable");
  assert.equal(disabled.mod_ids_csv, X); assert.equal(disabled.passive_mod_ids_csv, "");
  assert.deepEqual(disabled.curseforge_disabled_mod_ids, [Y]);
  assert.deepEqual(disabled.curseforge_disabled_passive_mod_ids, [Y, Z]);
  const restored = buildAsaModPlan(disabled, [Y, Z], "enable");
  assert.deepEqual(parseAsaModIds(restored.mod_ids_csv), [X, Y]);
  assert.deepEqual(parseAsaModIds(restored.passive_mod_ids_csv), [Y, Z]);
  assert.equal(rows(restored).filter((row) => row.id === Y).length, 1);
});

test("ASA explicit removal clears ownership in both modes and retained files cannot resurrect it", () => {
  const initial = { mod_ids_csv: X, passive_mod_ids_csv: X, options: { retained: true } };
  const removed = buildAsaModPlan(initial, [X], "remove", [X]);
  assert.deepEqual(rows(removed), []); assert.equal(removed.mod_ids_csv, ""); assert.equal(removed.passive_mod_ids_csv, "");
  assert.equal(readAsaModMembership(removed).removed.has(X), true);
  assert.throws(() => buildAsaModPlan(removed, [X], "enable", [X]), (error) => error.code === "not-owned");
  const restored = buildAsaModPlan(removed, [`00${X}`], "add");
  assert.equal(restored.mod_ids_csv, X); assert.equal(readAsaModMembership(restored).removed.has(X), false);
  assert.deepEqual(restored.options, initial.options);
});

test("ASA explicit file import restores only specified IDs as disabled without touching other retained caches", () => {
  const initial = { curseforge_removed_mod_ids: [X, Y], options: { retained: true } };
  const staged = buildAsaModPlan(initial, [X], "restore-files");
  assert.equal(readAsaModMembership(staged).owned.has(X), true);
  assert.equal(readAsaModMembership(staged).enabled.has(X), false);
  assert.equal(readAsaModMembership(staged).removed.has(Y), true);
  assert.deepEqual(rows(staged).map((row) => row.id), [X]);
});

test("ASA raw active/passive edits override historical modes and remove markers", () => {
  const disabled = buildAsaModPlan({ passive_mod_ids_csv: X }, [X], "disable");
  const edited = { ...disabled, mod_ids_csv: X, curseforge_removed_mod_ids: [X] };
  assert.equal(readAsaModMembership(edited).owned.has(X), true);
  const stopped = buildAsaModPlan(edited, [X], "disable");
  assert.deepEqual(stopped.curseforge_disabled_passive_mod_ids, []);
  const restored = buildAsaModPlan(stopped, [X], "enable");
  assert.equal(restored.mod_ids_csv, X); assert.equal(restored.passive_mod_ids_csv, "");
});

test("ASA custom Mod flags block structured changes with native quote and escape semantics", () => {
  for (const raw of ["-mods", "-PASSIVEMODS", "-mods=123", "-passivemods 123", '"-mods=123"', "'-passivemods' '123'",
    '-NoBattlEye "-mods"="123"', '-log\t-passivemods="123,456"', '-m"od"s=123', '-NoBattlEye\u0085-mods=123']) {
    const settings = { mod_ids_csv: X, custom_launch_flags: raw };
    assert.equal(hasAsaRawModFlags(settings), true, raw);
    for (const action of ["disable", "enable", "remove", "add", "restore-files"]) {
      assert.throws(() => buildAsaModPlan(settings, [X], action), (error) => error.code === "raw-mod-flags", `${raw}: ${action}`);
    }
    assert.equal(settings.custom_launch_flags, raw);
  }
  for (const raw of ["", "-NoBattlEye", "-modsettings=123", "-passivemodsExtra=123", '-Note="mentions -mods=123"',
    String.raw`-Note="escaped \"-mods=123\""`, '"-mods 123"', "--mods=123", '-NoBattlEye\uFEFF-mods=123']) {
    assert.equal(hasAsaRawModFlags({ custom_launch_flags: raw }), false, raw);
  }
});

test("ASA accepts native numeric reference forms and removes IDs rather than leaving URL loading aliases", () => {
  const raw = `cf-${X}，https://www.curseforge.com/ark-survival-ascended/projects/${Y} mod_id=${Z}`;
  assert.deepEqual(parseAsaModIds(raw), [X, Y, Z]);
  const next = buildAsaModPlan({ mod_ids_csv: raw, passive_mod_ids_csv: `https://example/?projectId=${X}&id=${Y}` }, [X], "remove");
  assert.deepEqual(parseAsaModIds(next.mod_ids_csv), [Y, Z]);
  assert.deepEqual(parseAsaModIds(next.passive_mod_ids_csv), [Y]);
  assert.equal(canonicalAsaModId("000200"), Y);
  for (const invalid of ["0", "-1", "abc", "18446744073709551616", "1".repeat(21)]) {
    assert.equal(canonicalAsaModId(invalid), null);
    assert.throws(() => buildAsaModPlan({}, [invalid], "add"), (error) => error.code === "invalid-id");
  }
  assert.throws(() => buildAsaModPlan({}, [X], "disable"), (error) => error.code === "not-owned");
});

test("ASA actual reorder resolves native aliases and preserves hidden active/passive overlap", async () => {
  const filename = require.resolve("../src/views/servers/ModWorkbench.tsx"), source = fs.readFileSync(filename, "utf8");
  const names = ["entryValueParser", "handleReorderEnabledRow", "assertAsaMembershipBase"], declarations = [];
  visitSyntax(parseSource(source, filename), (node) => {
    if (node.type === "FunctionDeclaration" && names.includes(node.identifier?.value)) declarations.push(sourceText(source, node));
  });
  const model = require("../src/views/servers/mod-workbench-model.ts");
  const asa = require("../src/views/servers/mod-workbench-asa.ts");
  const patch = require("../src/views/servers/mod-settings-patch.ts");
  let current = { mod_ids_csv: X, passive_mod_ids_csv: `cf-${Y} https://www.curseforge.com/ark-survival-ascended/projects/${X} mod:${Z}` }, pending, saved;
  const context = { ...model, ...asa, ...patch, moduleId: "arksurvivalascended", exports: {}, latestSettingsRef: { current },
    readWritableInstanceState: async () => ({ settings: current }),
    launchModMutation: (_kind, operation) => { pending = operation(); },
    persistSettings: async (next, base, _removal, validate) => {
      assert.equal(base, current); validate(current); saved = next;
    }, startTransition: (callback) => callback(), setApplyMessage() {}, setSelectedEnabledRowKey() {},
    setScanNonce() {}, setActiveDetailSource() {}, t: (_key, _params, fallback) => fallback };
  vm.runInNewContext(transpileTypeScript(`${declarations.join("\n")}\nexports.run = handleReorderEnabledRow;`, filename), context, { filename });
  const passive = rows(current).filter((row) => row.entry.fieldLabel === "passive_mod_ids_csv");
  assert.deepEqual(passive.map((row) => row.id), [Y, Z]);
  context.exports.run(passive[0], passive[1]); await pending;
  assert.ok(saved, "The visibly draggable native-reference row must produce a save");
  assert.deepEqual(parseAsaModIds(saved.passive_mod_ids_csv), [X, Z, Y]);
  assert.equal(saved.mod_ids_csv, X);
});
