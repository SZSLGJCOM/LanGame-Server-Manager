const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`application updates prompt once per release with direct download and online update at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const screenshotPath = process.env.LANGAME_UPDATE_SCREENSHOT_DIR
      ? path.join(process.env.LANGAME_UPDATE_SCREENSHOT_DIR, `app-update-${viewport.width}x${viewport.height}.png`) : undefined;
    const report = await runBrowserFixture({ fixturePath: "app-update-browser.html", keyboard: true, viewport, screenshotPath, fixtureCleanup: true });
    assert.equal(report.status, "passed", JSON.stringify(report));
    assert.deepEqual(report.cases, ["disabled-build", "quiet-header", "automatic-prompt", "same-version-quiet", "new-version-and-later",
      "installer-page", "keyboard-focus", "online-update-once", "download-progress", "installing-and-failure",
      "activity-retry", "closed-progress", "stale-download-isolation", "theme-and-viewport", "browser-errors"]);
    assert.equal(report.install_calls, 2);
    assert.equal(report.check_calls, 1);
    assert.deepEqual(report.installer_urls, [
      "https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/tag/v0.4.0",
      "https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/tag/v0.4.0"
    ]);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`APP_UPDATE_BROWSER ${JSON.stringify({ viewport, ...report })}`);
  });
}
