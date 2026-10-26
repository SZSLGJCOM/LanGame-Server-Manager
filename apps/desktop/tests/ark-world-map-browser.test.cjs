const assert = require('node:assert/strict');
const path = require('node:path');
const test = require('node:test');
const { runBrowserFixture } = require('./helpers/runtime-browser-process.cjs');
for (const viewport of [{ width: 1280, height: 800 }, { width: 960, height: 600 }]) {
  test(`ARK map scopes, live selection and tools at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: 'ark-world-map-browser.html', viewport, pointer: true,
      screenshotPath: process.env.LANGAME_TOOLS_SCREENSHOT_DIR
        ? path.join(process.env.LANGAME_TOOLS_SCREENSHOT_DIR, `ark-map-${viewport.width}x${viewport.height}.png`) : undefined });
    assert.equal(report.status, 'passed', report.error);
    assert.deepEqual(report.browser_errors, []);
    assert.ok(report.checks >= 30);
    assert.equal(report.maximum_map_requests, 1);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`ARK_MAP_BROWSER ${JSON.stringify(report)}`);
  });
}
