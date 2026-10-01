const fs = require("fs");
const path = require("path");
const { isLanguageNeutralMessage } = require("./i18n-language-neutral.cjs");
const { collectTranslationReferences, extractMessagePlaceholders } = require("./i18n-source-references.cjs");
const {
  collectCatalogFilesBySuffix,
  collectLocaleCatalogFiles
} = require("./catalog-file-discovery.cjs");

const ROOT = path.resolve(__dirname, "..");
const WORKSPACE_ROOT = path.resolve(ROOT, "../..");
const GAME_CATALOG_DIRECTORY = "src/i18n/games";

const ENGLISH_GAME_CATALOG_FILES = collectCatalogFilesBySuffix(ROOT, GAME_CATALOG_DIRECTORY, ".en.ts");
const CHINESE_GAME_CATALOG_FILES = collectCatalogFilesBySuffix(ROOT, GAME_CATALOG_DIRECTORY, ".zh-cn.ts");
const ENGLISH_LOCALE_CATALOG_FILES = collectLocaleCatalogFiles(ROOT, GAME_CATALOG_DIRECTORY, "en");
const CHINESE_LOCALE_CATALOG_FILES = collectLocaleCatalogFiles(ROOT, GAME_CATALOG_DIRECTORY, "zh-cn");

const ENGLISH_CATALOG_FILES = [
  "src/i18n-messages.ts",
  "src/i18n-messages-en-extra.ts",
  ...ENGLISH_LOCALE_CATALOG_FILES
];

const EXPLICIT_CHINESE_CATALOG_FILES = [
  "src/i18n-messages-zh-core.ts",
  "src/i18n-messages-zh-ui.ts",
  "src/i18n-messages-zh-extra.ts",
  "src/i18n-messages-zh-settings.ts",
  "src/i18n-messages-zh-schema.ts",
  ...CHINESE_LOCALE_CATALOG_FILES
];

function readWorkspaceFile(relativePath) {
  return fs.readFileSync(path.join(ROOT, relativePath), "utf8");
}

function readRepoFile(relativePath) {
  return fs.readFileSync(path.join(WORKSPACE_ROOT, relativePath), "utf8");
}

function extractCatalogKeys(relativePath) {
  const source = readWorkspaceFile(relativePath);
  const keys = new Set();
  const keyPattern = /["']([A-Za-z0-9_.-]+(?:\.[A-Za-z0-9_.-]+)+)["']\s*:/g;
  let match;

  while ((match = keyPattern.exec(source)) !== null) {
    keys.add(match[1]);
  }

  return keys;
}

function extractCatalogEntries(relativePath) {
  const source = readWorkspaceFile(relativePath);
  const entries = new Map();
  const entryPattern = /"([^"]+)"\s*:\s*"((?:\\.|[^"\\])*)"/g;
  let match;

  while ((match = entryPattern.exec(source)) !== null) {
    entries.set(match[1], JSON.parse(`"${match[2]}"`));
  }

  return entries;
}

function buildEffectiveGameCatalog(indexFile, locale) {
  const catalog = new Map();
  const generatedPattern = new RegExp(`^schema-generated\\.${locale}\\.chunk-\\d+\\.ts$`);
  const gameDirectory = path.join(ROOT, GAME_CATALOG_DIRECTORY);
  const directoryEntries = fs.readdirSync(gameDirectory).sort();

  for (const fileName of directoryEntries.filter((fileName) => generatedPattern.test(fileName))) {
    const relativePath = `${GAME_CATALOG_DIRECTORY}/${fileName}`;
    for (const [key, value] of extractCatalogEntries(relativePath)) {
      catalog.set(key, value);
    }
  }

  const indexSource = readWorkspaceFile(indexFile);
  const importPattern = /from\s+["']\.\/([^"']+)["'];/g;
  let match;

  while ((match = importPattern.exec(indexSource)) !== null) {
    const importStem = match[1];
    if (importStem === `schema-generated.${locale}`) {
      continue;
    }

    const importedFiles = directoryEntries
      .filter((fileName) => fileName === `${importStem}.ts` || (
        fileName.startsWith(`${importStem}.part`) && fileName.endsWith(".ts")
      ))
      .sort();

    for (const fileName of importedFiles) {
      const relativePath = `${GAME_CATALOG_DIRECTORY}/${fileName}`;
      for (const [key, value] of extractCatalogEntries(relativePath)) {
        catalog.set(key, value);
      }
    }
  }

  return catalog;
}

function mergeCatalogs(...catalogs) {
  const merged = new Map();
  for (const catalog of catalogs) {
    for (const [key, value] of catalog) {
      merged.set(key, value);
    }
  }
  return merged;
}

function extractPlaceholders(value) {
  return extractMessagePlaceholders(value);
}

function collectCatalogKeys(files) {
  const keys = new Set();

  for (const file of files) {
    for (const key of extractCatalogKeys(file)) {
      keys.add(key);
    }
  }

  return keys;
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

const englishKeys = collectCatalogKeys(ENGLISH_CATALOG_FILES);
const explicitChineseKeys = collectCatalogKeys(EXPLICIT_CHINESE_CATALOG_FILES);
const translationCalls = collectSourceTranslationReferences();
const schemaTranslationKeys = collectModuleSchemaTranslationKeys();

const missingChineseKeys = [...englishKeys]
  .filter((key) => !explicitChineseKeys.has(key))
  .sort();

const missingEnglishKeys = [...translationCalls.keys()]
  .filter((key) => !englishKeys.has(key))
  .sort()
  .map((key) => `${key} <- ${translationCalls.get(key)[0]}`);

const missingEnglishSchemaKeys = schemaTranslationKeys
  .filter((key) => !englishKeys.has(key))
  .sort();

const missingChineseSchemaKeys = schemaTranslationKeys
  .filter((key) => !explicitChineseKeys.has(key))
  .sort();

const missingEnglishGameCatalogImports = collectMissingGameCatalogImports(
  "src/i18n/games/en-us.ts",
  ENGLISH_GAME_CATALOG_FILES
);

const missingChineseGameCatalogImports = collectMissingGameCatalogImports(
  "src/i18n/games/zh-cn.ts",
  CHINESE_GAME_CATALOG_FILES
);

const effectiveEnglishGameCatalog = buildEffectiveGameCatalog("src/i18n/games/en-us.ts", "en");
const effectiveChineseGameCatalog = buildEffectiveGameCatalog("src/i18n/games/zh-cn.ts", "zh-cn");
const effectiveEnglishCatalog = mergeCatalogs(
  extractCatalogEntries("src/i18n-messages.ts"),
  effectiveEnglishGameCatalog,
  extractCatalogEntries("src/i18n-messages-en-extra.ts")
);
const effectiveChineseCatalog = mergeCatalogs(
  extractCatalogEntries("src/i18n-messages-zh-core.ts"),
  extractCatalogEntries("src/i18n-messages-zh-ui.ts"),
  effectiveChineseGameCatalog,
  extractCatalogEntries("src/i18n-messages-zh-extra.ts"),
  extractCatalogEntries("src/i18n-messages-zh-settings.ts"),
  extractCatalogEntries("src/i18n-messages-zh-schema.ts")
);

const missingEffectiveChineseGameMessages = [...effectiveEnglishGameCatalog.keys()]
  .filter((key) => !effectiveChineseGameCatalog.has(key))
  .sort();

const extraEffectiveChineseGameMessages = [...effectiveChineseGameCatalog.keys()]
  .filter((key) => !effectiveEnglishGameCatalog.has(key))
  .sort();

const unreadableEffectiveChineseGameMessages = [...effectiveChineseGameCatalog]
  .filter(([, value]) => !/[\u3400-\u9fff]/u.test(value) && !isLanguageNeutralMessage(value))
  .map(([key, value]) => `${key} = ${JSON.stringify(value)}`)
  .sort();

const copiedEnglishGameMessages = [...effectiveEnglishGameCatalog]
  .filter(([key, value]) => (
    effectiveChineseGameCatalog.get(key) === value && !isLanguageNeutralMessage(value)
  ))
  .map(([key, value]) => `${key} = ${JSON.stringify(value)}`)
  .sort();

const corruptEffectiveChineseGameMessages = [...effectiveChineseGameCatalog]
  .filter(([, value]) => /\?{2,}|\uFFFD/u.test(value))
  .map(([key, value]) => `${key} = ${JSON.stringify(value)}`)
  .sort();

const mechanicalDontStarveMessages = [...effectiveChineseGameCatalog]
  .filter(([key, value]) => (
    key.startsWith("dst.") && (/原生配置：\s*[A-Za-z]/u.test(value) || value === "选项")
  ))
  .map(([key, value]) => `${key} = ${JSON.stringify(value)}`)
  .sort();

const mismatchedGameMessagePlaceholders = [...effectiveEnglishGameCatalog]
  .filter(([key, englishValue]) => {
    const chineseValue = effectiveChineseGameCatalog.get(key);
    return chineseValue !== undefined && (
      JSON.stringify(extractPlaceholders(englishValue)) !== JSON.stringify(extractPlaceholders(chineseValue))
    );
  })
  .map(([key, englishValue]) => (
    `${key}: en=${JSON.stringify(extractPlaceholders(englishValue))}, zh=${JSON.stringify(extractPlaceholders(effectiveChineseGameCatalog.get(key)))}`
  ))
  .sort();

const missingEffectiveChineseMessages = [...effectiveEnglishCatalog.keys()]
  .filter((key) => !effectiveChineseCatalog.has(key))
  .sort();

const unreadableEffectiveChineseMessages = [...effectiveChineseCatalog]
  .filter(([, value]) => !/[\u3400-\u9fff]/u.test(value) && !isLanguageNeutralMessage(value))
  .map(([key, value]) => `${key} = ${JSON.stringify(value)}`)
  .sort();

const copiedEnglishMessages = [...effectiveEnglishCatalog]
  .filter(([key, value]) => (
    effectiveChineseCatalog.get(key) === value && !isLanguageNeutralMessage(value)
  ))
  .map(([key, value]) => `${key} = ${JSON.stringify(value)}`)
  .sort();

const corruptEffectiveChineseMessages = [...effectiveChineseCatalog]
  .filter(([, value]) => /\?{2,}|\uFFFD/u.test(value))
  .map(([key, value]) => `${key} = ${JSON.stringify(value)}`)
  .sort();

const mismatchedMessagePlaceholders = [...effectiveEnglishCatalog]
  .filter(([key, englishValue]) => {
    const chineseValue = effectiveChineseCatalog.get(key);
    return chineseValue !== undefined && (
      JSON.stringify(extractPlaceholders(englishValue)) !== JSON.stringify(extractPlaceholders(chineseValue))
    );
  })
  .map(([key, englishValue]) => (
    `${key}: en=${JSON.stringify(extractPlaceholders(englishValue))}, zh=${JSON.stringify(extractPlaceholders(effectiveChineseCatalog.get(key)))}`
  ))
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

if (
  missingChineseKeys.length > 0 ||
  missingEnglishKeys.length > 0 ||
  missingEnglishSchemaKeys.length > 0 ||
  missingChineseSchemaKeys.length > 0 ||
  missingEnglishGameCatalogImports.length > 0 ||
  missingChineseGameCatalogImports.length > 0 ||
  missingEffectiveChineseGameMessages.length > 0 ||
  extraEffectiveChineseGameMessages.length > 0 ||
  unreadableEffectiveChineseGameMessages.length > 0 ||
  copiedEnglishGameMessages.length > 0 ||
  corruptEffectiveChineseGameMessages.length > 0 ||
  mechanicalDontStarveMessages.length > 0 ||
  mismatchedGameMessagePlaceholders.length > 0 ||
  missingEffectiveChineseMessages.length > 0 ||
  unreadableEffectiveChineseMessages.length > 0 ||
  copiedEnglishMessages.length > 0 ||
  corruptEffectiveChineseMessages.length > 0 ||
  mismatchedMessagePlaceholders.length > 0 ||
  compatibilityFilterMarkers.length > 0
) {
  printMissing("Missing explicit zh-CN messages:", missingChineseKeys);
  printMissing("Missing en-US messages for translation references:", missingEnglishKeys);
  printMissing("Missing en-US schema messages:", missingEnglishSchemaKeys);
  printMissing("Missing zh-CN schema messages:", missingChineseSchemaKeys);
  printMissing("Game en-US catalog files not imported by src/i18n/games/en-us.ts:", missingEnglishGameCatalogImports);
  printMissing("Game zh-CN catalog files not imported by src/i18n/games/zh-cn.ts:", missingChineseGameCatalogImports);
  printMissing("Missing messages from the effective zh-CN game catalog:", missingEffectiveChineseGameMessages);
  printMissing("Unexpected messages in the effective zh-CN game catalog:", extraEffectiveChineseGameMessages);
  printMissing("Effective zh-CN game messages without readable Chinese:", unreadableEffectiveChineseGameMessages);
  printMissing("Effective zh-CN game messages copied from en-US:", copiedEnglishGameMessages);
  printMissing("Corrupt effective zh-CN game messages:", corruptEffectiveChineseGameMessages);
  printMissing("Mechanical Don't Starve Together zh-CN messages:", mechanicalDontStarveMessages);
  printMissing("Game message placeholder mismatches:", mismatchedGameMessagePlaceholders);
  printMissing("Missing messages from the final zh-CN catalog:", missingEffectiveChineseMessages);
  printMissing("Final zh-CN messages without readable Chinese:", unreadableEffectiveChineseMessages);
  printMissing("Final zh-CN messages copied from en-US:", copiedEnglishMessages);
  printMissing("Corrupt final zh-CN messages:", corruptEffectiveChineseMessages);
  printMissing("Final message placeholder mismatches:", mismatchedMessagePlaceholders);
  printMissing("Removed zh-CN compatibility filters were reintroduced:", compatibilityFilterMarkers);
  process.exitCode = 1;
} else {
  console.log(
    `i18n catalogs verified: ${englishKeys.size} en-US keys, ${explicitChineseKeys.size} explicit zh-CN keys, ${effectiveChineseCatalog.size} final zh-CN messages, ${effectiveChineseGameCatalog.size} effective zh-CN game messages, ${translationCalls.size} translation references, ${schemaTranslationKeys.length} schema keys.`
  );
}
