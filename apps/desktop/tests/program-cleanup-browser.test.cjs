const assert = require("node:assert/strict");
const test = require("node:test");
const path = require("node:path");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 960, height: 600 }, { width: 1440, height: 900 }]) {
  test(`library cleanup and retained programs at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "program-cleanup-browser.html", viewport, fixtureCleanup: true,
      screenshotPath: process.env.LANGAME_PROGRAM_CLEANUP_SCREENSHOT_DIR
        ? path.join(process.env.LANGAME_PROGRAM_CLEANUP_SCREENSHOT_DIR, `program-cleanup-${viewport.width}x${viewport.height}.png`) : undefined });
    assert.equal(report.status, "passed");
    assert.equal(report.uninstall_requests, 5);
    assert.deepEqual(report.browser_errors, []);
    assert.deepEqual(report.fixture_cleanup.browser_errors, []);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.browser_exited, true);
    assert.equal(report.scratch_removed, true);
    console.log(`PROGRAM_CLEANUP_BROWSER ${JSON.stringify(report)}`);
  });
}
