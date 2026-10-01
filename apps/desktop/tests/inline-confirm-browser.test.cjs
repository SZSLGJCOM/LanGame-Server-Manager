const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("inline actions preserve confirmation, focus, lifecycle and real uninstall behavior", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "inline-confirm-browser.html", keyboard: true, fixtureCleanup: true,
    screenshotPath: process.env.LANGAME_INLINE_CONFIRM_SCREENSHOT });
  assert.equal(report.status, "passed");
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.checks, 28);
  assert.deepEqual(report.preview_variants, ["dark-510", "dark-300", "light-510", "light-300"]);
  assert.equal(report.native_dialogs, 0);
  assert.deepEqual(report.fixture_cleanup, { browser_errors: [], native_dialogs: 0, unmounted_fixtures: 5 });
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`INLINE_CONFIRM_BROWSER ${JSON.stringify(report)}`);
});
