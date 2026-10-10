const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const width of [1280, 1024]) {
  test(`Valheim world rules persist through configuration reopening at ${width}px`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({
      fixturePath: "valheim-world-rules-browser.html",
      viewport: { width, height: 900 },
      fixtureCleanup: true,
      screenshotPath: width === 1280 ? process.env.LANGAME_VALHEIM_RULES_SCREENSHOT : undefined
    });
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.locales, ["en-US", "zh-CN"]);
    assert.ok(report.checks.length >= 60, "World rule acceptance did not finish");
    assert.deepEqual(report.browser_errors, []);
    assert.deepEqual(report.overflow_violations, []);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.browser_exited, true);
    assert.equal(report.scratch_removed, true);
    assert.deepEqual(report.fixture_cleanup.browser_errors, []);
    console.log(`VALHEIM_WORLD_RULES_BROWSER ${JSON.stringify({ width, status: report.status, checks: report.checks.length })}`);
  });
}
