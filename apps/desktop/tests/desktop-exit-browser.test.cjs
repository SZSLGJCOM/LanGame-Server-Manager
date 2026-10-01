const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`final desktop exit stops UI work and remains visible at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const screenshotPath = process.env.LANGAME_EXIT_SCREENSHOT_DIR
      ? path.join(process.env.LANGAME_EXIT_SCREENSHOT_DIR, `desktop-exit-${viewport.width}x${viewport.height}.png`) : undefined;
    const report = await runBrowserFixture({ fixturePath: "desktop-exit-browser.html", viewport, screenshotPath, fixtureCleanup: true });
    assert.equal(report.status, "passed", JSON.stringify(report));
    assert.deepEqual(report.cases, ["initial-admission", "real-server-workspace", "exit-unmounts-and-rejects-stale-read",
      "repeat-and-remount", "missed-event-recovery", "locales-themes-and-layout"]);
    assert.equal(report.active_intervals, 0);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`DESKTOP_EXIT_BROWSER ${JSON.stringify({ viewport, ...report })}`);
  });
}
