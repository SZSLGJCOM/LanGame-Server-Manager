const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("roster actions confirm beside their buttons and native Enter never bypasses review", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "player-roster-inline-browser.html", keyboard: true,
    screenshotPath: process.env.LANGAME_ROSTER_SCREENSHOT });
  assert.equal(report.status, "passed");
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.checks, 17);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`ROSTER_INLINE_BROWSER ${JSON.stringify(report)}`);
});
