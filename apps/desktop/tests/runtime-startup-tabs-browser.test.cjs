const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("startup state and progress survive tabs, page navigation and backend recovery", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "runtime-startup-tabs-browser.html",
    viewport: { width: 1560, height: 900 } });
  assert.equal(report.status, "passed", report.error);
  assert.equal(report.scenarios, 12);
  assert.equal(report.starts, 3);
  assert.equal(report.stops, 1);
  assert.equal(report.repairable_start_dispatched, true);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`RUNTIME_STARTUP_TABS_BROWSER ${JSON.stringify(report)}`);
});
