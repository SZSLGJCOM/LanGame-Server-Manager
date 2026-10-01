const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("ARK editions manage maps in one instance and route each LanGameCMD through its map's RCON", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "ark-cluster-maps-browser.html", keyboard: true,
    viewport: { width: 1560, height: 900 }, screenshotPath: process.env.LANGAME_ARK_MAPS_SCREENSHOT, fixtureCleanup: true });
  assert.equal(report.status, "passed");
  assert.equal(report.editions, 2);
  assert.equal(report.console_tabs, 3);
  assert.equal(report.commands, 2);
  assert.ok(report.checks >= 43);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`ARK_MAPS_BROWSER ${JSON.stringify(report)}`);
});
