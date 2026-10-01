const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("player actions retain request ownership through selection, refresh and instance changes", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "player-action-lifecycle-browser.html" });
  assert.equal(report.status, "passed");
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.checks, 13);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`PLAYER_ACTION_LIFECYCLE_BROWSER ${JSON.stringify(report)}`);
});
