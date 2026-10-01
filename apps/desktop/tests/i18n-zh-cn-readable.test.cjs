const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const gamesDir = path.resolve(__dirname, "..", "src", "i18n", "games");
const settingsModulesDir = path.resolve(__dirname, "..", "src", "views", "settings", "modules");
const srcDir = path.resolve(__dirname, "..", "src");

const checkedCatalogs = [
  "arksurvivalascended",
  "arksurvivalevolved",
  "barotrauma",
  "conanexiles",
  "dontstarve",
  "enshrouded",
  "necesse",
  "rust",
  "satisfactory",
  "scum",
  "sevendaystodie",
  "sonsoftheforest",
  "soulmask",
  "squad",
  "valheim",
  "windrose",
];

const mojibakePatterns = [
  "???",
  "\uFFFD",
  "\u9286",
  "\u951b",
  "\u9239",
  "\u5a11\u6497\u504d",
  "\u95ba\u5806\u79f4",
  "\u95b8\u612d\u7465",
  "\u940e\u6d99\uE686",
  "\u7f01\u5b0b\uE0c5",
];

function readCatalogSource(moduleId, locale) {
  const baseName = `${moduleId}.${locale}`;
  const files = fs.readdirSync(gamesDir)
    .filter((fileName) => fileName === `${baseName}.ts` || (
      fileName.startsWith(`${baseName}.part`) && fileName.endsWith(".ts")
    ))
    .sort();
  return files
    .map((fileName) => fs.readFileSync(path.join(gamesDir, fileName), "utf8"))
    .join("\n");
}

function extractCatalog(moduleId, locale) {
  const source = readCatalogSource(moduleId, locale);
  return extractEntriesFromSource(source);
}

function extractGeneratedSchemaCatalog(locale) {
  const chunkPattern = new RegExp(`^schema-generated\\.${locale}\\.chunk-\\d+\\.ts$`);
  const source = fs.readdirSync(gamesDir)
    .filter((fileName) => chunkPattern.test(fileName))
    .sort()
    .map((fileName) => fs.readFileSync(path.join(gamesDir, fileName), "utf8"))
    .join("\n");
  return extractEntriesFromSource(source);
}

function extractEffectiveGameCatalog(locale) {
  const catalog = extractGeneratedSchemaCatalog(locale);
  const indexName = locale === "en" ? "en-us.ts" : "zh-cn.ts";
  const indexSource = fs.readFileSync(path.join(gamesDir, indexName), "utf8");
  const importPattern = /from\s+["']\.\/([^"']+)["'];/g;
  let match;

  while ((match = importPattern.exec(indexSource)) !== null) {
    const importStem = match[1];
    if (importStem === `schema-generated.${locale}`) {
      continue;
    }

    const source = fs.readdirSync(gamesDir)
      .filter((fileName) => fileName === `${importStem}.ts` || (
        fileName.startsWith(`${importStem}.part`) && fileName.endsWith(".ts")
      ))
      .sort()
      .map((fileName) => fs.readFileSync(path.join(gamesDir, fileName), "utf8"))
      .join("\n");

    for (const [key, value] of extractEntriesFromSource(source)) {
      catalog.set(key, value);
    }
  }

  return catalog;
}

function extractEntriesFromSource(source) {
  const entries = new Map();
  const pattern = /"([^"]+)"\s*:\s*"((?:\\.|[^"\\])*)"/g;
  let match;

  while ((match = pattern.exec(source)) !== null) {
    entries.set(match[1], JSON.parse(`"${match[2]}"`));
  }

  return entries;
}

function hasHanText(value) {
  return /[\u3400-\u9fff]/u.test(value);
}

function extractPlaceholders(value) {
  return Array.from(value.matchAll(/\{([A-Za-z0-9_]+)\}/g), (match) => match[1]).sort();
}

function isLanguageNeutralMessage(value) {
  const normalized = value.trim();
  const withoutPlaceholders = normalized.replace(/\{[A-Za-z0-9_]+\}/g, "");

  if (!withoutPlaceholders || /^[\d\s.,:+/|()\[\]_-]+$/.test(withoutPlaceholders)) {
    return true;
  }

  if (/^[A-Za-z0-9_.-]+\.(?:cfg|ini|json|lua|toml|txt|xml)$/i.test(normalized)) {
    return true;
  }

  return /^(?:BattlEye|JSON|PVE|PVP|PvE|PvP|RCON|REST(?: API)?|TCP|Telnet|UDP|UPnP|VAC)(?:\s*[+/]\s*(?:JSON|PVE|PVP|PvE|PvP|RCON|REST(?: API)?|TCP|Telnet|UDP|UPnP|VAC))*$/.test(normalized);
}

test("checked zh-CN game catalogs do not contain common mojibake fragments", () => {
  const failures = [];

  for (const moduleId of checkedCatalogs) {
    const source = readCatalogSource(moduleId, "zh-cn");

    for (const pattern of mojibakePatterns) {
      if (source.includes(pattern)) {
        failures.push(`${moduleId}: ${JSON.stringify(pattern)}`);
      }
    }
  }

  assert.deepEqual(failures, []);
});

test("effective zh-CN game catalog is complete without silent English fallback", () => {
  const english = extractEffectiveGameCatalog("en");
  const chinese = extractEffectiveGameCatalog("zh-cn");
  const failures = [];

  assert.deepEqual([...chinese.keys()].sort(), [...english.keys()].sort());

  for (const [key, chineseValue] of chinese) {
    const englishValue = english.get(key);
    const languageNeutral = isLanguageNeutralMessage(chineseValue);

    if (!hasHanText(chineseValue) && !languageNeutral) {
      failures.push(`${key}: no readable Chinese copy (${JSON.stringify(chineseValue)})`);
    }
    if (englishValue === chineseValue && !languageNeutral) {
      failures.push(`${key}: copied from en-US (${JSON.stringify(chineseValue)})`);
    }
    if (/\?{2,}|\uFFFD/u.test(chineseValue)) {
      failures.push(`${key}: corrupt text (${JSON.stringify(chineseValue)})`);
    }
    if (key.startsWith("dst.") && (/原生配置：\s*[A-Za-z]/u.test(chineseValue) || chineseValue === "选项")) {
      failures.push(`${key}: mechanical generated copy (${JSON.stringify(chineseValue)})`);
    }
    if (
      englishValue !== undefined &&
      JSON.stringify(extractPlaceholders(chineseValue)) !== JSON.stringify(extractPlaceholders(englishValue))
    ) {
      failures.push(`${key}: placeholder mismatch`);
    }
  }

  const chineseIndexSource = fs.readFileSync(path.join(gamesDir, "zh-cn.ts"), "utf8");
  const chineseCatalogSource = fs.readFileSync(path.join(srcDir, "i18n-messages-zh-cn.ts"), "utf8");
  assert.equal(chineseIndexSource.includes("filterReadableChineseMessages"), false);
  assert.equal(chineseIndexSource.includes("hasReadableChineseCopy"), false);
  assert.equal(chineseCatalogSource.includes("...EN_US_MESSAGES"), false);
  assert.equal(chinese.get("servers.dst.accessEntryPlaceholder"), "粘贴 Klei 用户 ID，例如 KU_xxxxx");
  assert.equal(chinese.get("servers.dst.livePlayersPlaceholder"), "此处显示加入权限和实时会话状态。");
  assert.deepEqual(failures, []);
});

test("generated zh-CN schema copy is complete and free of mechanical placeholders", () => {
  const english = extractGeneratedSchemaCatalog("en");
  const chinese = extractGeneratedSchemaCatalog("zh-cn");
  const failures = [];

  assert.deepEqual([...chinese.keys()].sort(), [...english.keys()].sort());

  for (const [key, value] of chinese) {
    if (value === "配置项") {
      failures.push(`${key}: generic title placeholder`);
    }
    if (/Native description:/i.test(value)) {
      failures.push(`${key}: leaked native-description marker`);
    }
    if (/启动参数\s+启动参数/u.test(value)) {
      failures.push(`${key}: duplicated launch-argument suffix`);
    }
    if (/将[“\s]*配置项/u.test(value)) {
      failures.push(`${key}: generic field placeholder in description`);
    }
  }

  assert.deepEqual(failures, []);
  assert.equal(chinese.get("settings.schema.soulmask.xishu_exp_ratio.title"), "意识经验倍率");
  assert.match(
    chinese.get("settings.schema.soulmask.xishu_exp_ratio.description") ?? "",
    /意识经验倍率.*ExpRatio/u
  );
});

test("generated en-US Soulmask descriptions do not leak native Chinese source copy", () => {
  const english = extractGeneratedSchemaCatalog("en");
  const soulmaskDescriptions = [...english]
    .filter(([key]) => key.startsWith("settings.schema.soulmask.") && key.endsWith(".description"));
  const failures = soulmaskDescriptions.filter(([, value]) => /Native description:|[\u3400-\u9fff]/u.test(value));

  assert.deepEqual(failures, []);
  assert.equal(
    english.get("settings.schema.soulmask.xishu_exp_ratio.description"),
    "Controls ExpRatio in Soulmask GameXishu.json."
  );
});

test("zh-CN assistant messages use LAN branding without implementation-facing copy", () => {
  const coreSource = fs.readFileSync(path.join(srcDir, "i18n-messages-zh-core.ts"), "utf8");
  const uiSource = fs.readFileSync(path.join(srcDir, "i18n-messages-zh-ui.ts"), "utf8");
  const core = extractEntriesFromSource(coreSource);
  const ui = extractEntriesFromSource(uiSource);
  const merged = new Map([...core.entries(), ...ui.entries()]);
  const keys = [
    "assistant.settings.saved",
    "assistant.settings.keyStoredSuffix",
    "assistant.settings.failed",
    "assistant.settings.cleared",
    "assistant.settings.clearFailed",
    "assistant.settings.baseUrlRequired",
    "assistant.run.notReady",
    "assistant.chat.assistantName",
    "assistant.chat.userName",
    "assistant.chat.inputPlaceholder",
    "assistant.chat.userAvatar",
    "assistant.chat.greeting",
    "assistant.chat.quickTitle",
    "assistant.chat.emptyTitle",
    "assistant.chat.emptyBody",
    "assistant.chat.sendLabel",
    "assistant.chat.runningLabel",
    "assistant.chat.jumpLabel",
    "assistant.history.tooltip",
    "assistant.history.title",
    "assistant.history.close",
    "assistant.history.new",
    "assistant.history.emptyTitle",
    "assistant.history.emptyBody",
    "assistant.history.untitled",
    "assistant.history.delete",
    "assistant.history.deleteConfirm",
    "assistant.history.cancel",
    "assistant.run.started",
    "assistant.run.success",
    "assistant.run.failed",
    "assistant.broadcast.defaultStartupIntent",
    "assistant.broadcast.defaultShutdownIntent",
  ];

  const catalogs = [
    ["zh-CN core", core],
    ["zh-CN ui", ui],
    ["zh-CN merged", merged],
  ];

  for (const [label, catalog] of catalogs) {
    for (const key of keys) {
      const value = catalog.get(key) ?? "";
      assert.ok(value, `${key} should be present in ${label} assistant messages`);
      if (key !== "assistant.chat.assistantName") {
        assert.ok(hasHanText(value), `${key} should be localized in ${label} instead of falling back to English`);
      }
    }

    assert.equal(catalog.get("assistant.chat.assistantName"), "LAN", `${label} should identify the assistant as LAN`);
    assert.equal(catalog.has("assistant.chat.panelHint"), false, `${label} should not expose page-context implementation copy`);
    assert.equal(catalog.has("assistant.chat.composerHint"), false, `${label} should not expose backend implementation copy`);
  }
});

test("ARK Survival Evolved zh-CN schema is translated instead of copied from English", () => {
  const zh = extractCatalog("arksurvivalevolved", "zh-cn");
  const en = extractCatalog("arksurvivalevolved", "en");
  const schemaKeys = [...en.keys()].filter((key) => key.startsWith("settings.schema.arksurvivalevolved."));
  const unchanged = schemaKeys.filter((key) => zh.get(key) === en.get(key));

  assert.ok(hasHanText(zh.get("settings.schema.arksurvivalevolved.server_name.title") ?? ""));
  assert.ok(
    unchanged.length < 20,
    `Expected ARK Survival Evolved zh-CN schema to be localized; ${unchanged.length}/${schemaKeys.length} entries still match English.`
  );
});

test("ARK Survival Evolved zh-CN catalog does not use Ascended product copy", () => {
  const source = readCatalogSource("arksurvivalevolved", "zh-cn");

  assert.equal(source.includes("ARK: Survival Ascended"), false);
});

test("merged zh-CN ARK shared settings labels stay localized after catalog merge", () => {
  const ascended = extractCatalog("arksurvivalascended", "zh-cn");
  const evolved = extractCatalog("arksurvivalevolved", "zh-cn");
  const merged = new Map([...ascended, ...evolved]);
  const sharedKeys = [
    "ark.settings.sections.world",
    "ark.settings.sections.network",
    "ark.settings.sections.operations",
    "ark.settings.sections.modsDescription",
    "ark.settings.sections.advancedDescription",
  ];

  for (const key of sharedKeys) {
    assert.ok(hasHanText(merged.get(key) ?? ""), `${key} should resolve to zh-CN text after merging ARK catalogs.`);
  }
});

test("ARK Survival Evolved settings page uses its Steam Workshop mods description key", () => {
  const source = fs.readFileSync(path.join(settingsModulesDir, "ark-ase.ts"), "utf8");

  assert.match(source, /"arkse\.settings\.sections\.modsDescription"/);
});
