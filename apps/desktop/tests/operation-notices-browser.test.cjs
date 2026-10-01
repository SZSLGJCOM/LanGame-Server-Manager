const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 1366, height: 768 }]) {
  test(`operation retries preserve content layout at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "operation-notices-browser.html", keyboard: true, viewport });
    assert.equal(report.status, "passed");
    assert.equal(report.save_calls, 2);
    assert.equal(report.read_retries, 1);
    assert.equal(report.story_calls, 2);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.browser_exited, true);
    assert.equal(report.scratch_removed, true);
    console.log(`OPERATION_NOTICES_BROWSER ${JSON.stringify({ viewport, ...report })}`);
  });
}
