const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("cached startup warnings show safe details and stay dismissed across polling and language changes", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "runtime-start-warnings-browser.html", keyboard: true,
    viewport: { width: 1280, height: 800 }, reducedMotion: "reduce", fixtureCleanup: true,
    screenshotPath: process.env.LANGAME_RUNTIME_START_WARNINGS_SCREENSHOT });
  assert.equal(report.status, "passed");
  assert.equal(report.scenarios, 9);
  assert.equal(report.dismissals, 2);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`RUNTIME_START_WARNINGS_BROWSER ${JSON.stringify(report)}`);
});
