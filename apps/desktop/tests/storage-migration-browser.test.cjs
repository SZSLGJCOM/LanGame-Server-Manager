const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const { viewport, deviceScaleFactor } of [
  { viewport: { width: 1560, height: 900 }, deviceScaleFactor: 1 },
  { viewport: { width: 960, height: 600 }, deviceScaleFactor: 1 },
  { viewport: { width: 1024, height: 768 }, deviceScaleFactor: 1.5 },
]) {
  test(`automatic storage migration and lifecycle at ${viewport.width} x ${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "storage-migration-browser.html", viewport, deviceScaleFactor, pointer: true,
      screenshotPath: process.env.LANGAME_MIGRATION_SCREENSHOTS
        ? path.join(process.env.LANGAME_MIGRATION_SCREENSHOTS, `storage-migration-${viewport.width}.png`) : undefined });
    assert.equal(report.status, "passed", report.error);
    assert.ok(report.checks >= 35);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`STORAGE_MIGRATION_BROWSER ${JSON.stringify(report)}`);
  });
}
