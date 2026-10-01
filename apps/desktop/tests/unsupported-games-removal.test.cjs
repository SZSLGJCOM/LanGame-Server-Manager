const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const repoRoot = path.resolve(__dirname, "..", "..", "..");

const activeSupportRoots = [
  "modules",
  path.join("apps", "desktop", "src"),
  path.join("apps", "desktop", "src-tauri", "src"),
  path.join("apps", "desktop", "public"),
  "crates",
  "scripts"
];

const unsupportedGames = [
  "armareforger",
  "factorio",
  "thefront",
  "noonesurvived",
  "colonysurvival",
  "avorion",
  "empyrion",
  "fivem",
  "redm",
  "eco",
  "counterstrike2",
  "mythofempires",
  "smalland",
  "longvinter",
  "craftopia",
  "sunkenland",
  "spaceengineers",
  "theisle",
  "garrysmod",
];

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

const unsupportedGameAliases = {
  armareforger: ["armareforger", "arma reforger", "arma_reforger", "arma-reforger"],
  thefront: ["thefront", "the front", "the_front", "the-front"],
  noonesurvived: ["noonesurvived", "no one survived", "no_one_survived", "no-one-survived"],
  colonysurvival: ["colonysurvival", "colony survival", "colony_survival", "colony-survival"],
  counterstrike2: ["counterstrike2", "counter strike 2", "counter-strike-2", "counter_strike_2", "cs2"],
  mythofempires: ["mythofempires", "myth of empires", "myth_of_empires", "myth-of-empires"],
  spaceengineers: ["spaceengineers", "space engineers", "space_engineers", "space-engineers"],
  theisle: ["theisle", "the isle", "the_isle", "the-isle"],
  garrysmod: ["garrysmod", "garry's mod", "garrys mod", "garry_mod", "garry-mod"],
};

function gameTokenPattern(gameId) {
  const aliases = unsupportedGameAliases[gameId] ?? [gameId];
  const alternatives = aliases.map(escapeRegExp).join("|");
  return new RegExp(`(^|[^a-z0-9])(?:${alternatives})($|[^a-z0-9])`, "i");
}

test("unsupported game matcher covers path-safe aliases", () => {
  assert.ok(gameTokenPattern("thefront").test("assets/covers/the_front_cover.webp"));
  assert.ok(gameTokenPattern("noonesurvived").test("assets/covers/no_one_survived_cover.webp"));
  assert.ok(gameTokenPattern("colonysurvival").test("assets/covers/colony_survival_cover.webp"));
  assert.ok(gameTokenPattern("counterstrike2").test("assets/maps/counter_strike_2_dust2.webp"));
  assert.ok(gameTokenPattern("mythofempires").test("assets/maps/myth_of_empires_zhongzhou.webp"));
  assert.ok(gameTokenPattern("spaceengineers").test("modules/space_engineers/schema.json"));
  assert.ok(gameTokenPattern("theisle").test("modules/the-isle/schema.json"));
  assert.ok(gameTokenPattern("garrysmod").test("modules/garrysmod/schema.json"));
});

const textFileExtensions = new Set([
  ".bat",
  ".cjs",
  ".css",
  ".html",
  ".hbs",
  ".json",
  ".md",
  ".ps1",
  ".py",
  ".rs",
  ".toml",
  ".ts",
  ".tsx",
  ".txt",
  ".xml",
  ".yml",
  ".yaml"
]);

function shouldScanFileContents(filePath) {
  return textFileExtensions.has(path.extname(filePath).toLowerCase());
}

function walkFiles(root) {
  if (!fs.existsSync(root)) {
    return [];
  }

  const entries = fs.readdirSync(root, { withFileTypes: true });
  const files = [];

  for (const entry of entries) {
    const fullPath = path.join(root, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "__pycache__") {
        continue;
      }
      files.push(...walkFiles(fullPath));
    } else if (entry.isFile()) {
      files.push(fullPath);
    }
  }

  return files;
}

test("unsupported games are absent from active support surfaces", () => {
  const violations = [];

  for (const gameId of unsupportedGames) {
    const gamePattern = gameTokenPattern(gameId);
    for (const root of activeSupportRoots) {
      const absoluteRoot = path.join(repoRoot, root);
      for (const filePath of walkFiles(absoluteRoot)) {
        const relativePath = path.relative(repoRoot, filePath).replaceAll(path.sep, "/");
        if (gamePattern.test(relativePath)) {
          violations.push(relativePath);
          continue;
        }

        if (!shouldScanFileContents(filePath)) {
          continue;
        }

        const contents = fs.readFileSync(filePath, "utf8");
        if (gamePattern.test(contents)) {
          violations.push(relativePath);
        }
      }
    }
  }

  assert.deepEqual(violations.sort(), []);
});
