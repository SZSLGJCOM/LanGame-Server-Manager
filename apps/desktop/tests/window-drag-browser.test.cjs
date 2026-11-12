const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const deviceScaleFactor of [1, 1.5]) {
  test(`header drag restoration, click protection, and cleanup at ${deviceScaleFactor}x`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "window-drag-browser.html", pointer: true,
      viewport: { width: 1560, height: 900 }, deviceScaleFactor });
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.cases, ["click-and-jitter", "maximized-pointer-drag", "windowed-drag",
      "double-click", "window-buttons", "controls-and-right-click", "gesture-cancellation", "unmount"]);
    assert.equal(report.device_scale, deviceScaleFactor);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
  });
}
