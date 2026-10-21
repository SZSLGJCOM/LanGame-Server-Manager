const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1280, height: 800 }, { width: 960, height: 600 }]) {
  test(`daily maintenance edits remain scoped at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "daily-maintenance-browser.html", viewport,
      screenshotPath: process.env.LANGAME_TOOLS_SCREENSHOT_DIR ? path.join(process.env.LANGAME_TOOLS_SCREENSHOT_DIR, `daily-maintenance-${viewport.width}x${viewport.height}.png`) : undefined });
    assert.equal(report.status, "passed", report.error);
    assert.ok(report.checks >= 30);
    assert.equal(report.instance_isolation, true);
    assert.equal(report.read_only_isolation, true);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`DAILY_MAINTENANCE_BROWSER ${JSON.stringify(report)}`);
  });
}
