const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("unavailable instances retain concise recovery controls and opt-in diagnostics", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "instance-unavailable-browser.html", viewport: { width: 1440, height: 900 },
    screenshotPath: process.env.LANGAME_INSTANCE_UNAVAILABLE_SCREENSHOT });
  assert.equal(report.status, "passed");
  assert.equal(report.retries, 1);
  assert.equal(report.delete_calls, 1);
  assert.equal(report.removal_inspections, 1);
  assert.equal(report.diagnostics_on_demand, true);
  assert.equal(report.running_deletion_blocked, true);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`INSTANCE_UNAVAILABLE_BROWSER ${JSON.stringify(report)}`);
});
