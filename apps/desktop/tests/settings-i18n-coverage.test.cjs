const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const root = path.resolve(__dirname, "..", "..", "..");
const desktopRoot = path.join(root, "apps", "desktop");
const modulesDir = path.join(root, "modules");

function registerTypeScriptRequireExtension(extension) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    module._compile(transpileTypeScript(source, filename), filename);
  };
}

registerTypeScriptRequireExtension(".ts");
registerTypeScriptRequireExtension(".tsx");
require.extensions[".css"] = function compileEmptyCss(module, filename) {
  module._compile("", filename);
};
require.extensions[".png"] = function compileImageAsset(module, filename) {
  module._compile(`module.exports = ${JSON.stringify(filename)};`, filename);
};

const { resolveSettingsModuleDefinition } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "module-registry.ts"
));
const { parseGuidedSettingsSchema } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "guided-settings.ts"
));
const { buildConfigurationWorkspaceModel, resolveConfigurationSectionId } = require(path.join(
  desktopRoot, "src", "views", "settings", "configuration-workspace-model.ts"
));
const { getLocalizedModuleDisplayName } = require(path.join(desktopRoot, "src", "store-media.ts"));
const { EN_US_MESSAGES } = require(path.join(desktopRoot, "src", "i18n-messages.ts"));
const { ZH_CN_MESSAGES } = require(path.join(desktopRoot, "src", "i18n-messages-zh-cn.ts"));

function readRealModuleIds() {
  return fs.readdirSync(modulesDir)
    .filter((moduleId) => fs.existsSync(path.join(modulesDir, moduleId, "schema.json")))
    .sort();
}

function readModuleManifestName(moduleId) {
  const source = fs.readFileSync(path.join(modulesDir, moduleId, "module.toml"), "utf8");
  const match = source.match(/^name\s*=\s*"([^"]+)"/m);
  assert.ok(match, `${moduleId} manifest name is missing`);
  return match[1];
}

function moduleDetailsFor(moduleId) {
  return {
    summary: {
      id: moduleId,
      name: readModuleManifestName(moduleId),
      version: "0.0.0",
      description: null,
      steam_app_id: null,
      install_state: "NotInstalled",
      supported_platforms: ["windows"]
    },
    default_ports: [],
    runtime: {},
    schema_json: fs.readFileSync(path.join(modulesDir, moduleId, "schema.json"), "utf8")
  };
}

function makeCatalogTranslator(catalog) {
  return (key, params, fallback) => {
    const template = catalog[key] ?? fallback ?? key;
    return String(template).replace(/\{\s*([\w.]+)\s*\}/g, (match, paramKey) => {
      const value = params?.[paramKey];
      return value == null ? match : String(value);
    });
  };
}

test("Dragonwilds optional ownership instructions survive field projection and tooltip summarization", () => {
  const { summarizeConfigurationFieldHelp } = require(path.join(
    desktopRoot, "src", "views", "settings", "ConfigurationFieldHelp.tsx"
  ));
  for (const [locale, catalog, noOwner, permissions] of [
    ["zh-CN", ZH_CN_MESSAGES, "留空可开服", "服主专属权限"],
    ["en-US", EN_US_MESSAGES, "without an assigned owner", "owner permissions"]
  ]) {
    const t = makeCatalogTranslator(catalog);
    const parsed = parseGuidedSettingsSchema(moduleDetailsFor("runescapedragonwilds"), locale, t);
    const owner = parsed.fields.find((field) => field.key === "owner_id");
    assert.ok(owner);
    assert.equal(owner.required, false);
    const help = summarizeConfigurationFieldHelp(owner.description, owner.title, t);
    assert.ok(help?.includes(noOwner), `${locale} must explain empty ownership`);
    assert.ok(help?.includes(permissions), `${locale} must explain the permission boundary`);
    assert.ok(help?.includes("Player ID") && help.includes("Steam64"));
  }
});

function fieldControlForSchemaProperty(property) {
  if (Array.isArray(property.enum)) {
    return "select";
  }
  if (property.format === "textarea") {
    return "textarea";
  }
  if (property.type === "boolean") {
    return "checkbox";
  }
  if (property.type === "integer" || property.type === "number") {
    return "number";
  }
  return "text";
}

function fieldsForModule(moduleId) {
  const schema = JSON.parse(fs.readFileSync(path.join(modulesDir, moduleId, "schema.json"), "utf8"));
  return Object.entries(schema.properties ?? {}).map(([key, property]) => ({
    key,
    title: typeof property.title === "string" ? property.title : key,
    description: typeof property.description === "string" ? property.description : "",
    type:
      property.type === "integer"
        ? "integer"
        : property.type === "number"
          ? "number"
          : property.type === "boolean"
            ? "boolean"
            : "string",
    control: fieldControlForSchemaProperty(property),
    sectionId: typeof property["x-lsgm-section"] === "string" ? property["x-lsgm-section"] : "advanced",
    required: false,
    enumOptions: Array.isArray(property.enum)
      ? property.enum.map((value) => ({ label: String(value), value }))
      : undefined
  }));
}

function collectSettingsTranslationKeys() {
  const requestedKeys = new Set();
  const t = (key, _params, fallback) => {
    if (typeof key === "string" && key.includes(".")) {
      requestedKeys.add(key);
    }
    return typeof fallback === "string" ? fallback : key;
  };

  for (const moduleId of readRealModuleIds()) {
    const definition = resolveSettingsModuleDefinition(moduleId);
    assert.ok(definition, `${moduleId} should have a settings module definition`);

    const sections = definition.getSections ? definition.getSections(t, "zh-CN") : (definition.sections ?? []);
    const fields = fieldsForModule(moduleId);

    for (const section of sections) {
      const sectionFields = fields.filter((field) => field.sectionId === section.id);
      definition.buildFieldGroups?.(section.id, sectionFields, "zh-CN", t);

      for (const field of sectionFields) {
        definition.getFieldCopy?.(field.key, t, "zh-CN");

        for (const option of field.enumOptions ?? []) {
          definition.getEnumOptionLabel?.(field.key, option.value, "zh-CN", t);
        }
      }
    }
  }

  return requestedKeys;
}

test("settings pages do not fall back to English for dynamic game config copy", () => {
  const requestedKeys = collectSettingsTranslationKeys();
  const enKeys = new Set(Object.keys(EN_US_MESSAGES));
  const zhKeys = new Set(Object.keys(ZH_CN_MESSAGES));
  const missing = [...requestedKeys]
    .filter((key) => !enKeys.has(key) || !zhKeys.has(key))
    .sort()
    .map((key) => `${key} <- en:${enKeys.has(key)} zh:${zhKeys.has(key)}`);

  assert.deepEqual(missing, []);
});

test("Configuration workspace headings use localized zh-CN game names for every game", () => {
  const untranslated = readRealModuleIds()
    .map((moduleId) => {
      const details = moduleDetailsFor(moduleId);
      const localized = getLocalizedModuleDisplayName(moduleId, "zh-CN", details.summary.name);
      return `${moduleId}: ${localized}`;
    })
    .filter((entry) => !/^[a-z0-9]+:\s[\u3400-\u9fff]/u.test(entry));

  assert.deepEqual(untranslated, []);
});

test("Minecraft identity uses the shared room section and remote services use network", () => {
  const minecraftSchema = parseGuidedSettingsSchema(
    moduleDetailsFor("minecraft"),
    "en-US",
    makeCatalogTranslator(EN_US_MESSAGES)
  );
  const minecraftFields = new Map(minecraftSchema.fields.map((field) => [field.key, field.sectionId]));

  assert.equal(minecraftFields.get("motd"), "room");
  assert.equal(minecraftFields.get("max_players"), "room");
  assert.equal(minecraftFields.get("network_compression_threshold"), "network");
  assert.equal(minecraftFields.get("use_native_transport"), "network");
  assert.ok(minecraftSchema.sections.some((section) => section.id === "network"));
});

test("English configuration labels remain English across every bundled game", () => {
  const translated = [];
  for (const moduleId of readRealModuleIds()) {
    const schema = parseGuidedSettingsSchema(moduleDetailsFor(moduleId), "en-US", makeCatalogTranslator(EN_US_MESSAGES));
    assert.equal(schema.parseError, null, moduleId);
    for (const field of schema.presentationFields) {
      if (/[\u3400-\u9fff]/u.test(field.title)) translated.push(`${moduleId}.${field.key}: ${field.title}`);
    }
  }
  assert.deepEqual(translated, []);
  const minecraft = parseGuidedSettingsSchema(moduleDetailsFor("minecraft"), "en-US", makeCatalogTranslator(EN_US_MESSAGES));
  assert.equal(minecraft.fields.find((field) => field.key === "max_players").title, "Max Players");
});

test("non-DST enum labels preserve short Chinese and numeric choices while placeholder titles still fall back", () => {
  const choices = ["short", "long", "none", "slow", "zero", "negative", "decimal"];
  const labels = ["短", "长", "无", "慢", "0", "-1", "0.5"];
  const catalog = {
    "settings.schema.minecraft.rule_duration.title": "配置项",
    "settings.schema.minecraft.short_title.title": "短",
    "settings.schema.minecraft.numeric_title.title": "0",
    ...Object.fromEntries(choices.map((value, index) => [
      `settings.schema.minecraft.rule_duration.option.${value}`, labels[index]
    ]))
  };
  const parsed = parseGuidedSettingsSchema({
    summary: { id: "minecraft", name: "Minecraft" },
    schema_json: JSON.stringify({
      type: "object",
      properties: {
        rule_duration: { type: "string", title: "规则时长", enum: choices },
        short_title: { type: "string", title: "规则频率" },
        numeric_title: { type: "integer", title: "规则数量" }
      }
    })
  }, "zh-CN", makeCatalogTranslator(catalog));
  assert.equal(parsed.parseError, null);
  const fields = new Map(parsed.fields.map((field) => [field.key, field]));
  assert.equal(fields.get("rule_duration").control, "select");
  assert.deepEqual(fields.get("rule_duration").enumOptions,
    choices.map((value, index) => ({ value, label: labels[index] })));
  assert.equal(fields.get("rule_duration").title, "规则时长");
  assert.equal(fields.get("short_title").title, "规则频率");
  assert.equal(fields.get("numeric_title").title, "规则数量");
});

test("game identity is the first configuration section without parallel basic sections", () => {
  for (const moduleId of readRealModuleIds()) {
    const schema = parseGuidedSettingsSchema(moduleDetailsFor(moduleId), "en-US", makeCatalogTranslator(EN_US_MESSAGES));
    const model = buildConfigurationWorkspaceModel(schema);
    assert.equal(new Set(model.items.map((item) => item.fieldKey)).size, schema.presentationFields.length, moduleId);
    assert.equal(resolveConfigurationSectionId(model), "room", moduleId);
    assert.ok(!model.roots.some((node) => ["basics", "identity", "session"].includes(node.id)), moduleId);
    for (const field of schema.fields.filter((entry) => ["server_name", "server_description", "cluster_name", "cluster_description", "max_players", "max_server_players", "max_slots", "server_password", "cluster_password", "join_password", "password", "world_password"].includes(entry.key))) {
      assert.equal(field.sectionId, "room", `${moduleId}.${field.key}`);
    }
  }
});

test("DST room settings have a single entry and retain their native configuration keys", () => {
  const details = moduleDetailsFor("dontstarve");
  const schema = parseGuidedSettingsSchema(details, "zh-CN", makeCatalogTranslator(ZH_CN_MESSAGES));
  const model = buildConfigurationWorkspaceModel(schema);
  const nativeSchema = JSON.parse(details.schema_json);
  assert.equal(model.roots[0]?.id, "room");
  assert.equal(model.roots[0]?.title, "房间配置");
  assert.ok(!schema.sections.some((section) => section.id === "cluster-room-access"));
  for (const [key, nativeKey] of [
    ["cluster_name", "NETWORK.cluster_name"],
    ["cluster_description", "NETWORK.cluster_description"],
    ["cluster_password", "NETWORK.cluster_password"],
    ["max_players", "GAMEPLAY.max_players"]
  ]) {
    const items = model.items.filter((item) => item.fieldKey === key);
    assert.equal(items.length, 1, key);
    assert.equal(items[0].sectionId, "room", key);
    assert.equal(nativeSchema.properties[key]["x-lsgm-source-key"], nativeKey, key);
  }
  for (const sectionId of ["cluster-shard-coordination", "mastergen", "mastersettings", "cavesgen", "cavessettings"]) {
    assert.ok(model.actionableSectionIds.includes(sectionId), sectionId);
  }
});

test("each configuration field has one guided group or its registered specialized editor", () => {
  const translate = makeCatalogTranslator(EN_US_MESSAGES);
  for (const moduleId of readRealModuleIds()) {
    const definition = resolveSettingsModuleDefinition(moduleId);
    const schema = parseGuidedSettingsSchema(moduleDetailsFor(moduleId), "en-US", translate);
    for (const section of schema.sections) {
      const fields = schema.fields.filter((field) => field.sectionId === section.id);
      const groups = definition.buildFieldGroups?.(section.id, fields, "en-US", translate) ?? [];
      const groupedKeys = groups.length ? groups.flatMap((group) => group.fields.map((field) => field.key)) : fields.map((field) => field.key);
      for (const field of fields) {
        const renderer = definition.specializedRenderers?.[field.presentation.rendererId];
        const renderedByAddon = field.presentation.state === "specialized" && renderer?.kind === "module-addon";
        const count = groupedKeys.filter((key) => key === field.key).length;
        assert.ok(renderedByAddon ? count <= 1 : count === 1, `${moduleId}.${field.key} has ${count} guided placements`);
      }
    }
  }
});

test("Dont Starve settings page resolves zh-CN copy after the final catalog merge", () => {
  const zhSchema = parseGuidedSettingsSchema(moduleDetailsFor("dontstarve"), "zh-CN", makeCatalogTranslator(ZH_CN_MESSAGES));
  const enSchema = parseGuidedSettingsSchema(moduleDetailsFor("dontstarve"), "en-US", makeCatalogTranslator(EN_US_MESSAGES));
  const zhSections = new Map(zhSchema.sections.map((section) => [section.id, section.title]));
  const enSections = new Map(enSchema.sections.map((section) => [section.id, section.title]));
  const zhFields = new Map(zhSchema.fields.map((field) => [field.key, field]));
  const enFields = new Map(enSchema.fields.map((field) => [field.key, field]));
  const roomSection = zhSchema.sections.find((section) => section.id === "room");
  const clusterName = zhFields.get("cluster_name");
  const gameMode = zhFields.get("game_mode");
  const survivalOption = gameMode?.enumOptions?.find((option) => option.value === "survival");

  assert.equal(roomSection?.title, "房间配置");
  assert.equal(clusterName?.title, "房间名称");
  assert.equal(survivalOption?.label, "生存");
  assert.notEqual(clusterName?.title, enFields.get("cluster_name")?.title);
  assert.ok(/[\u3400-\u9fff]/u.test(ZH_CN_MESSAGES["dst.settings.bootstrap.importTitle"] ?? ""));
  assert.ok(/[\u3400-\u9fff]/u.test(ZH_CN_MESSAGES["dst.settings.modStatus.boolTrue"] ?? ""));
  assert.deepEqual(
    ["mastergen", "mastersettings", "cavesgen", "cavessettings"].map((id) => zhSections.get(id)),
    ["\u5730\u8868\u4e16\u754c\u751f\u6210", "\u5730\u8868\u4e16\u754c\u8bbe\u7f6e", "\u6d1e\u7a74\u4e16\u754c\u751f\u6210", "\u6d1e\u7a74\u4e16\u754c\u8bbe\u7f6e"]
  );
  assert.deepEqual(
    ["mastergen", "mastersettings", "cavesgen", "cavessettings"].map((id) => enSections.get(id)),
    ["Overworld Generation", "Overworld Settings", "Caves Generation", "Caves Settings"]
  );
});

test("ARK Survival Evolved ActiveMods field is owned by the server-level mods workbench", () => {
  const zhSchema = parseGuidedSettingsSchema(
    moduleDetailsFor("arksurvivalevolved"),
    "zh-CN",
    makeCatalogTranslator(ZH_CN_MESSAGES)
  );
  const enSchema = parseGuidedSettingsSchema(
    moduleDetailsFor("arksurvivalevolved"),
    "en-US",
    makeCatalogTranslator(EN_US_MESSAGES)
  );
  const zhFields = new Map(zhSchema.fields.map((field) => [field.key, field]));
  const enFields = new Map(enSchema.fields.map((field) => [field.key, field]));

  assert.equal(zhFields.has("active_mod_ids"), false);
  assert.equal(enFields.has("active_mod_ids"), false);
});
test("ARK Survival Ascended map field stays free-form while exposing registered map suggestions", () => {
  const schema = parseGuidedSettingsSchema(
    moduleDetailsFor("arksurvivalascended"),
    "en-US",
    makeCatalogTranslator(EN_US_MESSAGES)
  );
  const zhSchema = parseGuidedSettingsSchema(
    moduleDetailsFor("arksurvivalascended"),
    "zh-CN",
    makeCatalogTranslator(ZH_CN_MESSAGES)
  );
  const mapField = new Map(schema.fields.map((field) => [field.key, field])).get("map_name");
  const zhMapField = new Map(zhSchema.fields.map((field) => [field.key, field])).get("map_name");
  const suggestionValues = (mapField?.suggestions ?? []).map((option) => option.value);
  const zhSuggestionLabels = new Map((zhMapField?.suggestions ?? []).map((option) => [option.value, option.label]));
  const currentRegisteredMapValues = [
    "TheIsland_WP",
    "ScorchedEarth_WP",
    "TheCenter_WP",
    "Aberration_WP",
    "Extinction_WP",
    "Astraeos_WP",
    "Ragnarok_WP",
    "Valguero_WP",
    "Genesis_WP",
    "LostColony_WP",
    "BobsMissions_WP",
    "Svartalfheim_WP",
    "Forglar_WP",
    "Atlantis_WP",
    "LostCity_WP",
    "Appalachia_Official_WP",
    "Amissa_WP",
    "Vintal_WP",
    "insaluna_WP",
    "Temptress_WP",
    "Reverence_WP",
    "Nyrandil",
    "Althemia",
    "Thaloria_WP",
    "TheVolcano_WP",
    "EdenPremium_WP",
    "Mythica_WP",
    "M_ArkopolisWP",
    "TaeniaStella"
  ];

  assert.equal(mapField?.control, "text");
  assert.equal(mapField?.enumOptions, undefined);
  assert.equal(zhMapField?.control, "text");
  assert.equal(zhMapField?.enumOptions, undefined);
  assert.deepEqual(suggestionValues, currentRegisteredMapValues);
  assert.equal(zhSuggestionLabels.get("TheIsland_WP"), "孤岛 / The Island");
  assert.equal(zhSuggestionLabels.get("ScorchedEarth_WP"), "焦土飞升 / 焦土 / Scorched Earth");
  assert.equal(zhSuggestionLabels.get("TheCenter_WP"), "中心岛飞升 / 中心岛 / The Center");
  assert.equal(zhSuggestionLabels.get("Aberration_WP"), "畸变飞升 / 畸变 / Aberration");
  assert.equal(zhSuggestionLabels.get("Extinction_WP"), "灭绝飞升 / 灭绝 / Extinction");
  assert.equal(zhSuggestionLabels.get("Astraeos_WP"), "阿斯特瑞俄斯 / Astraeos");
  assert.equal(zhSuggestionLabels.get("Ragnarok_WP"), "仙境飞升 / 仙境 / Ragnarok");
  assert.equal(zhSuggestionLabels.get("Valguero_WP"), "瓦尔盖罗飞升 / 瓦尔盖罗 / Valguero");
  assert.equal(zhSuggestionLabels.get("LostColony_WP"), "失落殖民地 / Lost Colony");
});

test("ARK Survival Evolved map field stays free-form while exposing all official map suggestions", () => {
  const schema = parseGuidedSettingsSchema(
    moduleDetailsFor("arksurvivalevolved"),
    "en-US",
    makeCatalogTranslator(EN_US_MESSAGES)
  );
  const zhSchema = parseGuidedSettingsSchema(
    moduleDetailsFor("arksurvivalevolved"),
    "zh-CN",
    makeCatalogTranslator(ZH_CN_MESSAGES)
  );
  const mapField = new Map(schema.fields.map((field) => [field.key, field])).get("map_name");
  const zhMapField = new Map(zhSchema.fields.map((field) => [field.key, field])).get("map_name");
  const suggestionValues = (mapField?.suggestions ?? []).map((option) => option.value);
  const zhSuggestionLabels = new Map((zhMapField?.suggestions ?? []).map((option) => [option.value, option.label]));
  const officialMapValues = [
    "TheIsland",
    "TheCenter",
    "ScorchedEarth_P",
    "Ragnarok",
    "Aberration_P",
    "Extinction",
    "Valguero_P",
    "Genesis",
    "CrystalIsles",
    "Gen2",
    "LostIsland",
    "Fjordur",
    "Aquatica"
  ];

  assert.equal(mapField?.control, "text");
  assert.equal(mapField?.enumOptions, undefined);
  assert.equal(zhMapField?.control, "text");
  assert.equal(zhMapField?.enumOptions, undefined);
  assert.deepEqual(suggestionValues, officialMapValues);
  assert.equal(zhSuggestionLabels.get("TheCenter"), "中心岛 / 中心 / The Center");
  assert.equal(zhSuggestionLabels.get("Ragnarok"), "仙境 / Ragnarok");
  assert.equal(zhSuggestionLabels.get("Genesis"), "创世 1 / 创世纪 1 / Genesis: Part 1");
  assert.equal(zhSuggestionLabels.get("Gen2"), "创世 2 / 创世纪 2 / Genesis: Part 2");
  assert.equal(zhSuggestionLabels.get("Valguero_P"), "瓦尔盖罗 / Valguero");
  assert.equal(zhSuggestionLabels.get("LostIsland"), "迷失岛 / 失落岛 / Lost Island");
  assert.equal(zhSuggestionLabels.get("Fjordur"), "菲尤尔 / 峡湾 / Fjordur");
  assert.equal(zhSuggestionLabels.get("CrystalIsles"), "水晶岛 / Crystal Isles");
  assert.equal(zhSuggestionLabels.get("Aquatica"), "水域 / 水世界 / Aquatica");
});
test("authored Chinese field descriptions survive localization without a source marker", () => {
  for (const [description, expected] of [
    ["设置同时在线的玩家人数上限。", "设置同时在线的玩家人数上限。"],
    ["Native description: 设置同时在线的玩家人数上限。", "设置同时在线的玩家人数上限。"],
    ["配置项", null]
  ]) {
    const translator = makeCatalogTranslator({
      ...ZH_CN_MESSAGES,
      "settings.schema.nightingale.max_players.title": "玩家人数上限",
      "settings.schema.nightingale.max_players.description": description
    });
    const parsed = parseGuidedSettingsSchema(moduleDetailsFor("nightingale"), "zh-CN", translator);
    const field = parsed.fields.find((entry) => entry.key === "max_players");
    assert.ok(field);
    assert.equal(field.description, expected, description);
  }
});
