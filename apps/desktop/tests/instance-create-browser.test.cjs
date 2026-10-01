const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("instance creation stays visible and rejects duplicate submissions until completion", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "instance-create-browser.html", keyboard: true,
    viewport: { width: 1280, height: 900 }, screenshotPath: process.env.LANGAME_CREATE_SCREENSHOT });
  assert.equal(report.status, "passed");
  assert.equal(report.requests, 4);
  assert.equal(report.opened, 2);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`INSTANCE_CREATE_BROWSER ${JSON.stringify(report)}`);
});
