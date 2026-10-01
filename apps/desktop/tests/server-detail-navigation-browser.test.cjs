const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`server detail navigation survives instance and archive card changes at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "server-detail-navigation-browser.html", viewport,
      screenshotPath: process.env.LANGAME_DETAIL_NAVIGATION_SCREENSHOT_DIR
        ? path.join(process.env.LANGAME_DETAIL_NAVIGATION_SCREENSHOT_DIR, `server-detail-navigation-${viewport.width}x${viewport.height}.png`) : undefined });
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.normal_tabs, ["runtime", "settings", "mods", "players", "maintenance", "gm"]);
    assert.deepEqual(report.archive_tabs, ["runtime", "settings", "mods", "players", "maintenance"]);
    assert.ok(report.checks >= 45, "Loading, cross-game fallback and independent mode memory must be verified");
    assert.deepEqual(report.browser_errors, []);
    assert.deepEqual(report.unexpected_writes, []);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.browser_exited, true);
    assert.equal(report.scratch_removed, true);
    console.log(`SERVER_DETAIL_NAVIGATION_BROWSER ${JSON.stringify({ viewport, checks: report.checks, status: report.status })}`);
  });
}
