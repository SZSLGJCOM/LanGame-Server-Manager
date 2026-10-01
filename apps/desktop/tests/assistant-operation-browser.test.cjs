const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("real assistant dialog preserves native focus, exact review text and confirmation lifecycle", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "assistant-operation-browser.html", keyboard: true,
    screenshotPath: process.env.LANGAME_ASSISTANT_SCREENSHOT });
  assert.equal(report.status, "passed");
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.checks, 21);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`ASSISTANT_BROWSER ${JSON.stringify(report)}`);
});
