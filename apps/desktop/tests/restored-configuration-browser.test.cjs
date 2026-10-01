const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("a restored server opens configuration during archive inventory and recovers local read errors", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "restored-configuration-browser.html", viewport: { width: 1280, height: 800 } });
  assert.equal(report.status, "passed", report.error);
  assert.ok(report.checks.length >= 15, "Restored configuration acceptance did not finish");
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.browser_exited, true);
  assert.equal(report.scratch_removed, true);
  console.log(`RESTORED_CONFIGURATION_BROWSER ${JSON.stringify({ status: report.status, checks: report.checks.length })}`);
});
