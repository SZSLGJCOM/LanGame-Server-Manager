const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("owned startup history survives completion, failure, cancellation and reload with positioned paging", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "runtime-startup-session-browser.html", viewport: { width: 1280, height: 900 } });
  assert.equal(report.status, "passed", report.error);
  assert.equal(report.scenarios, 18);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.browser_exited, true);
  assert.equal(report.scratch_removed, true);
  console.log(`RUNTIME_STARTUP_SESSION_BROWSER ${JSON.stringify(report)}`);
});
