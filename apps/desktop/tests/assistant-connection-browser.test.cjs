const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`connection checks retain request ownership and report real stages at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const screenshotPath = process.env.LANGAME_CONNECTION_SCREENSHOT_DIR
      ? path.join(process.env.LANGAME_CONNECTION_SCREENSHOT_DIR, `assistant-connection-${viewport.width}x${viewport.height}.png`) : undefined;
    const report = await runBrowserFixture({ fixturePath: "assistant-connection-browser.html", viewport, screenshotPath });
    assert.equal(report.status, "passed", JSON.stringify(report));
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.checks, 27);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`ASSISTANT_CONNECTION_BROWSER ${JSON.stringify({ viewport, ...report })}`);
  });
}
