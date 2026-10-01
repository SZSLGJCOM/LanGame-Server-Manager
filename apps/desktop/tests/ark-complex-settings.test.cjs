const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const ext of [".ts", ".tsx"]) require.extensions[ext] = (m, f) => m._compile(transpileTypeScript(fs.readFileSync(f, "utf8"), f), f);
require.extensions[".css"] = (m) => m._compile("module.exports = {};", m.filename);
for (const ext of [".png", ".webp", ".svg", ".jpg"]) require.extensions[ext] = (m) => m._compile(`module.exports = ${JSON.stringify(m.filename)};`, m.filename);

test("native AST retains untouched spelling, nested rules, unknown properties and quoted commas", () => {
  const { parseArkValue, replaceArkNode } = require("../src/views/settings/ark-native-ast.ts");
  const text = '( ItemClassString="Mod_Item_C", BaseCraftingResourceRequirements=((ResourceItemTypeString="A,B_C",BaseResourceRequirement=2.50)),FutureFlag=True )';
  const ast = parseArkValue(text);
  assert.equal(ast.kind, "group");
  const amount = ast.entries[1].value.entries[0].value.entries[1].value;
  assert.equal(replaceArkNode(text, amount, "4"), text.replace("2.50", "4"));
  assert.throws(() => parseArkValue('(A="unfinished)'), /quote/i);
  assert.throws(() => parseArkValue('(A=1,,B=2)'), /empty/i);
});

test("named stat edits preserve unknown indexes, tamed variants and unrelated native lines", () => {
  const { readArkStats, patchArkStat } = require("../src/views/settings/ark-stat-model.ts");
  const key = "per_level_stats_multiplier_dino_tamed_type_integer";
  const raw = 'PerLevelStatsMultiplier_DinoTamed[0]=0.2\r\n_Add[0]=0.14\r\n_Affinity[0]=0.44\r\n[99]=7';
  assert.equal(readArkStats(raw, key).entries.length, 4);
  assert.equal(patchArkStat(raw, key, 0, "_Add", "0.5"), raw.replace("0.14", "0.5"));
  assert.equal(patchArkStat(raw, key, 0, "", ""), '_Add[0]=0.14\r\n_Affinity[0]=0.44\r\n[99]=7');
});

test("level table edits and CSV append retain second curve and unknown properties", () => {
  const { readArkLevels, patchArkLevel, appendArkLevels, importArkLevelCsv, exportArkLevelCsv } = require("../src/views/settings/ark-level-model.ts");
  const raw = '(ExperiencePointsForLevel[0]=0,ExperiencePointsForLevel[1]=100,Future=42)\n(ExperiencePointsForLevel[0]=0,ExperiencePointsForLevel[1]=80)';
  assert.equal(readArkLevels(raw).curves[0].levels[1].xp, 100);
  assert.equal(patchArkLevel(raw, 0, 1, "150"), raw.replace("=100", "=150"));
  const added = appendArkLevels(raw, 0, 2, 50);
  assert.match(added, /Future=42,ExperiencePointsForLevel\[2\]=150,ExperiencePointsForLevel\[3\]=200/);
  assert.ok(added.endsWith(raw.split('\n')[1]));
  const imported = importArkLevelCsv(raw, 'curve,level,xp\nplayer,2,200\n', "append");
  assert.match(imported, /ExperiencePointsForLevel\[2\]=200/);
  assert.throws(() => importArkLevelCsv(raw, 'curve,level,xp\nplayer,1,200\n', "append"), /already|exists/i);
  assert.match(exportArkLevelCsv(raw), /dino,1,80/);
});

test("complex validation rejects malformed rules, invalid stats and descending XP but accepts mod extensions", () => {
  const { validateArkComplexField } = require("../src/views/settings/ark-complex-validation.ts");
  for (const [key, value] of [
    ['npc_replacements', 'not a native rule ('],
    ['config_override_item_crafting_costs', '(ItemClassString="A_C",BaseCraftingResourceRequirements=((BaseResourceRequirement=-2)))'],
    ['per_level_stats_multiplier_player_integer', '[0]=NaN'],
    ['per_level_stats_multiplier_player_integer', '[-1]=2'],
    ['level_experience_ramp_overrides', '(ExperiencePointsForLevel[0]=100,ExperiencePointsForLevel[1]=50)'],
    ['level_experience_ramp_overrides', '(FixtureIndex=1)'],
    ['level_experience_ramp_overrides', '()'],
    ['override_player_level_engram_points', '1\n-2']
  ]) assert.ok(validateArkComplexField(key, value), `${key}: ${value}`);
  assert.equal(validateArkComplexField('npc_replacements', '(FromClassName="Mod_A_C",ToClassName="",Future=(Unknown="a,b"))'), undefined);
  assert.equal(validateArkComplexField('per_level_stats_multiplier_player_integer', '[99]=2'), undefined);
  assert.equal(validateArkComplexField('npc_replacements', '(FromClassName="Mod_A_C",ToClassName="",Future=(Weight=-5,Future=(EntryWeight=-2)))'), undefined);
  assert.ok(validateArkComplexField('config_override_supply_crate_items', '(ItemSets=((Weight=-5)))'));
});

test("every rule template is valid and each game's registered addon owns exactly one real focus control", () => {
  const React = require("react");
  const { renderToStaticMarkup } = require("react-dom/server");
  const { I18nContext } = require("../src/i18n-context.ts");
  const { parseGuidedSettingsSchema } = require("../src/views/settings/guided-settings.ts");
  const { resolveSettingsModuleDefinition } = require("../src/views/settings/module-registry.ts");
  const { ARK_RULE_FIELDS } = require("../src/views/settings/ark-rule-fields.ts");
  const { validateArkComplexField } = require("../src/views/settings/ark-complex-validation.ts");
  const { ARK_EDITORS_ZH_CN } = require("../src/i18n/games/ark-editors.zh-cn.ts");
  const t = (key, params, fallback) => Object.entries(params ?? {}).reduce((text, [key, value]) => text.replaceAll(`{${key}}`, String(value)), ARK_EDITORS_ZH_CN[key] ?? fallback ?? key);
  for (const [key, spec] of Object.entries(ARK_RULE_FIELDS)) assert.equal(validateArkComplexField(key, spec.sample), undefined, key);
  for (const id of ["arksurvivalascended", "arksurvivalevolved"]) {
    const schema = JSON.parse(fs.readFileSync(path.resolve(__dirname, `../../../modules/${id}/schema.json`), "utf8"));
    const moduleDetails = { summary: { id, name: id }, schema_json: JSON.stringify(schema) };
    const parsed = parseGuidedSettingsSchema(moduleDetails, "zh-CN", t);
    const definition = resolveSettingsModuleDefinition(id);
    for (const field of parsed.fields.filter((field) => field.presentation.rendererId?.startsWith("ark-structured-"))) {
      const registration = definition.specializedRenderers[field.presentation.rendererId];
      assert.equal(registration.fieldKey, undefined, "outer wrapper must not duplicate the actual input ID");
      for (const value of ["", ARK_RULE_FIELDS[field.key]?.sample ?? (field.key === "level_experience_ramp_overrides" ? '(ExperiencePointsForLevel[0]=0)' : field.key === "override_player_level_engram_points" ? "0\n8" : "[0]=1")]) {
        const html = renderToStaticMarkup(React.createElement(I18nContext.Provider, { value: { locale: "zh-CN", setLocale() {}, t } },
          React.createElement(registration.Renderer, { sectionId: field.sectionId, moduleDetails, details: { summary: { id: "test", module_id: id } }, settings: { [field.key]: value }, disabled: false, onPatch() {} })));
        const target = `configuration-${id}-${field.key.replaceAll("_", "-")}-input`;
        const ids = [...html.matchAll(/\bid="([^"]+)"/g)].map((match) => match[1]);
        assert.equal(ids.length, new Set(ids).size, field.key);
        assert.equal(ids.filter((entry) => entry === target).length, 1, field.key);
        assert.match(html, new RegExp(`<(?:input|button|select)[^>]*id="${target}"`), field.key);
        assert.doesNotMatch(html, /无法显示为表格/, field.key);
      }
    }
  }
});

test("full workspace excludes the generic copy of each structured ARK field", () => {
  const Module = require("node:module");
  const React = require("react");
  const { renderToStaticMarkup } = require("react-dom/server");
  const { I18nContext } = require("../src/i18n-context.ts");
  const { resolveSettingsModuleDefinition } = require("../src/views/settings/module-registry.ts");
  const t = (key, _params, fallback) => fallback ?? key;
  function findNavigation(element) {
    if (!React.isValidElement(element)) return null;
    if (element.type.name === "ConfigurationSearchNavigation") return element;
    for (const child of React.Children.toArray(element.props.children)) {
      const found = findNavigation(child);
      if (found) return found;
    }
    return null;
  }
  for (const id of ["arksurvivalascended", "arksurvivalevolved"]) {
    const schema = JSON.parse(fs.readFileSync(path.resolve(__dirname, `../../../modules/${id}/schema.json`), "utf8"));
    const definition = resolveSettingsModuleDefinition(id);
    for (const section of ["leveling", "experience", "spawns", "engrams", "loot", "crafting"]) {
      const hooks = [];
      let cursor = 0;
      function useState(initial) {
        const slot = cursor++;
        if (!(slot in hooks)) hooks[slot] = typeof initial === "function" ? initial() : initial;
        return [hooks[slot], (next) => { hooks[slot] = typeof next === "function" ? next(hooks[slot]) : next; }];
      }
      const filename = path.resolve(__dirname, "../src/views/settings/ConfigurationWorkspace.tsx");
      const loaded = new Module(filename, module); loaded.filename = filename;
      const originalRequire = Module.createRequire(filename);
      loaded.require = (key) => key === "react" ? {
        ...React, useState, useMemo: (compute) => compute(), useLayoutEffect() {},
        useRef: (initial) => useState(() => ({ current: initial }))[0]
      }
        : key === "../../i18n" ? { ...originalRequire(key), useI18n: () => ({ locale: "en-US", t }) }
        : key === "./useAutoSaveInstanceSettings" ? { useAutoSaveInstanceSettings: () => ({ status: { state: "saved" }, retry() {} }) }
        : key === "./useInstancePortRegistration" ? { useInstancePortRegistration: () => ({ ports: [], defaultPorts: [], setPorts() {} }) }
        : key === "./useModuleConfigurationIcons" ? { useModuleConfigurationIcons: () => ({ icons: {} }) } : originalRequire(key);
      loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
      const settings = Object.fromEntries(Object.entries(schema.properties).filter(([, p]) => p.default !== undefined).map(([key, p]) => [key, p.default]));
      const props = { details: { summary: { id: "ark-test", module_id: id, name: "ARK", status: "stopped", bind_ip: "127.0.0.1" }, settings_json: JSON.stringify(settings), auto_backup_on_stop: false, backup_retention_count: 3 },
        moduleDetails: { summary: { id, name: id }, schema_json: JSON.stringify(schema) }, bindAddressCandidates: [], runtime: null, launchPlan: null, launchPlanError: null, onSave() {} };
      const navigation = findNavigation(loaded.exports.ConfigurationWorkspace(props));
      assert.ok(navigation, `${id} exposes section navigation`);
      navigation.props.onSelectSection(section);
      cursor = 0;
      const workspace = loaded.exports.ConfigurationWorkspace(props);
      assert.equal(findNavigation(workspace).props.selectedSectionId, section);
      const html = renderToStaticMarkup(React.createElement(I18nContext.Provider, { value: { locale: "en-US", setLocale() {}, t } }, workspace));
      for (const [key, presentation] of Object.entries(definition.fieldPresentationOverrides)) {
        if (!presentation.rendererId?.startsWith("ark-structured-") || presentation.sectionId !== section) continue;
        assert.equal([...html.matchAll(new RegExp(`data-field-key="${key}"`, "g"))].length, 1, `${id}.${section}.${key}`);
      }
      const ids = [...html.matchAll(/\bid="([^"]+)"/g)].map((match) => match[1]);
      assert.equal(ids.length, new Set(ids).size, `${id}.${section}`);
    }
  }
});

test("append operations cannot silently manufacture a missing player curve or overwrite CSV levels", () => {
  const { appendArkLevels, importArkLevelCsv } = require("../src/views/settings/ark-level-model.ts");
  assert.throws(() => appendArkLevels("", 1, 1, 100), /player curve/);
  assert.throws(() => importArkLevelCsv("", "curve,level,xp\ndino,0,100", "append"), /player curve/);
  assert.throws(() => appendArkLevels("", 0, 501, 100), /1–500/);
});
