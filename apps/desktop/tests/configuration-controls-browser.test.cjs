const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("configuration controls share dimensions across generic, network and specialized editors", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "configuration-controls-browser.html",
    viewport: { width: 1560, height: 1000 }, screenshotPath: process.env.LANGAME_CONTROL_SCREENSHOT });
  console.log(`CONFIGURATION_CONTROLS_BROWSER ${JSON.stringify(report)}`);
  assert.equal(report.status, "passed");
  assert.equal(report.measurements.length, 19);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  assert.deepEqual(report.violations, []);
});
