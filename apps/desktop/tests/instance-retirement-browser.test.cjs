const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("instance retirement isolates stale reads while unrelated refresh and creation continue", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "instance-retirement-browser.html", viewport: { width: 1440, height: 900 },
    screenshotPath: process.env.LANGAME_INSTANCE_RETIREMENT_SCREENSHOT, fixtureCleanup: true });
  assert.equal(report.status, "passed");
  assert.deepEqual(report.scenarios, ["delete-failure", "delete-success", "unrelated-work-during-archive", "archive-navigation", "pending-unmount"]);
  assert.equal(report.retirement_requests, 4);
  assert.ok(report.discarded_reads >= 6);
  assert.equal(report.reads_after_retirement_started, 0);
  assert.equal(report.post_deletion_inventory_checks, 1);
  assert.ok(report.system_reads_during_retirement >= 1);
  assert.equal(report.creations_during_retirement, 1);
  assert.equal(report.module_changes_during_retirement, 2);
  assert.deepEqual(report.inventory_contentions, []);
  assert.deepEqual(report.library_installation_checks, ["before-delete:minecraft", "after-delete:minecraft",
    "other-module:corekeeper", "return-to-module:minecraft", "during-archive:corekeeper"]);
  assert.deepEqual(report.browser_errors, []);
  assert.deepEqual(report.fixture_cleanup.browser_errors, []);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`INSTANCE_RETIREMENT_BROWSER ${JSON.stringify(report)}`);
});
