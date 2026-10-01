const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("the real player workbench keeps one roster and manual control through refreshes", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "player-center-refresh-browser.html" });
  assert.equal(report.status, "passed");
  assert.equal(report.checks, 6);
  assert.equal(report.props_refreshes, 40);
  assert.ok(report.snapshot_refreshes >= 22);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`PLAYER_CENTER_REFRESH_BROWSER ${JSON.stringify(report)}`);
});
