const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const {
  parseSource,
  transpileTypeScript,
  visitSyntax
} = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");
const repoRoot = path.resolve(desktopRoot, "../..");
for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    module._compile(transpileTypeScript(source, filename), filename);
  };
}

// Register styles after source extensions so extensionless imports resolve TypeScript first.
require.extensions[".css"] = (module) => module._compile("", module.filename);

const {
  buildScumJsonMessageCatalog,
  buildScumNativeMessageCatalog,
  scumNativeMessageKey
} = require("../src/i18n/games/scum-native-messages.ts");
const { SCUM_NATIVE_ZH_TITLES } = require("../src/i18n/games/scum-native-zh-titles.ts");
const { EN_US_SCUM_MESSAGES } = require("../src/i18n/games/scum.en.ts");
const { ZH_CN_SCUM_MESSAGES } = require("../src/i18n/games/scum.zh-cn.ts");
const {
  buildScumNativePresentationFields,
  initializeScumSettings,
  patchScumNativeValue,
  SCUM_EDITABLE_SETTINGS,
  SCUM_SECTION_FIELDS,
  validateScumStructuredSettings
} = require("../src/views/settings/scum-server-settings-inventory.ts");
const { validateScumJsonSettings } = require("../src/views/settings/ScumJsonSettingsRenderer.tsx");
const { resolveSettingsModuleDefinition } = require("../src/views/settings/module-registry.ts");
const scumSettingsDefinition = resolveSettingsModuleDefinition("scum");

function translate(catalog) {
  return (key, params, fallback) => {
    let message = catalog[key] ?? fallback ?? key;
    for (const [name, value] of Object.entries(params ?? {})) {
      message = message.replaceAll(`{${name}}`, String(value));
    }
    return message;
  };
}

const enT = translate(EN_US_SCUM_MESSAGES);
const zhT = translate(ZH_CN_SCUM_MESSAGES);

function hasHan(value) {
  return /[\u3400-\u9fff]/u.test(value);
}

const MOJIBAKE_PATTERNS = [
  /[\u0080-\u009f\u20ac\ue000-\uf8ff\ufffd]/u,
  /(?:鍩哄|寤洪|鏈€|璁剧|鍚|鏁伴|鐢ㄦ|鎴峰|閰嶇)/u,
  /(?:[\u3400-\u9fff]\?|\?[\u3400-\u9fff])/u
];

function assertNoMojibake(value, label) {
  for (const pattern of MOJIBAKE_PATTERNS) {
    assert.doesNotMatch(value, pattern, `${label} contains probable encoding damage`);
  }
}

test("SCUM zh-CN title inventory is explicit, complete, unique, and encoding-safe", () => {
  const specializedKeys = SCUM_EDITABLE_SETTINGS.map((setting) => setting.key).sort();
  const translatedKeys = Object.keys(SCUM_NATIVE_ZH_TITLES).sort();
  assert.deepEqual(translatedKeys, specializedKeys);

  const titleSourcePath = path.join(desktopRoot, "src", "i18n", "games", "scum-native-zh-titles.ts");
  const titleSource = fs.readFileSync(titleSourcePath, "utf8");
  assertNoMojibake(titleSource, titleSourcePath);

  const sourceFile = parseSource(titleSource, titleSourcePath);
  let titleMapLiteral;
  visitSyntax(sourceFile, (node) => {
    if (
      node.type === "VariableDeclarator"
      && node.id.type === "Identifier"
      && node.id.value === "SCUM_NATIVE_ZH_TITLES"
      && node.init?.type === "ObjectExpression"
    ) {
      titleMapLiteral = node.init;
      return false;
    }
  });
  assert.ok(titleMapLiteral, "SCUM_NATIVE_ZH_TITLES should remain an auditable object literal");
  const sourceKeys = titleMapLiteral.properties.map((property) => {
    assert.equal(property.type, "KeyValueProperty", "title map should only contain property assignments");
    return property.key.value;
  });
  assert.equal(sourceKeys.length, specializedKeys.length);
  assert.equal(new Set(sourceKeys).size, sourceKeys.length, "title map must not contain duplicate keys");

  const allowedLatinTerms = new Set([
    "AI", "BCU", "Cruiser", "DEENA", "Duster", "HTZ", "ID", "Kinglet", "LTZ", "Laika",
    "Mariner", "MTZ", "NPC", "POI", "PlaySafe", "RIS", "RT", "Rate", "Rager", "SCUM",
    "SUP", "Tick", "URL", "Wolfswagen", "X", "Y"
  ]);
  for (const [key, title] of Object.entries(SCUM_NATIVE_ZH_TITLES)) {
    assert.ok(hasHan(title), `${key} must contain a real Chinese title`);
    assertNoMojibake(title, key);
    for (const term of title.match(/[A-Za-z]+/gu) ?? []) {
      assert.ok(allowedLatinTerms.has(term), `${key} leaks unexpected English token: ${term}`);
    }
  }

  assert.equal(SCUM_NATIVE_ZH_TITLES.allow_skill_gain_in_safe_zones, "允许在安全区提升技能");
  assert.equal(SCUM_NATIVE_ZH_TITLES.abandoned_bunker_reset_armory_lockers_on_activation_only, "仅在激活时重置废弃地堡军械柜");
  assert.equal(SCUM_NATIVE_ZH_TITLES.encounter_can_clamp_character_num_when_out_of_resources, "资源不足时限制遭遇角色数量");
  assert.equal(SCUM_NATIVE_ZH_TITLES.smoker_start_decaying_if_flag_area_has_more_than, "旗帜区域内熏肉架超过此数后开始衰减");
  assert.equal(SCUM_NATIVE_ZH_TITLES.stamina_drain_on_jump_multiplier, "跳跃耐力消耗倍率");
  assert.equal(SCUM_NATIVE_ZH_TITLES.maximum_time_for_vehicles_in_forbidden_zones, "禁区内载具最长保留时间");
});

test("all 436 SCUM native controls keep bilingual titles and source mappings without template help", () => {
  const english = buildScumNativeMessageCatalog("en-US");
  const chinese = buildScumNativeMessageCatalog("zh-CN");

  assert.equal(SCUM_EDITABLE_SETTINGS.length, 436);
  assert.deepEqual(Object.keys(english).sort(), Object.keys(chinese).sort());
  assert.equal(Object.keys(english).filter((key) => key.endsWith(".title")).length, 436);

  for (const setting of SCUM_EDITABLE_SETTINGS) {
    const titleKey = scumNativeMessageKey(setting.key, "title");
    const descriptionKey = scumNativeMessageKey(setting.key, "description");
    const zhTitle = chinese[titleKey];
    const zhDescription = chinese[descriptionKey];

    assert.equal(english[titleKey], setting.title, `${titleKey} should preserve the natural English inventory title`);
    if (english[descriptionKey]) {
      assert.doesNotMatch(english[descriptionKey], /Configures .*native .*server parameter/u);
      assert.ok(hasHan(zhDescription), `${descriptionKey} must have matching Chinese help`);
    }
    assert.ok(hasHan(zhTitle), `${titleKey} should not fall back to English`);
    assertNoMojibake(zhTitle, titleKey);
    if (zhDescription) assertNoMojibake(zhDescription, descriptionKey);
    assert.doesNotMatch(zhTitle, /配置项/u, `${titleKey} should not use a generic placeholder`);
    assert.doesNotMatch(zhDescription ?? "", /保存后写入 SCUM 原生参数/u);
  }

  const localizedPresentation = buildScumNativePresentationFields(zhT)
    .filter((field) => ["specialized", "editable"].includes(field.presentation.state));
  assert.equal(localizedPresentation.length, 436);
  for (const field of localizedPresentation) {
    assert.ok(hasHan(field.title), `${field.key} presentation title should be localized`);
    if (field.description) assert.ok(hasHan(field.description), `${field.key} presentation help should be localized`);
    const setting = SCUM_EDITABLE_SETTINGS.find((item) => field.key.endsWith(`.${item.key}`));
    assert.equal(field.sourceKey, setting.nativeKey);
  }

  const generatedPresentation = buildScumNativePresentationFields(zhT)
    .filter((field) => field.presentation.state === "generated");
  assert.equal(generatedPresentation.length, 1);
  assert.equal(generatedPresentation[0].title, "服务器设置版本");
  assert.equal(generatedPresentation[0].description, "此配置版本标记由 SCUM 生成并维护。");
  assert.equal(generatedPresentation[0].presentation.reason, "SCUM 负责写入和迁移 ServerSettings 版本标记。");
});

test("SCUM JSON editor fields and every operation or warning have bilingual catalog copy", () => {
  const englishJson = buildScumJsonMessageCatalog("en-US");
  const chineseJson = buildScumJsonMessageCatalog("zh-CN");
  assert.equal(Object.keys(englishJson).length, Object.keys(chineseJson).length);
  assert.equal(Object.keys(chineseJson).filter((key) => key.endsWith(".title")).length, 37);
  for (const [key, value] of Object.entries(chineseJson)) {
    assert.ok(hasHan(value), `${key} should be localized in zh-CN`);
    assert.ok(englishJson[key], `${key} should have an en-US counterpart`);
  }

  const copyKeys = [
    ...[
      "identity", "communication", "performance", "retention-logging", "gameplay-access",
      "maintenance-risk", "wildlife", "encounters", "time-weather", "cargo-drops", "bunkers",
      "world-rules", "building-raiding", "resources", "items", "squads", "skills", "quests",
      "diagnostics", "survival-features", "prices", "cooldowns", "spawn-rules", "energy",
      "lifecycle", "fleet-limits", "pvp", "decay", "npc-structures"
    ].map((id) => `scum.settings.groups.${id}`),
    "scum.settings.groups.player-access",
    "scum.settings.groups.player-accessDescription",
    "scum.settings.groups.launch-overrides",
    "scum.settings.groups.launch-overridesDescription",
    ...["partial_wipe", "gold_wipe", "full_wipe", "master_server_is_local_test"]
      .map((id) => `scum.settings.risk.${id}`),
    ...[
      "settingCount", "filterPlaceholder", "stopRiskChanges", "confirmEnable", "cancel"
    ].map((id) => `scum.settings.renderer.${id}`),
    ...[
      "economyTitle", "globalEconomy", "traderOverrides", "globalHelp", "traderHelp", "tradeableHelp",
      "raidHelp", "notificationHelp", "row", "rowCount", "remove", "addTradeableRow", "addRow",
      "raidTitle", "notificationsTitle"
    ].map((id) => `scum.settings.json.${id}`),
    ...[
      "adminSteamIds", "nativeType", "minimum", "maximum", "singleLine", "economyObject",
      "tradersObject", "structuredList", "missingNativeField", "raidMaximum"
    ].map((id) => `scum.settings.validation.${id}`)
  ];

  for (const key of copyKeys) {
    assert.ok(EN_US_SCUM_MESSAGES[key], `${key} should exist in en-US`);
    assert.ok(hasHan(ZH_CN_SCUM_MESSAGES[key] ?? ""), `${key} should exist in zh-CN without English fallback`);
  }

  const serverRenderer = fs.readFileSync(path.join(
    desktopRoot, "src", "views", "settings", "ScumServerSettingsRenderer.tsx"
  ), "utf8");
  const jsonRenderer = fs.readFileSync(path.join(
    desktopRoot, "src", "views", "settings", "ScumJsonSettingsRenderer.tsx"
  ), "utf8");
  assert.doesNotMatch(serverRenderer, />\s*(?:Confirm enable|Cancel)\s*</u);
  assert.doesNotMatch(jsonRenderer, />\s*(?:Remove|Add row|Add tradeable row)\s*</u);
});

test("SCUM official timing and economy copy distinguishes units, sentinels, and scopes", () => {
  const native = buildScumNativeMessageCatalog("en-US");
  const json = buildScumJsonMessageCatalog("en-US");
  const zh = buildScumJsonMessageCatalog("zh-CN");
  assert.match(native[scumNativeMessageKey("raid_protection_type", "description")], /3.*global.*empty.*no protection/iu);
  assert.match(native[scumNativeMessageKey("raid_protection_global_should_show_raid_times_message", "description")], /welcome.*message of the day/iu);
  assert.match(json["scum.settings.json.fields.start-announcement-time.description"], /Minutes.*before.*0.*disables/iu);
  assert.match(json["scum.settings.json.fields.end-announcement-time.description"], /Minutes.*before.*0.*disables/iu);
  assert.equal(zh["scum.settings.json.fields.duration.title"], "显示时长（秒）");
  assert.equal(zh["scum.settings.json.fields.wait.title"], "重复间隔（分钟）");
  assert.match(json["scum.settings.json.fields.delta-price.description"], /-1.*random.*0.*fix/iu);
  assert.match(json["scum.settings.json.fields.can-be-purchased.description"], /default.*true.*false/iu);
  assert.match(native[scumNativeMessageKey("item_virtualization_visitor_bounds", "title")], /\(cm\)/u);
  assert.match(native[scumNativeMessageKey("item_virtualization_visitor_bounds", "description")], /around a player.*restoring virtualized items/iu);
  for (const [key, value] of Object.entries(json)) {
    if (key.endsWith(".description")) assert.doesNotMatch(value, /^Sets the .*field\./u);
  }
});

test("SCUM native and JSON validation errors resolve through zh-CN", () => {
  const initialized = initializeScumSettings({});
  const invalidNative = { ...initialized };
  for (const fieldKey of Object.values(SCUM_SECTION_FIELDS)) {
    invalidNative[fieldKey] = Object.fromEntries(
      Object.keys(invalidNative[fieldKey]).map((key) => [key, {}])
    );
  }

  const nativeTypeIssues = validateScumStructuredSettings(invalidNative, zhT);
  assert.equal(nativeTypeIssues.length, 436);
  for (const issue of nativeTypeIssues) {
    assert.ok(hasHan(issue.message), `${issue.fieldKey} validation should be localized`);
    assert.doesNotMatch(issue.message, /must be/u);
  }

  const lowerBound = validateScumStructuredSettings({
    ...initialized,
    server_general: { ...initialized.server_general, max_players: 0 }
  }, zhT).find((issue) => issue.fieldKey.endsWith(".max_players"));
  assert.ok(lowerBound && hasHan(lowerBound.message));

  const singleLine = validateScumStructuredSettings({
    ...initialized,
    server_general: { ...initialized.server_general, server_name: "line one\nline two" }
  }, zhT).find((issue) => issue.fieldKey.endsWith(".server_name"));
  assert.ok(singleLine && hasHan(singleLine.message));

  const jsonShapeIssues = validateScumJsonSettings({
    economy_override: null,
    raid_times: {},
    notifications: [{}]
  }, zhT);
  assert.deepEqual(
    new Set(jsonShapeIssues.map((issue) => issue.reason)),
    new Set(["type", "row"])
  );
  for (const issue of jsonShapeIssues) assert.ok(hasHan(issue.message));

  const validRaidRow = initialized.raid_times[0];
  const jsonLimitIssues = validateScumJsonSettings({
    economy_override: { "economy-reset-time-hours": "24.0" },
    raid_times: Array.from({ length: 51 }, () => ({ ...validRaidRow })),
    notifications: []
  }, zhT);
  assert.ok(jsonLimitIssues.some((issue) => issue.reason === "traders" && hasHan(issue.message)));
  assert.ok(jsonLimitIssues.some((issue) => issue.reason === "maximum" && hasHan(issue.message)));
});

test("SCUM schema defaults remain valid after a keycard edit and settings readback", () => {
  const schema = JSON.parse(fs.readFileSync(path.join(repoRoot, "modules", "scum", "schema.json"), "utf8"));
  const defaults = Object.fromEntries(Object.entries(schema.properties)
    .filter(([, field]) => Object.hasOwn(field, "default"))
    .map(([key, field]) => [key, structuredClone(field.default)]));
  const context = { locale: "zh-CN", t: zhT };
  const initialized = scumSettingsDefinition.initializeSettings(defaults, context);
  assert.deepEqual(initialized.economy_override, {}, "empty override selects native economy defaults");
  assert.deepEqual(scumSettingsDefinition.getSettingsValidationIssues(initialized, context), []);

  const keycard = SCUM_EDITABLE_SETTINGS.find((setting) => setting.key === "max_allowed_apex_facility_keycards");
  assert.ok(keycard);
  const edited = { ...initialized, ...patchScumNativeValue(initialized, keycard, 8) };
  const readBack = scumSettingsDefinition.initializeSettings(JSON.parse(JSON.stringify(edited)), context);
  assert.equal(readBack.server_world.max_allowed_apex_facility_keycards, 8);
  assert.deepEqual(readBack.economy_override, {}, "editing another section must not manufacture trader data");
  assert.deepEqual(scumSettingsDefinition.getSettingsValidationIssues(readBack, context), []);
});

test("SCUM economy validation still rejects malformed overrides and preserves unknown native values", () => {
  const defaults = { economy_override: {}, raid_times: [], notifications: [] };
  for (const economy of [null, [], "", 0]) {
    assert.deepEqual(validateScumJsonSettings({ ...defaults, economy_override: economy }).map((issue) => issue.reason), ["type"]);
  }
  for (const economy of [
    { "economy-reset-time-hours": "24.0" },
    { traders: null },
    { traders: [] },
    { traders: "" }
  ]) {
    assert.deepEqual(validateScumJsonSettings({ ...defaults, economy_override: economy }).map((issue) => issue.reason), ["traders"]);
  }

  const economy = {
    "future-native-global": { enabled: false, weight: "01.0" },
    traders: { Future_Trader: [{ "tradeable-code": "First", "future-native-column": [0, false, ""] }] }
  };
  const original = structuredClone(economy);
  const initialized = initializeScumSettings({ ...defaults, economy_override: economy });
  assert.deepEqual(validateScumJsonSettings(initialized), []);
  assert.deepEqual(initialized.economy_override, original);
  assert.deepEqual(economy, original, "validation and initialization must not rewrite input");
});
