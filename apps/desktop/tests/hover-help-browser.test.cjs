const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("hover help follows real mouse intent, keyboard focus and interaction lifecycle", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "hover-help-browser.html", keyboard: true, pointer: true,
    viewport: { width: 960, height: 720 }, fixtureCleanup: true,
    screenshotPath: process.env.LANGAME_HOVER_HELP_SCREENSHOT });
  assert.equal(report.status, "passed");
  assert.equal(report.checks, 23);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`HOVER_HELP_BROWSER ${JSON.stringify(report)}`);
});
