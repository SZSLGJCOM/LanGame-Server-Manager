const fs = require("fs");
const path = require("path");
const { collectLocaleCatalogFiles } = require("./catalog-file-discovery.cjs");
const { collectTranslationReferences } = require("./i18n-source-references.cjs");

const DESKTOP_ROOT = path.resolve(__dirname, "..");
const WORKSPACE_ROOT = path.resolve(DESKTOP_ROOT, "../..");
const GAME_CATALOG_DIRECTORY = "src/i18n/games";

const ENGLISH_CATALOG_FILES = [
  "src/i18n-messages.ts",
  "src/i18n-messages-en-extra.ts",
  ...collectLocaleCatalogFiles(DESKTOP_ROOT, GAME_CATALOG_DIRECTORY, "en")
];

const EXPLICIT_CHINESE_CATALOG_FILES = [
  "src/i18n-messages-zh-core.ts",
  "src/i18n-messages-zh-ui.ts",
  "src/i18n-messages-zh-extra.ts",
  "src/i18n-messages-zh-settings.ts",
  "src/i18n-messages-zh-schema.ts",
  ...collectLocaleCatalogFiles(DESKTOP_ROOT, GAME_CATALOG_DIRECTORY, "zh-cn")
];

const TEXT_SOURCE_EXTENSIONS = new Set([
  ".cjs",
  ".json",
  ".md",
  ".rs",
  ".toml",
  ".ts",
  ".tsx"
]);

const REPLACEMENT_CHARACTER_ALLOWLIST = new Set([
  "apps/desktop/src/store-media.ts"
]);

function readDesktopFile(relativePath) {
  return fs.readFileSync(path.join(DESKTOP_ROOT, relativePath), "utf8");
}

function readWorkspaceFile(relativePath) {
  return fs.readFileSync(path.join(WORKSPACE_ROOT, relativePath), "utf8");
}

function extractCatalogKeys(relativePath) {
  const source = readDesktopFile(relativePath);
  const keys = new Set();
  const keyPattern = /["']([A-Za-z0-9_.-]+(?:\.[A-Za-z0-9_.-]+)+)["']\s*:/g;
  let match;

  while ((match = keyPattern.exec(source)) !== null) {
    keys.add(match[1]);
  }

  return keys;
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

function walk(root, relativeDirectory, options = {}) {
  const absoluteDirectory = path.join(root, relativeDirectory);
  if (!fs.existsSync(absoluteDirectory)) {
    return [];
  }

  const entries = fs.readdirSync(absoluteDirectory, { withFileTypes: true });
  const files = [];
  const ignoredNames = new Set(options.ignoredNames ?? []);

  for (const entry of entries) {
    if (ignoredNames.has(entry.name)) {
      continue;
    }

    const relativePath = path.join(relativeDirectory, entry.name).replace(/\\/g, "/");
    if (entry.isDirectory()) {
      files.push(...walk(root, relativePath, options));
      continue;
    }

    if (!options.extensions || options.extensions.has(path.extname(entry.name))) {
      files.push(relativePath);
    }
  }

  return files;
}

function collectSourceTranslationReferences() {
  const sourceFiles = walk(DESKTOP_ROOT, "src", {
    ignoredNames: ["node_modules", "dist"],
    extensions: new Set([".ts", ".tsx"])
  });
  const calls = new Map();

  for (const file of sourceFiles) {
    const source = readDesktopFile(file);
    for (const [key, locations] of collectTranslationReferences(source, file)) {
      calls.set(key, [...(calls.get(key) ?? []), ...locations]);
    }
  }

  return calls;
}

function countMatches(source, pattern) {
  return (source.match(pattern) ?? []).length;
}

function collectFrontendBypassSignals() {
  const sourceFiles = walk(DESKTOP_ROOT, "src", {
    ignoredNames: ["node_modules", "dist", "data"],
    extensions: new Set([".ts", ".tsx"])
  }).filter((file) => {
    if (/\.css$/.test(file)) {
      return false;
    }
    if (/^src\/i18n-messages/.test(file)) {
      return false;
    }
    if (/^src\/i18n\//.test(file)) {
      return false;
    }
    if (file === "src/store-copy.ts") {
      return false;
    }
    return true;
  });

  return sourceFiles
    .map((file) => {
      const source = readDesktopFile(file);
      const localeBranch = countMatches(
        source,
        /\bisZh\b|locale\s*===\s*["']zh-CN["']|startsWith\(["']zh["']\)/g
      );
      const localizeText = countMatches(source, /localizeText\(/g);
      const literalTernary = countMatches(
        source,
        /\?\s*["'`][^"'`]*[\u3400-\u9fff][^"'`]*["'`]\s*:/g
      );
      const inlineZhObject = countMatches(source, /\bzh\s*:\s*["'`]/g);
      const inlineEnObject = countMatches(source, /\ben\s*:\s*["'`]/g);

      return {
        file,
        localeBranch,
        localizeText,
        literalTernary,
        inlineZhObject,
        inlineEnObject,
        total: localeBranch + localizeText + literalTernary + inlineZhObject + inlineEnObject
      };
    })
    .filter((entry) => entry.total > 0)
    .sort((left, right) => right.total - left.total);
}

function collectSchemaCoverage(chineseCatalogKeys) {
  const modulesRoot = path.join(WORKSPACE_ROOT, "modules");
  if (!fs.existsSync(modulesRoot)) {
    return [];
  }

  return fs
    .readdirSync(modulesRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort()
    .flatMap((moduleId) => {
      const schemaPath = path.join("modules", moduleId, "schema.json").replace(/\\/g, "/");
      const absoluteSchemaPath = path.join(WORKSPACE_ROOT, schemaPath);
      if (!fs.existsSync(absoluteSchemaPath)) {
        return [];
      }

      const schema = JSON.parse(readWorkspaceFile(schemaPath));
      const properties = schema.properties && typeof schema.properties === "object" ? schema.properties : {};
      const keys = Object.keys(properties).filter((key) => key !== "bind_ip");
      let descriptions = 0;
      let missingTitle = 0;
      let missingDescription = 0;

      for (const key of keys) {
        if (!chineseCatalogKeys.has(`settings.schema.${moduleId}.${key}.title`)) {
          missingTitle += 1;
        }
        if (typeof properties[key]?.description === "string") {
          descriptions += 1;
          if (!chineseCatalogKeys.has(`settings.schema.${moduleId}.${key}.description`)) {
            missingDescription += 1;
          }
        }
      }

      return [{
        moduleId,
        fields: keys.length,
        descriptions,
        missingTitle,
        missingDescription
      }];
    });
}

function collectReplacementCharacters() {
  const files = [
    ...walk(DESKTOP_ROOT, "src", {
      ignoredNames: ["node_modules", "dist"],
      extensions: TEXT_SOURCE_EXTENSIONS
    }).map((file) => path.relative(WORKSPACE_ROOT, path.join(DESKTOP_ROOT, file)).replace(/\\/g, "/")),
    ...walk(WORKSPACE_ROOT, "crates", {
      ignoredNames: ["target"],
      extensions: TEXT_SOURCE_EXTENSIONS
    }),
    ...walk(WORKSPACE_ROOT, "modules", {
      ignoredNames: ["target"],
      extensions: TEXT_SOURCE_EXTENSIONS
    })
  ];

  return files
    .map((file) => {
      const source = readWorkspaceFile(file);
      let count = 0;
      for (const char of source) {
        if (char.charCodeAt(0) === 0xfffd) {
          count += 1;
        }
      }
      return { file, count };
    })
    .filter((entry) => entry.count > 0 && !REPLACEMENT_CHARACTER_ALLOWLIST.has(entry.file));
}

function printTop(title, entries, formatter, limit = 20) {
  console.log(`\n${title}`);
  if (entries.length === 0) {
    console.log("  none");
    return;
  }

  for (const entry of entries.slice(0, limit)) {
    console.log(`  ${formatter(entry)}`);
  }
  if (entries.length > limit) {
    console.log(`  ...and ${entries.length - limit} more`);
  }
}

const englishKeys = collectCatalogKeys(ENGLISH_CATALOG_FILES);
const explicitChineseKeys = collectCatalogKeys(EXPLICIT_CHINESE_CATALOG_FILES);
const translationCalls = collectSourceTranslationReferences();
const missingChineseKeys = [...englishKeys].filter((key) => !explicitChineseKeys.has(key)).sort();
const missingEnglishKeys = [...translationCalls.keys()].filter((key) => !englishKeys.has(key)).sort();
const bypassSignals = collectFrontendBypassSignals();
const schemaCoverage = collectSchemaCoverage(explicitChineseKeys);
const replacementCharacters = collectReplacementCharacters();

console.log("i18n surface audit");
console.log(`  en-US catalog keys: ${englishKeys.size}`);
console.log(`  explicit zh-CN catalog keys: ${explicitChineseKeys.size}`);
console.log(`  translation references: ${translationCalls.size}`);
console.log(`  missing explicit zh-CN catalog keys: ${missingChineseKeys.length}`);
console.log(`  missing en-US keys for translation references: ${missingEnglishKeys.length}`);
console.log(`  frontend files with locale-branch bypasses: ${bypassSignals.length}`);
console.log(`  files with U+FFFD replacement characters: ${replacementCharacters.length}`);

printTop(
  "Missing en-US keys for translation references",
  missingEnglishKeys,
  (key) => `${key} (${translationCalls.get(key).join(", ")})`
);

printTop(
  "Missing explicit zh-CN catalog keys",
  missingChineseKeys,
  (key) => key
);

printTop(
  "Top frontend locale bypasses",
  bypassSignals,
  (entry) =>
    `${entry.total}  ${entry.file}  locale=${entry.localeBranch} localizeText=${entry.localizeText} ternary=${entry.literalTernary} zh/enObject=${entry.inlineZhObject}/${entry.inlineEnObject}`
);

printTop(
  "Module schema zh-CN catalog gaps",
  schemaCoverage
    .filter((entry) => entry.missingTitle > 0 || entry.missingDescription > 0)
    .sort((left, right) => (right.missingTitle + right.missingDescription) - (left.missingTitle + left.missingDescription)),
  (entry) =>
    `${entry.moduleId}: fields=${entry.fields}, title gaps=${entry.missingTitle}, description gaps=${entry.missingDescription}/${entry.descriptions}`
);

printTop(
  "Replacement-character findings",
  replacementCharacters,
  (entry) => `${entry.file}: ${entry.count}`
);
