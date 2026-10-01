const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("player workspace fills its tab with independent list and control scrolling", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "player-center-layout-browser.html",
    viewport: { width: 1280, height: 900 }, screenshotPath: process.env.LANGAME_PLAYER_CENTER_SCREENSHOT });
  assert.equal(report.status, "passed");
  assert.equal(report.checks, 29);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`PLAYER_CENTER_LAYOUT_BROWSER ${JSON.stringify(report)}`);
});
