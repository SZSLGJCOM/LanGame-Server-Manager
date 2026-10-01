const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const Module = require("node:module");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const modelPath = path.resolve(__dirname, "..", "src", "app-update-model.ts");
const typesPath = path.resolve(__dirname, "..", "src", "types.ts");

test("manual update downloads open the discovered version's official release page", () => {
  const loaded = new Module(modelPath, module);
  loaded._compile(transpileTypeScript(fs.readFileSync(modelPath, "utf8"), modelPath), modelPath);
  const { appUpdateReleaseUrl } = loaded.exports;
  assert.equal(appUpdateReleaseUrl("0.2.0"),
    "https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/tag/v0.2.0");
  for (const version of [undefined, null, "", "0.2.0\n", "v0.2.0", "01.2.0", "0.2.0-beta.1",
    "../latest", "0.2.0?redirect=example.org", "https://example.org", "9".repeat(65) + ".0.0"]) {
    assert.equal(appUpdateReleaseUrl(version), null, `Unexpected download link for ${String(version)}`);
  }
});

test("app update model defines product update states separate from server updates", () => {
  const modelSource = fs.readFileSync(modelPath, "utf8");
  const typesSource = fs.readFileSync(typesPath, "utf8");

  assert.match(typesSource, /interface AppUpdateMetadata/);
  assert.match(typesSource, /type AppUpdateStateStatus/);
  assert.match(modelSource, /export function createInitialAppUpdateState/);
  assert.match(modelSource, /export function reduceAppUpdateInstallEvent/);
  assert.doesNotMatch(modelSource, /SteamCMD/);
  assert.doesNotMatch(modelSource, /server files/i);
});

test("app update model tracks download totals from updater events", () => {
  const modelSource = fs.readFileSync(modelPath, "utf8");

  assert.match(modelSource, /content_length/);
  assert.match(modelSource, /downloadedBytes/);
  assert.match(modelSource, /downloadPercent/);
});
