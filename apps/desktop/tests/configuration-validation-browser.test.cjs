const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`configuration validation stays in activity with direct navigation at ${viewport.width} x ${viewport.height}`,
    { timeout: 90000 }, async () => {
      const report = await runBrowserFixture({ fixturePath: "configuration-validation-browser.html", keyboard: true, viewport, fixtureCleanup: true,
        screenshotPath: process.env.LANGAME_CONFIGURATION_VALIDATION_SCREENSHOT_DIR
          ? path.join(process.env.LANGAME_CONFIGURATION_VALIDATION_SCREENSHOT_DIR, `configuration-validation-${viewport.width}x${viewport.height}.png`)
          : undefined });
      // The shared runner verifies fixture success, browser errors and cleanup.
      assert.equal(report.checks, 9);
      assert.ok(report.saves > 0);
      console.log(`CONFIGURATION_VALIDATION_BROWSER ${JSON.stringify(report)}`);
    });
}
