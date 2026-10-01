const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("ARK maintenance preserves explicit recovery scope, operation ownership and cleanup warnings", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "ark-cluster-maintenance-browser.html",
    viewport: { width: 1560, height: 900 }, screenshotPath: process.env.LANGAME_ARK_MAINTENANCE_SCREENSHOT });
  assert.equal(report.status, "passed");
  assert.equal(report.checks, 10);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`ARK_MAINTENANCE_BROWSER ${JSON.stringify(report)}`);
});
