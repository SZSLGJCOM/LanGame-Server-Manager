const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("real assistant composer accepts natural-language tasks and clarification without manual policy controls", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "assistant-intent-browser.html",
    screenshotPath: process.env.LANGAME_ASSISTANT_INTENT_SCREENSHOT });
  assert.equal(report.status, "passed");
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.checks, 18);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`ASSISTANT_INTENT_BROWSER ${JSON.stringify(report)}`);
});
