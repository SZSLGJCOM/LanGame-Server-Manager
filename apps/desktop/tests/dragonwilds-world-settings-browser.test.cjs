const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const width of [1280, 1024]) {
  test(`Dragonwilds categorized native rules automatically save precise drafts at ${width}px`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({
      fixturePath: "dragonwilds-world-settings-browser.html", viewport: { width, height: 900 }, fixtureCleanup: true, pointer: true,
      screenshotPath: width === 1280 ? process.env.LANGAME_DRAGONWILDS_WORLD_SETTINGS_SCREENSHOT : undefined
    });
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.locales, ["en-US", "zh-CN"]);
    assert.ok(report.check_count >= 1200, "Peer navigation and complete native world rule behavior acceptance did not finish");
    assert.equal(report.loading_lifecycle_checks.length, 26, "StrictMode native read lifecycle acceptance did not finish in both languages");
    for (const outcome of ["immediate native success", "delayed native success clears loading", "delayed native failure clears loading",
      "late old-instance native success", "late old-instance native failure", "start await its unresolved read", "empty save leaves loading"]) {
      assert.equal(report.loading_lifecycle_checks.filter((check) => check.includes(outcome)).length, 2,
        `Missing native loading lifecycle case: ${outcome}`);
    }
    assert.deepEqual(report.browser_errors, []);
    assert.deepEqual(report.overflow_violations, []);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.browser_exited, true);
    assert.equal(report.scratch_removed, true);
    assert.deepEqual(report.fixture_cleanup.browser_errors, []);
    console.log(`DRAGONWILDS_WORLD_SETTINGS_BROWSER ${JSON.stringify({ width, status: report.status, checks: report.check_count })}`);
  });
}
