const fs = require("node:fs");
const path = require("node:path");
const { transpileTypeScript } = require("./typescript_source_tools.cjs");

const DESKTOP_ROOT = path.resolve(__dirname, "..");

// Load repository-owned catalog modules through their real imports and object
// spreads, including computed messages. Restore the caller's TypeScript hook.
function loadCatalog(relativePath, exportName) {
  const previous = require.extensions[".ts"];
  let exported;
  try {
    require.extensions[".ts"] = (module, filename) => {
      module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
    };
    exported = require(path.resolve(DESKTOP_ROOT, relativePath));
  } finally {
    if (previous) require.extensions[".ts"] = previous;
    else delete require.extensions[".ts"];
  }
  const names = Object.keys(exported);
  if (!exportName && names.length !== 1) {
    throw new Error(`Catalog ${relativePath} requires an explicit export name`);
  }
  const catalog = exported[exportName ?? names[0]];
  if (!catalog || typeof catalog !== "object" || Array.isArray(catalog)) {
    throw new TypeError(`Catalog ${relativePath} does not export a message object`);
  }
  const entries = Object.entries(catalog);
  for (const [key, value] of entries) {
    if (typeof value !== "string") {
      throw new TypeError(`Catalog ${relativePath}: ${key} must be a string`);
    }
  }
  return new Map(entries);
}

function loadEffectiveCatalogs() {
  return {
    english: loadCatalog("src/i18n-messages.ts", "EN_US_MESSAGES"),
    chinese: loadCatalog("src/i18n-messages-zh-cn.ts", "ZH_CN_MESSAGES"),
    englishGames: loadCatalog("src/i18n/games/en-us.ts", "EN_US_GAME_MESSAGES"),
    chineseGames: loadCatalog("src/i18n/games/zh-cn.ts", "ZH_CN_GAME_MESSAGES")
  };
}

module.exports = { loadCatalog, loadEffectiveCatalogs };
