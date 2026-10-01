const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("manual player controls stay secondary without discarding an in-progress draft", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "manual-player-actions-browser.html" });
  assert.equal(report.status, "passed");
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.checks, 10);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`MANUAL_PLAYER_ACTIONS_BROWSER ${JSON.stringify(report)}`);
});
