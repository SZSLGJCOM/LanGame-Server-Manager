const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1280, height: 800 }, { width: 960, height: 600 }]) {
  test(`world regeneration icon, confirmation and progress hint ${viewport.width}`, { timeout: 60000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "dst-world-regeneration-browser.html", viewport, fixtureTimeoutMs: 30000 });
    assert.equal(report.status, "passed", report.error);
    assert.ok(report.checks >= 17);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.native_server_mutated, false);
    assert.deepEqual(report.scenarios, ["icon-confirm-cancel", "confirm-runs-in-background", "failure-visible", "lan-hidden"]);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`DST_WORLD_REGENERATION_BROWSER ${JSON.stringify(report)}`);
  });
}
