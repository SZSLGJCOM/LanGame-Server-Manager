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
    for (const locale of report.locales) {
      for (const check of [
        "version 42 saved metadata displays concrete current and next rules in the read-only table",
        "native status uses the real activity notice store outside configuration DOM",
        "choosing Easy over saved Hard preserves native residual keys and projects Custom",
        "choosing Easy over unknown saved rules keeps the partial native projection unknown",
        "an explicit Normal baseline makes the Easy startup projection determinate",
        "removing an enabled world's startup override retains its effective saved rule",
        "failed readback leaves every valid configuration control editable without fabricated defaults",
        "explicit startup overrides have known projections even when native readback fails",
        "native retry recovers readback and retains edited startup overrides",
        "identical runtime polling does not reread metadata",
        "a new run identity refreshes metadata once",
        "a completed stop refreshes metadata exactly once",
        "a late result from an earlier run cannot replace current readback",
        "new-world readback projects native defaults without storing them in configuration",
        "missing metadata is isolated to readback and leaves startup editing available"
      ]) assert.ok(report.checks.includes(`${locale}: ${check}`), `Missing acceptance: ${locale}: ${check}`);
    }
    assert.deepEqual(report.browser_errors, []);
    assert.deepEqual(report.overflow_violations, []);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.browser_exited, true);
    assert.equal(report.scratch_removed, true);
    assert.deepEqual(report.fixture_cleanup.browser_errors, []);
    console.log(`VALHEIM_WORLD_RULES_BROWSER ${JSON.stringify({ width, status: report.status, checks: report.checks.length })}`);
  });
}
