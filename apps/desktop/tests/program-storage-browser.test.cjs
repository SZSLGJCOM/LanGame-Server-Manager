const assert = require("node:assert/strict");
const test = require("node:test");
const path = require("node:path");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`shared program choices and maintenance at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const screenshotPath = process.env.LANGAME_PROGRAM_STORAGE_SCREENSHOT_DIR
      ? path.join(process.env.LANGAME_PROGRAM_STORAGE_SCREENSHOT_DIR, `program-storage-${viewport.width}x${viewport.height}.png`)
      : undefined;
    const report = await runBrowserFixture({ fixturePath: "program-storage-browser.html", viewport, screenshotPath });
    assert.equal(report.status, "passed");
    assert.equal(report.calls.length, 8);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.browser_exited, true);
    assert.equal(report.scratch_removed, true);
    console.log(`PROGRAM_STORAGE_BROWSER ${JSON.stringify({ viewport, ...report })}`);
  });
}
