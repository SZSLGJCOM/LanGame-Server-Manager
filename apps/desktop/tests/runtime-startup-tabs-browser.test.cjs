const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("server tabs receive startup progress while the runtime console is hidden", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "runtime-startup-tabs-browser.html",
    viewport: { width: 1560, height: 900 } });
  assert.equal(report.status, "passed", report.error);
  assert.equal(report.scenarios, 6);
  assert.equal(report.starts, 1);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`RUNTIME_STARTUP_TABS_BROWSER ${JSON.stringify(report)}`);
});
