const fs = require("fs");
const path = require("path");
const { isLanguageNeutralMessage } = require("./i18n-language-neutral.cjs");
const { collectTranslationReferences, extractMessagePlaceholders } = require("./i18n-source-references.cjs");
const { collectCatalogFilesBySuffix } = require("./catalog-file-discovery.cjs");
const { loadEffectiveCatalogs } = require("./i18n-catalog-loader.cjs");

const ROOT = path.resolve(__dirname, "..");
const WORKSPACE_ROOT = path.resolve(ROOT, "../..");
const GAME_CATALOG_DIRECTORY = "src/i18n/games";

const ENGLISH_GAME_CATALOG_FILES = collectCatalogFilesBySuffix(ROOT, GAME_CATALOG_DIRECTORY, ".en.ts");
const CHINESE_GAME_CATALOG_FILES = collectCatalogFilesBySuffix(ROOT, GAME_CATALOG_DIRECTORY, ".zh-cn.ts");

function readWorkspaceFile(relativePath) {
  return fs.readFileSync(path.join(ROOT, relativePath), "utf8");
}

function readRepoFile(relativePath) {
  return fs.readFileSync(path.join(WORKSPACE_ROOT, relativePath), "utf8");
}

function collectSourceFiles(directory) {
  const absoluteDirectory = path.join(ROOT, directory);
  const entries = fs.readdirSync(absoluteDirectory, { withFileTypes: true });
  const files = [];

  for (const entry of entries) {
    if (entry.name === "node_modules" || entry.name === "dist") {
      continue;
    }

    const relativePath = path.join(directory, entry.name).replace(/\\/g, "/");
    if (entry.isDirectory()) {
      files.push(...collectSourceFiles(relativePath));
      continue;
    }

    if (/\.(ts|tsx)$/.test(entry.name)) {
      files.push(relativePath);
    }
  }

  return files;
}

function collectSourceTranslationReferences() {
  const calls = new Map();
  const sourceFiles = collectSourceFiles("src");

  for (const file of sourceFiles) {
    const source = readWorkspaceFile(file);
    for (const [key, locations] of collectTranslationReferences(source, file)) {
      calls.set(key, [...(calls.get(key) ?? []), ...locations]);
    }
  }

  return calls;
}

function buildSchemaEnumOptionKey(value) {
  const raw = String(value).trim();
  const prefix = raw.startsWith("-") ? "minus_" : "";
  const normalized = raw
    .replace(/^-+/, "")
    .replace(/[^A-Za-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "")
    .toLowerCase();

  return `${prefix}${normalized || "empty"}`;
}

function collectModuleSchemaTranslationKeys() {
  const modulesRoot = path.join(WORKSPACE_ROOT, "modules");
  if (!fs.existsSync(modulesRoot)) {
    return [];
  }

  const keys = [];
  const moduleIds = fs
    .readdirSync(modulesRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort();

  for (const moduleId of moduleIds) {
    const schemaPath = path.join("modules", moduleId, "schema.json").replace(/\\/g, "/");
    const absoluteSchemaPath = path.join(WORKSPACE_ROOT, schemaPath);
    if (!fs.existsSync(absoluteSchemaPath)) {
      continue;
    }

    const schema = JSON.parse(readRepoFile(schemaPath));
    const properties = schema.properties && typeof schema.properties === "object" ? schema.properties : {};
    for (const [fieldKey, property] of Object.entries(properties)) {
      if (fieldKey === "bind_ip" || !property || typeof property !== "object" || Array.isArray(property)) {
        continue;
      }

      const baseKey = `settings.schema.${moduleId}.${fieldKey}`;
      keys.push(`${baseKey}.title`);
      if (typeof property.description === "string" && property.description.trim()) {
        keys.push(`${baseKey}.description`);
      }
      if (Array.isArray(property.enum)) {
        for (const optionValue of property.enum) {
          keys.push(`${baseKey}.option.${buildSchemaEnumOptionKey(optionValue)}`);
        }
      }
    }
  }

  return Array.from(new Set(keys)).sort();
}

function collectMissingGameCatalogImports(indexFile, catalogFiles) {
  const source = readWorkspaceFile(indexFile);
  return catalogFiles
    .filter((file) => {
      const importPath = `./${path.basename(file, ".ts")}`;
      return !source.includes(`"${importPath}"`) && !source.includes(`'${importPath}'`);
    })
    .sort();
}

function auditCatalogPair(englishCatalog, chineseCatalog, options = {}) {
  const missing = [...englishCatalog.keys()]
    .filter((key) => !chineseCatalog.has(key))
    .sort();
  const extra = options.checkExtra
    ? [...chineseCatalog.keys()].filter((key) => !englishCatalog.has(key)).sort()
    : [];
  const unreadable = [];
  const copied = [];
  const corrupt = [];
  const mismatchedPlaceholders = [];

  for (const [key, chineseValue] of chineseCatalog) {
    if (options.onlyKeys && !options.onlyKeys.has(key)) continue;
    if (!/[\u3400-\u9fff]/u.test(chineseValue) && !isLanguageNeutralMessage(chineseValue)) {
      unreadable.push(`${key} = ${JSON.stringify(chineseValue)}`);
    }
    if (/\?{2,}|\uFFFD/u.test(chineseValue)) {
      corrupt.push(`${key} = ${JSON.stringify(chineseValue)}`);
    }
  }

  for (const [key, englishValue] of englishCatalog) {
    if (options.onlyKeys && !options.onlyKeys.has(key)) continue;
    const chineseValue = chineseCatalog.get(key);
    if (chineseValue === undefined) continue;
    if (chineseValue === englishValue && !isLanguageNeutralMessage(englishValue)) {
      copied.push(`${key} = ${JSON.stringify(englishValue)}`);
    }
    const enPlaceholders = extractMessagePlaceholders(englishValue);
    const zhPlaceholders = extractMessagePlaceholders(chineseValue);
    if (JSON.stringify(enPlaceholders) !== JSON.stringify(zhPlaceholders)) {
      mismatchedPlaceholders.push(`${key}: en=${JSON.stringify(enPlaceholders)}, zh=${JSON.stringify(zhPlaceholders)}`);
    }
  }

  return {
    missing,
    extra,
    unreadable: unreadable.sort(),
    copied: copied.sort(),
    corrupt: corrupt.sort(),
    mismatchedPlaceholders: mismatchedPlaceholders.sort()
  };
}

function printMissing(title, entries) {
  if (entries.length === 0) {
    return;
  }

  console.error(`\n${title}`);
  for (const entry of entries.slice(0, 120)) {
    console.error(`  ${entry}`);
  }

  if (entries.length > 120) {
    console.error(`  ...and ${entries.length - 120} more`);
  }
}

const {
  english: effectiveEnglishCatalog,
  chinese: effectiveChineseCatalog,
  englishGames: effectiveEnglishGameCatalog,
  chineseGames: effectiveChineseGameCatalog
} = loadEffectiveCatalogs();
const translationCalls = collectSourceTranslationReferences();
const schemaTranslationKeys = collectModuleSchemaTranslationKeys();

const missingEnglishKeys = [...translationCalls.keys()]
  .filter((key) => !effectiveEnglishCatalog.has(key))
  .sort()
  .map((key) => `${key} <- ${translationCalls.get(key)[0]}`);

const missingEnglishSchemaKeys = schemaTranslationKeys
  .filter((key) => !effectiveEnglishCatalog.has(key))
  .sort();

const missingChineseSchemaKeys = schemaTranslationKeys
  .filter((key) => !effectiveChineseCatalog.has(key))
  .sort();

const missingEnglishGameCatalogImports = collectMissingGameCatalogImports(
  "src/i18n/games/en-us.ts",
  ENGLISH_GAME_CATALOG_FILES
);

const missingChineseGameCatalogImports = collectMissingGameCatalogImports(
  "src/i18n/games/zh-cn.ts",
  CHINESE_GAME_CATALOG_FILES
);

// Final catalog checks cover all unshadowed game entries; audit game catalogs directly
// only for game-level key parity and entries overridden by outer catalog spreads.
const shadowedGameKeys = new Set();
for (const [key, value] of effectiveEnglishGameCatalog) {
  if (effectiveEnglishCatalog.get(key) !== value) shadowedGameKeys.add(key);
}
for (const [key, value] of effectiveChineseGameCatalog) {
  if (effectiveChineseCatalog.get(key) !== value) shadowedGameKeys.add(key);
}

const gameAudit = auditCatalogPair(effectiveEnglishGameCatalog, effectiveChineseGameCatalog, {
  checkExtra: true,
  onlyKeys: shadowedGameKeys
});
const finalAudit = auditCatalogPair(effectiveEnglishCatalog, effectiveChineseCatalog);

const mechanicalDontStarveMessages = [...effectiveChineseGameCatalog]
  .filter(([key, value]) => (
    key.startsWith("dst.") && (/原生配置：\s*[A-Za-z]/u.test(value) || value === "选项")
  ))
  .map(([key, value]) => `${key} = ${JSON.stringify(value)}`)
  .sort();

const chineseGameIndexSource = readWorkspaceFile("src/i18n/games/zh-cn.ts");
const compatibilityFilterMarkers = [
  "filterReadableChineseMessages",
  "hasReadableChineseCopy"
].filter((marker) => chineseGameIndexSource.includes(marker));

const chineseCatalogSource = readWorkspaceFile("src/i18n-messages-zh-cn.ts");
if (chineseCatalogSource.includes("...EN_US_MESSAGES")) {
  compatibilityFilterMarkers.push("...EN_US_MESSAGES");
}

const checks = [
  ["Missing en-US messages for translation references:", missingEnglishKeys],
  ["Missing en-US schema messages:", missingEnglishSchemaKeys],
  ["Missing zh-CN schema messages:", missingChineseSchemaKeys],
  ["Game en-US catalog files not imported by src/i18n/games/en-us.ts:", missingEnglishGameCatalogImports],
  ["Game zh-CN catalog files not imported by src/i18n/games/zh-cn.ts:", missingChineseGameCatalogImports],
  ["Missing messages from the effective zh-CN game catalog:", gameAudit.missing],
  ["Unexpected messages in the effective zh-CN game catalog:", gameAudit.extra],
  ["Shadowed zh-CN game messages without readable Chinese:", gameAudit.unreadable],
  ["Shadowed zh-CN game messages copied from en-US:", gameAudit.copied],
  ["Corrupt shadowed zh-CN game messages:", gameAudit.corrupt],
  ["Shadowed game message placeholder mismatches:", gameAudit.mismatchedPlaceholders],
  ["Mechanical Don't Starve Together zh-CN messages:", mechanicalDontStarveMessages],
  ["Missing messages from the final zh-CN catalog:", finalAudit.missing],
  ["Final zh-CN messages without readable Chinese:", finalAudit.unreadable],
  ["Final zh-CN messages copied from en-US:", finalAudit.copied],
  ["Corrupt final zh-CN messages:", finalAudit.corrupt],
  ["Final message placeholder mismatches:", finalAudit.mismatchedPlaceholders],
  ["Removed zh-CN compatibility filters were reintroduced:", compatibilityFilterMarkers]
];

if (checks.some(([, entries]) => entries.length > 0)) {
  for (const [title, entries] of checks) {
    printMissing(title, entries);
  }
  process.exitCode = 1;
} else {
  console.log(
    `i18n catalogs verified: ${effectiveEnglishCatalog.size} en-US keys, ${effectiveChineseCatalog.size} final zh-CN messages, ${effectiveChineseGameCatalog.size} effective zh-CN game messages, ${translationCalls.size} translation references, ${schemaTranslationKeys.length} schema keys.`
  );
}
