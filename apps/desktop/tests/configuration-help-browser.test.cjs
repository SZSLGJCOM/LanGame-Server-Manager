const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("configuration help remains readable, dismissible and scoped to the nearest field", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "configuration-help-browser.html", keyboard: true,
    viewport: { width: 960, height: 600 }, fixtureCleanup: true,
    screenshotPath: process.env.LANGAME_CONFIGURATION_HELP_SCREENSHOT });
  assert.equal(report.status, "passed");
  assert.equal(report.checks, 8);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`CONFIGURATION_HELP_BROWSER ${JSON.stringify(report)}`);
});
