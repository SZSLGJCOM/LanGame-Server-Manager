const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1280, height: 900 }, { width: 1366, height: 768 }]) {
  test(`runtime read failures stay inside the console at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "runtime-console-errors-browser.html", keyboard: true, viewport,
      screenshotPath: viewport.width === 1280 ? process.env.LANGAME_RUNTIME_ERRORS_SCREENSHOT : undefined });
    assert.equal(report.status, "passed");
    assert.equal(report.scenarios, 7);
    assert.equal(report.commands_sent, 1);
    assert.equal(report.retries, 5);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`RUNTIME_CONSOLE_ERRORS_BROWSER ${JSON.stringify({ viewport, ...report })}`);
  });
}
