const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("saves autosave chip follows native save capability and explains unsupported games", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "saves-autosave-capability-browser.html", keyboard: true,
    viewport: { width: 1100, height: 700 }, screenshotPath: process.env.LANGAME_SAVES_CAPABILITY_SCREENSHOT });
  assert.equal(report.status, "passed", report.error);
  assert.equal(report.checks, 14);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
});
