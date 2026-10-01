const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("foreground inventory reconciliation clears a deleted shell after detail polling pauses", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "instance-inventory-browser.html", viewport: { width: 1440, height: 900 }, fixtureCleanup: true });
  assert.equal(report.status, "passed");
  assert.equal(report.paused_reconciliation, true);
  assert.equal(report.inventory_failure_preserved, true);
  assert.ok(report.detail_reads >= 3);
  assert.equal(report.manual_deletion_calls, 0);
  assert.deepEqual(report.browser_errors, []);
  assert.deepEqual(report.fixture_cleanup.browser_errors, []);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`INSTANCE_INVENTORY_BROWSER ${JSON.stringify(report)}`);
});
