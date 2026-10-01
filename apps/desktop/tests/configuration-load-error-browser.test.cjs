const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`configuration failures remain recoverable at ${viewport.width} x ${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "configuration-load-error-browser.html", keyboard: true, viewport,
      screenshotPath: process.env.LANGAME_CONFIGURATION_ERROR_SCREENSHOT_DIR
        ? path.join(process.env.LANGAME_CONFIGURATION_ERROR_SCREENSHOT_DIR, `configuration-error-${viewport.width}x${viewport.height}.png`)
        : undefined });
    assert.equal(report.status, "passed");
    assert.equal(report.checks, 8);
    assert.equal(report.retry_count, 2);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`CONFIGURATION_ERROR_BROWSER ${JSON.stringify(report)}`);
  });
}
