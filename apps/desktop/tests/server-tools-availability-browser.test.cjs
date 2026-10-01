const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1280, height: 800 }, { width: 960, height: 600 }]) {
  test(`server tools availability and explanation at ${viewport.width} x ${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({
      fixturePath: "server-tools-availability-browser.html", keyboard: true, viewport,
      screenshotPath: process.env.LANGAME_TOOLS_SCREENSHOT_DIR
        ? path.join(process.env.LANGAME_TOOLS_SCREENSHOT_DIR, `server-tools-${viewport.width}x${viewport.height}.png`) : undefined
    });
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.viewport, viewport);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.supported_modules, 3);
    assert.equal(report.unsupported_modules, 29);
    assert.ok(report.checks >= 17, "Availability, tooltip and in-flight tab navigation checks must complete");
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`SERVER_TOOLS_AVAILABILITY_BROWSER ${JSON.stringify(report)}`);
  });
}
