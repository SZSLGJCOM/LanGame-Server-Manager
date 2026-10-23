const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("knowledge settings fold diagnostics, distinguish restricted sources and preserve update lifecycle", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "knowledge-settings-browser.html", keyboard: true, screenshotPath: process.env.LANGAME_KNOWLEDGE_SCREENSHOT });
  assert.equal(report.status, "passed", JSON.stringify(report));
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.checks, 15);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`KNOWLEDGE_BROWSER ${JSON.stringify(report)}`);
});
