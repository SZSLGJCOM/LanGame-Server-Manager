const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const desktopRoot = path.resolve(__dirname, "..");
const tauriConfig = JSON.parse(
  fs.readFileSync(path.join(desktopRoot, "src-tauri", "tauri.conf.json"), "utf8")
);

test("desktop bundle uses a release identifier", () => {
  assert.equal(tauriConfig.identifier, "cn.langame.servermanager");
  assert.doesNotMatch(tauriConfig.identifier, /rewrite|test|dev|temp/i);
});

test("desktop webview has an explicit fail-closed content security policy", () => {
  const csp = tauriConfig.app?.security?.csp;

  assert.equal(typeof csp, "object");
  assert.match(csp["default-src"], /'self'/);
  assert.equal(csp["base-uri"], "'none'");
  assert.equal(csp["form-action"], "'none'");
  assert.equal(csp["frame-src"], "'none'");
  assert.equal(csp["object-src"], "'none'");
  assert.equal(csp["script-src"], "'self'");
  assert.equal(csp["worker-src"], "'self' blob:", "HLS decoding must be allowed to use its isolated worker");
  assert.doesNotMatch(JSON.stringify(csp), /unsafe-eval/i);
});
