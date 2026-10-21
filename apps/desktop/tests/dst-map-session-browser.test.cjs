const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1280, height: 800 }, { width: 960, height: 600 }]) {
  test(`map session migration, offline cache and world boundaries ${viewport.width}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "dst-map-session-browser.html", viewport, fixtureTimeoutMs: 60000 });
    assert.equal(report.status, "passed", report.error);
    assert.ok(report.checks >= 28);
    assert.deepEqual(report.browser_errors, []);
    assert.deepEqual(report.migration_shards, ["master", "caves", "islands", "volcano"]);
    assert.equal(report.real_indexed_db, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`DST_MAP_SESSION_BROWSER ${JSON.stringify(report)}`);
  });
}
