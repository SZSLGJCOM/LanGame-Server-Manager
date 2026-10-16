const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`compact save import, scope confirmation and recovery at ${viewport.width} x ${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "save-import-browser.html", viewport,
      screenshotPath: process.env.LANGAME_SAVE_IMPORT_SCREENSHOTS
        ? path.join(process.env.LANGAME_SAVE_IMPORT_SCREENSHOTS, `save-import-${viewport.width}.png`) : undefined });
    console.log(`SAVE_IMPORT_BROWSER ${JSON.stringify(report)}`);
    assert.equal(report.status, "passed", report.error);
    assert.ok(report.checks >= 50);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
  });
}
