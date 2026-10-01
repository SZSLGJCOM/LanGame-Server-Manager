const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("Steam updates follow language changes and recover through a localized keyboard retry", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "library-updates-browser.html", keyboard: true,
    viewport: { width: 1280, height: 900 }, screenshotPath: process.env.LANGAME_UPDATES_SCREENSHOT });
  assert.equal(report.status, "passed");
  assert.deepEqual(report.scenarios, ["language-request", "english-keyboard-retry", "locale-cache", "obsolete-response", "failed-locale-switch", "chinese-retry"]);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`LIBRARY_UPDATES_BROWSER ${JSON.stringify(report)}`);
});
