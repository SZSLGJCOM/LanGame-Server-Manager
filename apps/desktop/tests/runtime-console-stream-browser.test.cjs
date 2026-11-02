const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("runtime console renders versioned partial output without per-event reads and recovers missing revisions", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "runtime-console-stream-browser.html",
    viewport: { width: 1280, height: 900 }, screenshotPath: process.env.LANGAME_RUNTIME_STREAM_SCREENSHOT });
  assert.equal(report.status, "passed", report.error);
  assert.equal(report.scenarios, 14);
  assert.equal(report.burst_document_updates, 1);
  assert.equal(report.normal_event_readbacks, 0);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`RUNTIME_CONSOLE_STREAM_BROWSER ${JSON.stringify(report)}`);
});
