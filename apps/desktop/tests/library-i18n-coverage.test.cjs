const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const root = path.resolve(__dirname, "..", "..", "..");
const modulesDir = path.join(root, "modules");
const desktopSrcDir = path.join(root, "apps", "desktop", "src");
const storeCopySource = fs.readFileSync(path.join(desktopSrcDir, "store-copy.ts"), "utf8");

function readModuleIds() {
  return fs.readdirSync(modulesDir)
    .filter((name) => fs.existsSync(path.join(modulesDir, name, "module.toml")))
    .filter((name) => fs.existsSync(path.join(modulesDir, name, "schema.json")))
    .sort();
}

function readRecordKeys(source, recordName) {
  const match = source.match(
    new RegExp(String.raw`(?:export\s+)?const\s+${recordName}\s*:\s*Record<string,\s*[^>]+>\s*=\s*\{([\s\S]*?)\n\};`)
  );
  assert.ok(match, `${recordName} record is missing`);
  return Array.from(match[1].matchAll(/^\s{2}([a-z0-9]+):\s*\{/gm), (item) => item[1]).sort();
}

function uniqueSorted(values) {
  return [...new Set(values)].sort();
}

function storeCopyKeys(localePrefix) {
  return uniqueSorted([
    ...readRecordKeys(storeCopySource, `${localePrefix}_MODULE_STORE_COPY`),
    ...readRecordKeys(storeCopySource, `${localePrefix}_RECENT_MODULE_STORE_COPY`)
  ]);
}

function readStoreCopyNames(recordName) {
  const entries = new Map();
  const startPattern = new RegExp(String.raw`(?:export\s+)?const\s+${recordName}\s*:\s*Record<string,\s*[^>]+>\s*=\s*\{`, "m");
  const startMatch = startPattern.exec(storeCopySource);
  assert.ok(startMatch, `${recordName} record is missing`);

  const startIndex = startMatch.index + startMatch[0].length;
  const endIndex = storeCopySource.indexOf("\n};", startIndex);
  assert.notEqual(endIndex, -1, `${recordName} record is not closed`);

  const source = storeCopySource.slice(startIndex, endIndex);
  const entryPattern = /^\s{2}([a-z0-9]+):\s*\{[\s\S]*?storeName:\s*"((?:\\.|[^"\\])*)"/gm;
  let match;

  while ((match = entryPattern.exec(source)) !== null) {
    entries.set(match[1], JSON.parse(`"${match[2]}"`));
  }

  return entries;
}

function readStoreCopyShortDescriptions(recordName) {
  const entries = new Map();
  const startPattern = new RegExp(String.raw`(?:export\s+)?const\s+${recordName}\s*:\s*Record<string,\s*[^>]+>\s*=\s*\{`, "m");
  const startMatch = startPattern.exec(storeCopySource);
  assert.ok(startMatch, `${recordName} record is missing`);

  const startIndex = startMatch.index + startMatch[0].length;
  const endIndex = storeCopySource.indexOf("\n};", startIndex);
  assert.notEqual(endIndex, -1, `${recordName} record is not closed`);

  const source = storeCopySource.slice(startIndex, endIndex);
  const entryPattern = /^\s{2}([a-z0-9]+):\s*\{[\s\S]*?shortDescription:\s*\n?\s*"((?:\\.|[^"\\])*)"/gm;
  let match;

  while ((match = entryPattern.exec(source)) !== null) {
    entries.set(match[1], JSON.parse(`"${match[2]}"`));
  }

  return entries;
}

function storeCopyNames(localePrefix) {
  const names = new Map([
    ...readStoreCopyNames(`${localePrefix}_MODULE_STORE_COPY`),
    ...readStoreCopyNames(`${localePrefix}_RECENT_MODULE_STORE_COPY`)
  ]);

  if (localePrefix === "ZH_CN") {
    for (const [moduleId, storeName] of readStringRecord("ZH_CN_MODULE_DISPLAY_NAME_OVERRIDES")) {
      names.set(moduleId, storeName);
    }
  }

  return names;
}

function readStringRecord(recordName) {
  const entries = new Map();
  const startPattern = new RegExp(String.raw`const\s+${recordName}\s*:\s*Record<string,\s*string>\s*=\s*\{`, "m");
  const startMatch = startPattern.exec(storeCopySource);

  if (!startMatch) {
    return entries;
  }

  const startIndex = startMatch.index + startMatch[0].length;
  const endIndex = storeCopySource.indexOf("\n};", startIndex);
  assert.notEqual(endIndex, -1, `${recordName} record is not closed`);

  const source = storeCopySource.slice(startIndex, endIndex);
  const entryPattern = /^\s{2}([a-z0-9]+):\s*"((?:\\.|[^"\\])*)"/gm;
  let match;

  while ((match = entryPattern.exec(source)) !== null) {
    entries.set(match[1], JSON.parse(`"${match[2]}"`));
  }

  return entries;
}

test("localized game library copy covers every real module", () => {
  const moduleIds = readModuleIds();

  assert.deepEqual(storeCopyKeys("EN_US"), moduleIds);
  assert.deepEqual(storeCopyKeys("ZH_CN"), moduleIds);
});

test("zh-CN game library display names are localized", () => {
  const moduleIds = readModuleIds();
  const zhNames = storeCopyNames("ZH_CN");
  const untranslatedNames = moduleIds
    .filter((moduleId) => !/^[\u3400-\u9fff]/u.test(zhNames.get(moduleId) ?? ""))
    .map((moduleId) => `${moduleId}: ${zhNames.get(moduleId) ?? "<missing>"}`);

  assert.deepEqual(untranslatedNames, []);
});

test("zh-CN game library display names stay concise", () => {
  const moduleIds = readModuleIds();
  const zhNames = storeCopyNames("ZH_CN");
  const bloatedNames = moduleIds
    .filter((moduleId) => /社区服务器|专用服务器|Dedicated Server/i.test(zhNames.get(moduleId) ?? ""))
    .map((moduleId) => `${moduleId}: ${zhNames.get(moduleId) ?? "<missing>"}`);

  assert.deepEqual(bloatedNames, []);
});

test("game library short descriptions avoid hosting operations copy", () => {
  const records = [
    "EN_US_MODULE_STORE_COPY",
    "EN_US_RECENT_MODULE_STORE_COPY",
    "ZH_CN_MODULE_STORE_COPY",
    "ZH_CN_RECENT_MODULE_STORE_COPY"
  ];
  const hostingCopyPattern = /dedicated server|server\.jar|RCON|SteamCMD|Steam query|专服|专用服务器|开服|服务器运营|端口|托管|运维/i;
  const polluted = records.flatMap((recordName) => {
    const descriptions = readStoreCopyShortDescriptions(recordName);
    return [...descriptions]
      .filter(([, description]) => hostingCopyPattern.test(description))
      .map(([moduleId, description]) => `${recordName}.${moduleId}: ${description}`);
  });

  assert.deepEqual(polluted, []);
});
