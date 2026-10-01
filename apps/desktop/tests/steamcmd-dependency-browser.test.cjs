const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("SteamCMD removal, explicit preparation and independent installers keep real controls consistent", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "steamcmd-dependency-browser.html",
    viewport: { width: 960, height: 600 }, screenshotPath: process.env.LANGAME_DEPENDENCY_SCREENSHOT });
  assert.equal(report.status, "passed");
  assert.ok(report.checks >= 20);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`STEAMCMD_DEPENDENCY_BROWSER ${JSON.stringify(report)}`);
});
