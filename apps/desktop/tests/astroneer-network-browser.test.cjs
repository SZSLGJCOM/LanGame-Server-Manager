const assert = require("node:assert/strict");
const test = require("node:test");
const path = require("node:path");
const fs = require("node:fs/promises");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`ASTRONEER compact network help at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const screenshotPath = process.env.LANGAME_ASTRO_NETWORK_SCREENSHOT_DIR
      ? path.join(process.env.LANGAME_ASTRO_NETWORK_SCREENSHOT_DIR, `astroneer-network-help-${viewport.width}x${viewport.height}.png`) : undefined;
    const report = await runBrowserFixture({ fixturePath: "astroneer-network-browser.html", viewport, screenshotPath,
      pointer: true, keyboard: true, fixtureCleanup: true });
    if (screenshotPath) await fs.writeFile(screenshotPath.replace(/\.png$/, ".json"), JSON.stringify(report, null, 2));
    assert.equal(report.status, "passed", report.error);
    assert.equal(report.creations.length, 2);
    assert.deepEqual(report.browser_errors, []);
    assert.deepEqual(report.fixture_cleanup.browser_errors, []);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.browser_exited, true);
    assert.equal(report.scratch_removed, true);
    console.log(`ASTRONEER_NETWORK_BROWSER ${JSON.stringify({ viewport, checks: report.checks.length })}`);
  });
}
