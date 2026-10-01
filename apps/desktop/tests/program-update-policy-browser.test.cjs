const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("program update policy preserves data through keyboard saves, conflicts, retries and readback", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "program-update-policy-browser.html", keyboard: true,
    viewport: { width: 1280, height: 900 }, screenshotPath: process.env.LANGAME_PROGRAM_POLICY_SCREENSHOT });
  assert.equal(report.status, "passed");
  assert.equal(report.checks, 16);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`PROGRAM_UPDATE_POLICY_BROWSER ${JSON.stringify(report)}`);
});
