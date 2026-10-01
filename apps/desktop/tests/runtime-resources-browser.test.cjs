const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");
test("resource limits support validated saves, restart readback and applied values in a real browser", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "runtime-resources-browser.html", keyboard: true,
    viewport: { width: 1280, height: 900 }, screenshotPath: process.env.LANGAME_RESOURCE_SCREENSHOT });
  assert.equal(report.status, "passed");
  assert.equal(report.checks, 9);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`RESOURCE_LIMITS_BROWSER ${JSON.stringify(report)}`);
});
