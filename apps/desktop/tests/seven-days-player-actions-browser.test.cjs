const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("7DTD real roster declarations expose concrete actions with trusted-identity forms and direct unban", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "seven-days-player-actions-browser.html",
    viewport: { width: 1280, height: 900 }, screenshotPath: process.env.LANGAME_SEVEN_DAYS_PLAYER_SCREENSHOT });
  assert.equal(report.status, "passed");
  assert.equal(report.checks, 14);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.module_id, "sevendaystodie");
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`SEVEN_DAYS_PLAYER_ACTIONS_BROWSER ${JSON.stringify(report)}`);
});
