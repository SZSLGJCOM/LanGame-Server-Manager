const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("ARK local validation stays in activity and reveals retained drafts inside collapsed editors", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "ark-configuration-validation-browser.html", keyboard: true,
    viewport: { width: 1440, height: 900 }, screenshotPath: process.env.LANGAME_ARK_VALIDATION_SCREENSHOT, fixtureCleanup: true });
  // The shared runner verifies fixture success, browser errors and cleanup.
  assert.equal(report.csv_editors, 2);
  assert.ok(report.checks >= 40);
  console.log(`ARK_VALIDATION_BROWSER ${JSON.stringify(report)}`);
});
