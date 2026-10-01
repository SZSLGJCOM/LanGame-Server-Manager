const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("configuration fields align controls across mixed rows and retain input behavior", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "configuration-field-alignment-browser.html", keyboard: true,
    viewport: { width: 1280, height: 1000 }, fixtureCleanup: true,
    screenshotPath: process.env.LANGAME_ALIGNMENT_SCREENSHOT });
  assert.equal(report.status, "passed", report.error);
  assert.equal(report.checks, 19);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`CONFIGURATION_FIELD_ALIGNMENT_BROWSER ${JSON.stringify(report)}`);
});
