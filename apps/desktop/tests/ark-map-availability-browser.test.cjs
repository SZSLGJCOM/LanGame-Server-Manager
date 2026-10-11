const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("ASA main and additional map choices use installed native packages and save their Mods atomically", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "ark-map-availability-browser.html", viewport: { width: 1560, height: 900 },
    screenshotPath: process.env.LANGAME_ARK_MAP_AVAILABILITY_SCREENSHOT, fixtureCleanup: true });
  assert.equal(report.status, "passed");
  assert.equal(report.map_saves, 2);
  assert.equal(report.installation_executed, false);
  assert.ok(report.checks >= 20);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`ARK_MAP_AVAILABILITY_BROWSER ${JSON.stringify(report)}`);
});
