const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("runtime console retains live history across polls and recovers after a reset", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "runtime-console-history-browser.html", keyboard: true,
    viewport: { width: 1280, height: 900 }, screenshotPath: process.env.LANGAME_RUNTIME_HISTORY_SCREENSHOT });
  assert.equal(report.status, "passed", report.error);
  assert.equal(report.scenarios, 26);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`RUNTIME_CONSOLE_HISTORY_BROWSER ${JSON.stringify(report)}`);
});
