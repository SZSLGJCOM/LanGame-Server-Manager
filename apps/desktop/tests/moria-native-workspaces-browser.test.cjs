const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`Moria permissions and world maintenance save safely at ${viewport.width}×${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({
      fixturePath: "moria-native-workspaces-browser.html", viewport, fixtureCleanup: true,
      contentSecurityPolicy: "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src 'self'",
      screenshotPath: process.env.LANGAME_CONFIGURATION_REVIEW_DIR
        ? path.join(process.env.LANGAME_CONFIGURATION_REVIEW_DIR, `moria-workspaces-${viewport.width}x${viewport.height}.png`) : undefined
    });
    assert.equal(report.status, "passed", report.error);
    assert.equal(report.cases.length, 6);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`MORIA_NATIVE_WORKSPACES_BROWSER ${JSON.stringify(report)}`);
  });
}
