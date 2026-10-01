const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("Satisfactory configuration preserves unmanaged native options across editing and reopening", { timeout: 90000 }, async () => {
  const readbackRoot = process.env.LANGAME_SATISFACTORY_READBACK_DIR;
  const readbacks = readbackRoot ? Object.fromEntries(["created", "unmanaged", "managed", "released"].map((stage) => {
    const file = path.join(readbackRoot, `satisfactory-${stage}.json`);
    assert.ok(fs.statSync(file).size < 1024 * 1024, "Synthetic readback must be bounded");
    const details = JSON.parse(fs.readFileSync(file, "utf8"));
    assert.equal(details.summary.module_id, "satisfactory");
    assert.equal(typeof details.settings_json, "string");
    return [stage, details];
  })) : null;
  const report = await runBrowserFixture({ fixturePath: "satisfactory-native-options-browser.html", viewport: { width: 1280, height: 900 },
    fixtureMiddleware(request, response, next) {
      if (request.url !== "/__satisfactory_readbacks") return next();
      if (request.method !== "GET") { response.writeHead(405).end(); return; }
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify(readbacks));
    }
  });
  assert.equal(report.status, "passed", report.error);
  assert.ok(report.checks.length >= 45, "Native option acceptance did not finish");
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.browser_exited, true);
  assert.equal(report.scratch_removed, true);
  assert.equal(report.input_source, readbackRoot ? "storage-readback" : "storage-contract");
  console.log(`SATISFACTORY_NATIVE_OPTIONS_BROWSER ${JSON.stringify({ status: report.status, input_source: report.input_source, checks: report.checks.length })}`);
});
