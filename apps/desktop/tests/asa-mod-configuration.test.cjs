const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) require.extensions[extension] = (module, filename) =>
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const model = require("../src/views/servers/asa-mod-configuration.ts");
const { asaModConfigurationText } = require("../src/views/servers/asa-mod-configuration-messages.ts");
const { parseAsaModIni: parse, decodeAsaModIniFile: decode, buildAsaModConfigurationSettings: build } = model;
const gameMode = "/script/shootergame.shootergamemode";
const settings = () => ({ mod_ids_csv: "1254808,928708", passive_mod_ids_csv: "500", retained: { marker: "keep" },
  asa_mod_configurations: {
    "1254808": { game_ini: "[Fixture]\nKeep=1\nKeep=2\n", game_user_settings_ini: "[BetterVariants]\nDebug=False\n" },
    "928708": { game_ini: "[OtherFixture]\nOriginal=yes\n", game_user_settings_ini: "" },
  } });
const code = (expected) => (error) => error instanceof model.AsaModConfigurationError && error.issue.code === expected;

test("per-Mod INI preview preserves sections, repeated assignments, order, comments and literal values", () => {
  const text = "; author comment\r\n[Fixture]\r\nKey=one=two\r\nKey=\"<script>alert(1)</script>\"\r\n[Next] # comment\r\nOther=3\r\n";
  const result = parse(text);
  assert.deepEqual(result.issues, []); assert.equal(result.text, text);
  assert.deepEqual(result.entries, [
    { section: "Fixture", key: "Key", value: "one=two", line: 3 },
    { section: "Fixture", key: "Key", value: '"<script>alert(1)</script>"', line: 4 },
    { section: "Next", key: "Other", value: "3", line: 6 },
  ]);
});
test("INI syntax reports missing section, malformed headers, missing assignment and controls instead of accepting a partial file", () => {
  for (const [raw, expected] of [["Key=1", "assignment"], ["[]\nKey=1", "section"], ["[Incomplete", "section"],
    ["[Section] trailing text\nKey=1", "section"], ["[Section]\nnot an assignment", "assignment"], ["[Section]\nKey=a\0b", "control"]]) {
    assert.equal(parse(raw).issues.some((issue) => issue.code === expected), true, raw);
    assert.throws(() => build(settings(), "1254808", "game_ini", raw, settings().asa_mod_configurations["1254808"].game_ini), code(expected));
  }
  assert.equal(parse(" \r\n\t").isClear, true);
  assert.equal(parse("// author comment\n[Fixture]\nEmpty=\n").issues.length, 0);
});
test("INI import decodes UTF-8 and UTF-16 BOM files without guessing unmarked code pages", () => {
  const text = "[Fixture]\r\n名字=中文\r\n";
  assert.equal(decode(new TextEncoder().encode(text)), text);
  assert.equal(decode(Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), Buffer.from(text)])), text);
  const le = Buffer.from(text, "utf16le");
  assert.equal(decode(Buffer.concat([Buffer.from([0xff, 0xfe]), le])), text);
  const be = Buffer.from(le); be.swap16();
  assert.equal(decode(Buffer.concat([Buffer.from([0xfe, 0xff]), be])), text);
  assert.throws(() => decode(Uint8Array.from([0xff, 0xfe, 0x41])), code("encoding"));
  assert.throws(() => decode(Uint8Array.from([0xc3, 0x28])), code("encoding"));
  assert.throws(() => decode(Buffer.alloc(65537)), code("size"));
  assert.equal(parse("模".repeat(21846)).issues[0].code, "size");
});
test("saving one destination preserves the other file, other Mods and the complete baseline", () => {
  const before = settings(), original = structuredClone(before), text = "[Fixture]\nKeep=3\nKeep=4\n";
  const next = build(before, "1254808", "game_ini", text, before.asa_mod_configurations["1254808"].game_ini);
  assert.equal(next.asa_mod_configurations["1254808"].game_ini, text);
  assert.equal(next.asa_mod_configurations["1254808"].game_user_settings_ini, before.asa_mod_configurations["1254808"].game_user_settings_ini);
  assert.deepEqual(next.asa_mod_configurations["928708"], before.asa_mod_configurations["928708"]);
  assert.deepEqual(next.retained, before.retained); assert.equal(next.mod_ids_csv, before.mod_ids_csv); assert.deepEqual(before, original);
  assert.throws(() => build(before, "1254808", "game_ini", text, "stale"), code("base-changed"));
});
test("clearing is limited to the reviewed Mod and file and removes its empty entry only after both files are empty", () => {
  const before = settings();
  const cleared = build(before, "1254808", "game_ini", " \n", before.asa_mod_configurations["1254808"].game_ini);
  assert.equal(cleared.asa_mod_configurations["1254808"].game_ini, "");
  assert.equal(cleared.asa_mod_configurations["1254808"].game_user_settings_ini, before.asa_mod_configurations["1254808"].game_user_settings_ini);
  const removed = build(cleared, "1254808", "game_user_settings_ini", "", cleared.asa_mod_configurations["1254808"].game_user_settings_ini);
  assert.equal(removed.asa_mod_configurations["1254808"], undefined);
  assert.deepEqual(removed.asa_mod_configurations["928708"], before.asa_mod_configurations["928708"]);
});
test("active Mod collisions are scoped to file, ASCII-folded section and exact key", () => {
  const before = settings();
  const conflict = "[OTHERFIXTURE]\noriginal=no\n";
  assert.throws(() => build(before, "1254808", "game_ini", conflict, before.asa_mod_configurations["1254808"].game_ini), code("conflict"));
  assert.doesNotThrow(() => build(before, "1254808", "game_user_settings_ini", conflict, before.asa_mod_configurations["1254808"].game_user_settings_ini));
  before.mod_ids_csv = "1254808";
  const warnings = model.findAsaModIniConflicts(before, "1254808", "game_ini", parse(conflict).entries);
  assert.equal(warnings[0].severity, "warning");
  assert.doesNotThrow(() => build(before, "1254808", "game_ini", conflict, before.asa_mod_configurations["1254808"].game_ini));
});
test("raw extra collisions honor the native default section and explicit sections", () => {
  const before = settings(); before.game_user_settings_extra = "[BetterVariants]\nDebug=True\n";
  assert.throws(() => build(before, "1254808", "game_user_settings_ini", "[bettervariants]\ndebug=False\n",
    before.asa_mod_configurations["1254808"].game_user_settings_ini), code("conflict"));
  before.game_ini_extra = "Custom=1\n";
  assert.throws(() => build(before, "1254808", "game_ini", `[${gameMode}]\nCustom=2\n`, before.asa_mod_configurations["1254808"].game_ini), code("conflict"));
  before.game_user_settings_extra = "Message=raw\n";
  assert.throws(() => build(before, "1254808", "game_user_settings_ini", "[MessageOfTheDay]\nMessage=mod\n",
    before.asa_mod_configurations["1254808"].game_user_settings_ini), code("conflict"));
});
test("class-keyed crafting, stacking and engram rules defer identity validation to native instead of a same-key rejection", () => {
  for (const key of ["ConfigOverrideItemCraftingCosts", "ConfigOverrideItemMaxQuantity", "OverrideNamedEngramEntries"]) {
    const before = settings(); before.game_ini_extra = `${key}=(Fixture=\"A\")\n`;
    before.asa_mod_configurations["928708"].game_ini = `[${gameMode}]\n${key}=(Fixture=\"B\")\n`;
    const entries = parse(`[${gameMode}]\n${key}=(Fixture=\"C\")\n`).entries;
    assert.deepEqual(model.findAsaModIniConflicts(before, "1254808", "game_ini", entries), []);
  }
});
test("adapted fields modify the same INI string, preserve unknown values and refuse ambiguous repeated assignments", () => {
  const parameter = { file: "game_user_settings_ini", section: "BetterVariants", key: "Debug", label: "Debug", kind: "boolean", defaultValue: "False" };
  const before = "; comment\r\n[BetterVariants]\r\nUnknown=keep\r\nDebug=False\r\n";
  const changed = model.setAsaModIniParameter(before, parameter, "True");
  assert.equal(changed, before.replace("Debug=False", "Debug=True"));
  assert.equal(model.asaModParameterValue(changed, parameter), "True");
  assert.equal(model.setAsaModIniParameter(changed, parameter, null), "; comment\r\n[BetterVariants]\r\nUnknown=keep\r\n");
  assert.equal(model.setAsaModIniParameter("", parameter, "True"), "[BetterVariants]\nDebug=True\n");
  assert.throws(() => model.setAsaModIniParameter("[BetterVariants]\nDebug=True\nDebug=False\n", parameter, "True"), code("duplicate-parameter"));
  assert.throws(() => model.setAsaModIniParameter(before, parameter, "True\nInjected=1"), code("control"));
});
test("configuration shape and resource limits reject malformed records instead of dropping their content", () => {
  for (const raw of [null, [], { "01254808": {} }, { "1254808": { game_ini: 3 } }, { "1254808": { unknown: "keep" } }, { "1254808": { constructor: "not a file" } }]) {
    assert.throws(() => model.readAsaModConfigurations({ asa_mod_configurations: raw }));
  }
  assert.deepEqual(model.readAsaModConfigurations({ asa_mod_configurations: { "1254808": { game_ini: "[Fixture]\nKey=1\n" } } }),
    { "1254808": { game_ini: "[Fixture]\nKey=1\n", game_user_settings_ini: "" } });
  const tooMany = Object.fromEntries(Array.from({ length: 257 }, (_, index) => [String(index + 1), { game_ini: "", game_user_settings_ini: "" }]));
  assert.throws(() => model.readAsaModConfigurations({ asa_mod_configurations: tooMany }), code("count"));
  const tooBig = Object.fromEntries(Array.from({ length: 17 }, (_, index) => [String(index + 1), { game_ini: "x".repeat(65536), game_user_settings_ini: "" }]));
  assert.throws(() => model.readAsaModConfigurations({ asa_mod_configurations: tooBig }), code("total-size"));
});
test("unknown Mod guidance is localized and makes no claim that the Mod has no configurable parameters", () => {
  assert.match(asaModConfigurationText("zh-CN", "unknown"), /尚未适配.*作者参数/);
  assert.match(asaModConfigurationText("en-US", "unknown"), /author-defined fields.*not been adapted/);
  for (const locale of ["zh-CN", "en-US"]) {
    assert.match(asaModConfigurationText(locale, "saveHelp", { file: "Game.ini" }), /Game\.ini/);
    assert.match(asaModConfigurationText(locale, "clearReview", { id: "1254808", file: "Game.ini" }), /1254808.*Game\.ini/);
  }
});
test("the verified Better Variants catalog declares only its two Boolean INI keys and never inserts defaults", () => {
  const author = model.ASA_MOD_AUTHOR_CATALOG.find((entry) => entry.modId === "1254808");
  assert.equal(author.title, "Better Variants");
  assert.deepEqual(author.parameters.map(({ file, section, key, kind, defaultValue }) => ({ file, section, key, kind, defaultValue })), [
    { file: "game_user_settings_ini", section: "BetterVariants", key: "Debug", kind: "boolean", defaultValue: "False" },
    { file: "game_user_settings_ini", section: "BetterVariants", key: "DebugSpawns", kind: "boolean", defaultValue: "False" },
  ]);
  assert.deepEqual(author.parameters[1].requires, { file: "game_user_settings_ini", section: "BetterVariants", key: "Debug", value: "True" });
  for (const parameter of author.parameters) {
    assert.equal(model.asaModParameterValue("", parameter), undefined);
    assert.equal(model.setAsaModIniParameter("", parameter, null), "");
  }
});
