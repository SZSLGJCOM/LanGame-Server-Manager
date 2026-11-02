const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("download metrics never commit a previous observation's extra seconds", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "download-metrics-browser.html", fixtureCleanup: true });
  assert.equal(report.status, "passed", report.error);
  assert.equal(report.scenarios, 8);
  assert.ok(report.commits > 20);
  assert.ok(report.dom_changes > 0);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.fixture_cleanup.remaining_intervals, 0);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`DOWNLOAD_METRICS_BROWSER ${JSON.stringify(report)}`);
});
