const assert = require("node:assert/strict");
const { execFileSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const { collectLocaleCatalogFiles } = require("../scripts/catalog-file-discovery.cjs");

const desktopRoot = path.resolve(__dirname, "..");
const catalogDirectory = "src/i18n/games";

test("locale catalog discovery includes base, part, and generated chunk files", () => {
  const englishFiles = collectLocaleCatalogFiles(desktopRoot, catalogDirectory, "en");
  const chineseFiles = collectLocaleCatalogFiles(desktopRoot, catalogDirectory, "zh-cn");

  assert.ok(englishFiles.includes("src/i18n/games/scum.en.ts"));
  assert.ok(englishFiles.includes("src/i18n/games/arksurvivalascended.en.part1.ts"));
  assert.ok(englishFiles.includes("src/i18n/games/schema-generated.en.chunk-01.ts"));
  assert.ok(chineseFiles.includes("src/i18n/games/arksurvivalascended.zh-cn.part1.ts"));
  assert.ok(chineseFiles.includes("src/i18n/games/schema-generated.zh-cn.chunk-01.ts"));
});

test("i18n surface audit resolves translation references from split catalogs", () => {
  const output = execFileSync(process.execPath, ["scripts/audit_i18n_surface.cjs"], {
    cwd: desktopRoot,
    encoding: "utf8"
  });

  assert.match(output, /missing en-US keys for translation references: 0/u);
  assert.match(output, /missing explicit zh-CN catalog keys: 0/u);
  assert.doesNotMatch(output, /scum-native-messages\.ts/u);
});

test("catalogs do not retain copy owned only by removed frontend surfaces", () => {
  const catalogFiles = [
    "src/i18n-messages.ts",
    "src/i18n-messages-en-extra.ts",
    "src/i18n-messages-zh-ui.ts",
    ...collectLocaleCatalogFiles(desktopRoot, catalogDirectory, "en"),
    ...collectLocaleCatalogFiles(desktopRoot, catalogDirectory, "zh-cn")
  ];
  const catalogs = catalogFiles
    .map((file) => fs.readFileSync(path.join(desktopRoot, file), "utf8"))
    .join("\n");

  for (const removedPrefix of [
    "overview.quick.",
    "overview.stats.",
    "overview.summary.",
    "maintenance.instances.",
    "maintenance.metrics.",
    "maintenance.storage.",
    "dst.settings.guide.",
    "settings.dstSync."
  ]) {
    assert.doesNotMatch(catalogs, new RegExp(`['\"]${removedPrefix.replaceAll(".", "\\.")}`, "u"));
  }

  const removedExactKeys = [
    "systemSettings.title",
    "placeholder.migrating",
    "dst.settings.access.cluster_token",
    "dst.settings.world.caves_atriumgate",
    "dst.settings.world.caves_day",
    "dst.settings.world.caves_regrowth",
    "dst.settings.world.caves_world_size",
    "dst.settings.world.caves_wormattacks",
    "dst.settings.world.master_hounds",
    "dst.settings.world.master_world_size",
    "dst.settings.world.world_autumn",
    "dst.settings.world.world_specialevent",
    "servers.dst.accessListsTitle",
    "servers.dst.cavesMods",
    "servers.dst.masterMods",
    "servers.dst.roomName",
    "servers.dst.sharedMods",
    "servers.dst.visibility",
    "servers.dst.visibilityLan",
    "servers.dst.visibilityOffline",
    "servers.dst.visibilityPublic",
    "servers.dst.worldRoot"
  ];
  for (const key of removedExactKeys) {
    assert.doesNotMatch(catalogs, new RegExp(`['\"]${key.replaceAll(".", "\\.")}['\"]`, "u"));
  }
});
