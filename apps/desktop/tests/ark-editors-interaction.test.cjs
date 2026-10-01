const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const ext of [".ts", ".tsx"]) require.extensions[ext] = (m, f) => m._compile(transpileTypeScript(fs.readFileSync(f, "utf8"), f), f);
require.extensions[".css"] = (m) => m._compile("module.exports={};", m.filename);
const { ARK_EDITORS_EN } = require("../src/i18n/games/ark-editors.en.ts");
const { ARK_EDITORS_ZH_CN } = require("../src/i18n/games/ark-editors.zh-cn.ts");
const { validateArkComplexField } = require("../src/views/settings/ark-complex-validation.ts");
function descendants(node) { return React.isValidElement(node) ? [node, ...React.Children.toArray(node.props.children).flatMap(descendants)] : []; }
function load(name, exportName, props, catalog = ARK_EDITORS_EN) {
  const slots = []; let cursor = 0;
  const filename = path.resolve(__dirname, `../src/views/settings/${name}.tsx`);
  const loaded = new Module(filename, module); loaded.filename = filename;
  const originalRequire = Module.createRequire(filename);
  loaded.require = (id) => id === "react" ? { ...React,
    useState(initial) { const slot = cursor++; if (!(slot in slots)) slots[slot] = typeof initial === "function" ? initial() : initial;
      return [slots[slot], (next) => { slots[slot] = typeof next === "function" ? next(slots[slot]) : next; }]; },
    useEffect() {}, useId: () => "test-id"
  } : id === "../../i18n" ? { useI18n: () => ({ t: (key, params, fallback) => Object.entries(params ?? {}).reduce((text, [key, value]) => text.replaceAll(`{${key}}`, String(value)), catalog[key] ?? fallback ?? key), locale: "en-US" }) } : originalRequire(id);
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  return { props, render() { cursor = 0; return descendants(loaded.exports[exportName](props)); } };
}
function editor(name, exportName, key, raw) {
  const patches = [];
  const props = { settingKey: key, sectionId: "leveling", details: { summary: { id: "i", module_id: "arksurvivalascended" } },
    moduleDetails: { summary: { id: "arksurvivalascended" } }, disabled: false, settings: { [key]: raw, server_name: "Keep", mod_ids_csv: "321" },
    onPatch(patch) { patches.push(patch); props.settings = { ...props.settings, ...patch }; } };
  return { ...load(name, exportName, props), patches };
}

test("attribute edits preserve unrelated values and malformed numbers reach the save barrier", () => {
  const key = "per_level_stats_multiplier_dino_tamed_type_integer";
  const raw = '[0]=1\n_Add[0]=0.14\n_Affinity[0]=0.44\n[99]=7';
  const view = editor("ArkStatsEditor", "ArkStatsEditor", key, raw);
  const input = view.render().find((node) => node.type.name === "ArkCommitInput" && node.props.label === "Health Taming addition");
  input.props.onCommit("0.5");
  assert.equal(view.props.settings[key], raw.replace("0.14", "0.5"));
  assert.equal(view.props.settings.server_name, "Keep");
  view.render().find((node) => node.type.name === "ArkCommitInput" && node.props.label === "Health Per-level multiplier").props.onCommit("-");
  assert.ok(validateArkComplexField(key, view.props.settings[key]));
  const frame = load("ArkEditorFrame", "ArkEditorFrame", { ...view.props, children: "table" });
  const recovery = frame.render().find((node) => node.type === "textarea");
  assert.match(recovery.props.value, /\[0\]=-/);
  recovery.props.onChange({ target: { value: raw } });
  assert.equal(validateArkComplexField(key, view.props.settings[key]), undefined);
});

test("level UI appends without changing dino data and rejects CSV conflicts atomically", () => {
  const key = "level_experience_ramp_overrides";
  const raw = '(ExperiencePointsForLevel[0]=0,ExperiencePointsForLevel[1]=100,Future=(Value="keep"))\n(ExperiencePointsForLevel[0]=0,ExperiencePointsForLevel[1]=80)';
  const view = editor("ArkLevelsEditor", "ArkLevelsEditor", key, raw);
  const append = view.render().find((node) => node.type === "button" && node.props["aria-label"] === "Append levels · Player");
  assert.equal(append.props.children, "Append levels", "visible action does not repeat its curve heading");
  assert.equal(view.render().some((node) => node.type === "p" && String(node.props.children).includes("Append only.")), false);
  append.props.onClick();
  assert.match(view.props.settings[key], /ExperiencePointsForLevel\[2\]=200/);
  assert.ok(view.props.settings[key].endsWith(raw.split("\n")[1]));
  const before = view.props.settings[key];
  view.render().find((node) => node.type === "textarea").props.onChange({ target: { value: "curve,level,xp\nplayer,1,200" } });
  view.render().find((node) => node.type === "button" && node.props.children === "Append CSV rows").props.onClick();
  assert.equal(view.props.settings[key], before);
  assert.ok(view.render().some((node) => node.props.role === "alert"));
  view.render().find((node) => node.type.name === "ArkCommitInput" && node.props.label === "Player 1 Cumulative XP").props.onCommit("-9");
  assert.ok(validateArkComplexField(key, view.props.settings[key]));
});

test("tuple edits retain mod properties and invalid native numeric edits are not silently dropped", () => {
  const { parseArkRule } = require("../src/views/settings/ark-native-ast.ts");
  const text = '(DinoNameTag="Mod_Raptor",SpawnWeightMultiplier=1,Future=(Weight=-5,BaseResourceRequirement=-2))';
  let result;
  const props = { group: parseArkRule(text, "DinoSpawnWeightMultipliers"), text, path: "Rule 1", disabled: false, onChange(value) { result = value; } };
  const view = load("ArkRuleNodeEditor", "ArkRuleNodeEditor", props);
  view.render().find((node) => node.type.name === "ArkCommitInput" && node.props.label.endsWith("SpawnWeightMultiplier")).props.onCommit("-1");
  assert.equal(result, text.replace("Multiplier=1", "Multiplier=-1"));
  assert.ok(validateArkComplexField("dino_spawn_weight_multipliers", result));
  assert.match(result, /Future=\(Weight=-5,BaseResourceRequirement=-2\)/);
  for (const [catalog, expected] of [[ARK_EDITORS_EN, "Spawn weight multiplier"], [ARK_EDITORS_ZH_CN, "刷新权重倍率"]]) {
    const nodes = load("ArkRuleNodeEditor", "ArkRuleNodeEditor", props, catalog).render();
    assert.ok(nodes.some((node) => node.type === "span" && node.props.children === expected), "known property has a human label");
    assert.ok(nodes.some((node) => node.type === "code" && node.props.children === "SpawnWeightMultiplier"), "native key remains visible");
    const child = nodes.find((node) => node.type.name === "ArkRuleNodeEditor");
    const modNodes = load("ArkRuleNodeEditor", "ArkRuleNodeEditor", child.props, catalog).render();
    assert.ok(modNodes.some((node) => node.type === "label" && node.props.children === "Weight"), "unknown Mod containers retain native property names");
    assert.ok(modNodes.some((node) => node.type === "label" && node.props.children === "BaseResourceRequirement"), "a native-looking key in a Mod container is not translated");
    const summary = nodes.find((node) => node.type === "summary");
    const remove = descendants(summary).find((node) => node.type === "button");
    assert.ok(remove, "nested removal belongs to the group heading");
    let prevented = false;
    remove.props.onClick({ preventDefault() { prevented = true; }, stopPropagation() {} });
    assert.ok(prevented, "removing a group does not toggle its disclosure");
    assert.equal(result, '(DinoNameTag="Mod_Raptor",SpawnWeightMultiplier=1)');
  }
});

test("a failed commit keeps the typed draft, displays an error, and a retry can apply it", () => {
  let fail = true; const committed = [];
  const view = load("ArkEditorFrame", "ArkCommitInput", { value: "1", label: "Health", onCommit(value) { if (fail) throw new Error("source changed"); committed.push(value); } });
  view.render().find((node) => node.type === "input").props.onChange({ target: { value: "2.5" } });
  assert.doesNotThrow(() => view.render().find((node) => node.type === "input").props.onBlur());
  assert.equal(view.render().find((node) => node.type === "input").props.value, "2.5");
  assert.ok(view.render().some((node) => node.props.role === "alert"));
  assert.deepEqual(committed, []);
  view.props.value = "3";
  assert.equal(view.render().find((node) => node.type === "input").props.value, "2.5", "background refresh preserves the local draft");
  fail = false;
  view.render().find((node) => node.type === "input").props.onBlur();
  assert.deepEqual(committed, ["2.5"]);
});

test("native toggle changes no setting and restoring a valid raw value restores table access", () => {
  const key = "per_level_stats_multiplier_player_integer";
  const view = editor("ArkEditorFrame", "ArkEditorFrame", key, "[0]=1\r\n[99]=3");
  view.props.children = React.createElement("span", null, "structured");
  view.render().find((node) => node.type === "button").props.onClick();
  assert.equal(view.patches.length, 0);
  assert.equal(view.render().find((node) => node.type === "textarea").props.value, "[0]=1\r\n[99]=3");
  view.render().find((node) => node.type === "textarea").props.onChange({ target: { value: "[0]=NaN\r\n[99]=3" } });
  assert.ok(validateArkComplexField(key, view.props.settings[key]));
  view.render().find((node) => node.type === "textarea").props.onChange({ target: { value: "[0]=2\r\n[99]=3" } });
  view.render().find((node) => node.type === "button").props.onClick();
  assert.equal(view.render().some((node) => node.type === "textarea"), false);
  assert.equal(view.props.settings.mod_ids_csv, "321");
});
