const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const Module = require("node:module");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const modelPath = path.resolve(__dirname, "..", "src", "app-update-model.ts");

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
