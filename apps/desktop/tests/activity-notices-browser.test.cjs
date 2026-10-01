const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 1280, height: 900 }, { width: 1366, height: 768 }, { width: 960, height: 600 }]) {
  test(`global notices stay in the activity bar at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "activity-notices-browser.html", keyboard: true, viewport, fixtureCleanup: true,
      screenshotPath: viewport.width === 1560 ? process.env.LANGAME_ACTIVITY_NOTICES_SCREENSHOT : undefined });
    assert.equal(report.status, "passed");
    assert.equal(report.scenarios, 24);
    assert.equal(report.create_calls, 1);
    assert.equal(report.update_retries, 1);
    assert.equal(report.local_retries, 1);
    assert.equal(report.local_dismissals, 1);
    assert.equal(report.resumes, 3);
    assert.equal(report.start_attempts, 1);
    assert.deepEqual(report.installation_stops, ["fixture-installation"]);
    assert.deepEqual(report.steamcmd_stops, ["fixture-steamcmd"]);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`ACTIVITY_NOTICES_BROWSER ${JSON.stringify({ viewport, ...report })}`);
  });
}
